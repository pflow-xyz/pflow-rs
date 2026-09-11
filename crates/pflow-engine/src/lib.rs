//! State machine harness for continuous Petri net simulation, ported from
//! go-pflow's `engine` package (`engine/engine.go`).
//!
//! The unified ODE + event-sourced runtime petri-pilot's `pflow-engine.js`
//! derives from (ROADMAP.md Phase 5). Maintains a live simulation in memory
//! that continually updates state and triggers actions when the state meets
//! a declared condition, over the same [`pflow_core::net::PetriNet`] +
//! [`pflow_solver`] pairing go-pflow's version uses — not
//! [`pflow_metamodel::Model`], because that is what the source actually
//! builds on (`petri.PetriNet`, `solver.NewProblem`), and this is a direct
//! port rather than a redesign.
//!
//! # Concurrency, ported idiomatically rather than literally
//!
//! Go's `Engine` guards its fields with a `sync.RWMutex` and runs
//! [`Engine::run`]'s loop in a goroutine cancelled via `context.CancelFunc`.
//! Rust has no goroutines; [`Engine`] instead wraps its mutable fields in a
//! [`std::sync::Mutex`] (readers and writers alike — `RwLock`'s extra
//! complexity buys nothing here, since every access here also needs `&self`
//! through a shared engine) and [`Engine::run`] spawns a
//! [`std::thread`], stopped by dropping a channel sender rather than
//! cancelling a context — the same "tell the loop to stop, then wait for
//! its own next tick to notice" shape, achieved with the tools Rust has.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use pflow_core::net::PetriNet;
use pflow_core::State;
use pflow_solver::{solve, Options, Problem, Solution};

/// A predicate on the system state. Triggers when it returns `true`.
pub type Condition = Box<dyn Fn(&State) -> bool + Send + Sync>;

/// An action triggered when a [`Rule`]'s condition holds. Receives the
/// state at the moment the condition fired and may return an error, which
/// [`Engine::check_rules`] swallows exactly as go-pflow's `TODO: Add error
/// handling/logging` comment says its own version does — see that
/// function's doc.
pub type Action = Box<dyn Fn(&State) -> Result<(), String> + Send + Sync>;

/// Pairs a condition with an action to trigger when it holds.
pub struct Rule {
    pub name: String,
    pub condition: Condition,
    pub action: Action,
    pub enabled: bool,
}

struct Inner {
    state: State,
    rates: HashMap<String, f64>,
    rules: Vec<Rule>,
}

/// Maintains a live Petri net simulation with continuous state updates and
/// condition-action rule triggers.
pub struct Engine {
    net: PetriNet,
    inner: Mutex<Inner>,
    running: AtomicBool,
    stop_tx: Mutex<Option<mpsc::Sender<()>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Engine {
    /// Creates a new engine for `net`. `None` for either argument falls
    /// back to the net's own declared defaults, via
    /// [`PetriNet::set_state`]/[`PetriNet::set_rates`].
    pub fn new(net: PetriNet, initial_state: Option<State>, rates: Option<HashMap<String, f64>>) -> Self {
        let state = match initial_state {
            Some(s) => net.set_state(Some(&s)),
            None => net.set_state(None),
        };
        let rates = match rates {
            Some(r) => net.set_rates(Some(&r)),
            None => net.set_rates(None),
        };
        Self {
            net,
            inner: Mutex::new(Inner {
                state,
                rates,
                rules: Vec::new(),
            }),
            running: AtomicBool::new(false),
            stop_tx: Mutex::new(None),
            handle: Mutex::new(None),
        }
    }

    /// Adds a condition-action rule.
    pub fn add_rule(&self, name: impl Into<String>, condition: Condition, action: Action) {
        let mut inner = self.inner.lock().unwrap();
        inner.rules.push(Rule {
            name: name.into(),
            condition,
            action,
            enabled: true,
        });
    }

    /// A copy of the current state.
    pub fn get_state(&self) -> State {
        self.inner.lock().unwrap().state.clone()
    }

    /// Merges `state` into the current state (overwriting named entries,
    /// leaving the rest untouched — matches go-pflow's `SetState`, which is
    /// a merge despite the name).
    pub fn set_state(&self, state: &State) {
        let mut inner = self.inner.lock().unwrap();
        for (k, v) in state {
            inner.state.insert(k.clone(), *v);
        }
    }

    /// Merges `rates` into the current transition rates.
    pub fn update_rates(&self, rates: &HashMap<String, f64>) {
        let mut inner = self.inner.lock().unwrap();
        for (k, v) in rates {
            inner.rates.insert(k.clone(), *v);
        }
    }

    /// Evaluates every enabled rule against the current state and runs the
    /// action for each satisfied condition. An action's error is discarded
    /// — go-pflow's own `checkRules` has the identical `_ = err` no-op with
    /// a `TODO`; this is a faithful port of that gap, not an improvement on
    /// it, since fixing it would need a caller-supplied error sink this
    /// crate's own callers do not have yet.
    fn check_rules(&self) {
        let (state_copy, rule_count) = {
            let inner = self.inner.lock().unwrap();
            (inner.state.clone(), inner.rules.len())
        };
        for i in 0..rule_count {
            let (enabled, fires) = {
                let inner = self.inner.lock().unwrap();
                let rule = &inner.rules[i];
                (rule.enabled, rule.enabled && (rule.condition)(&state_copy))
            };
            if enabled && fires {
                let inner = self.inner.lock().unwrap();
                let _ = (inner.rules[i].action)(&state_copy);
            }
        }
    }

    /// Advances the simulation by one time step of length `dt` using ODE
    /// integration, then runs [`Engine::check_rules`]. Returns the new
    /// state.
    pub fn step(&self, dt: f64) -> State {
        let (current_state, current_rates) = {
            let inner = self.inner.lock().unwrap();
            (inner.state.clone(), inner.rates.clone())
        };

        let prob = Problem::new(self.net.clone(), current_state, [0.0, dt], current_rates);
        let opts = Options {
            dt: dt / 10.0,
            dtmin: 1e-9,
            dtmax: dt,
            abstol: 1e-9,
            reltol: 1e-6,
            maxiters: 1000,
            adaptive: true,
        };
        let solver = pflow_solver::methods::tsit5();
        let sol: Solution = solve(&prob, &solver, &opts);

        let new_state = sol.get_final_state().cloned().unwrap_or_default();
        {
            let mut inner = self.inner.lock().unwrap();
            inner.state = new_state;
        }

        self.check_rules();
        self.get_state()
    }

    /// Starts the continuous simulation loop, ticking every `interval` and
    /// advancing by `dt` model time each tick, in a background thread. A
    /// second call while already running is a no-op, matching go-pflow.
    pub fn run(self: &Arc<Self>, interval: Duration, dt: f64) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let (tx, rx) = mpsc::channel::<()>();
        *self.stop_tx.lock().unwrap() = Some(tx);

        let engine = Arc::clone(self);
        let join = std::thread::spawn(move || loop {
            match rx.recv_timeout(interval) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    engine.running.store(false, Ordering::SeqCst);
                    return;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    engine.step(dt);
                }
            }
        });
        *self.handle.lock().unwrap() = Some(join);
    }

    /// Halts the continuous simulation loop started by [`Engine::run`].
    pub fn stop(&self) {
        if let Some(tx) = self.stop_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.lock().unwrap().take() {
            let _ = handle.join();
        }
        self.running.store(false, Ordering::SeqCst);
    }

    /// Whether the continuous simulation loop is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Runs a batch (non-continuous) simulation for `duration` model time
    /// and returns the full solution, for analysis rather than live state.
    pub fn simulate(&self, duration: f64, opts: &Options) -> Solution {
        let (current_state, current_rates) = {
            let inner = self.inner.lock().unwrap();
            (inner.state.clone(), inner.rates.clone())
        };
        let prob = Problem::new(self.net.clone(), current_state, [0.0, duration], current_rates);
        let solver = pflow_solver::methods::tsit5();
        solve(&prob, &solver, opts)
    }
}

// --- Example condition functions, ported from go-pflow's engine.go ---

/// A condition that triggers when `place` exceeds `threshold`.
pub fn threshold_exceeded(place: impl Into<String>, threshold: f64) -> Condition {
    let place = place.into();
    Box::new(move |state: &State| state.get(&place).copied().unwrap_or(0.0) > threshold)
}

/// A condition that triggers when `place` falls below `threshold`.
pub fn threshold_below(place: impl Into<String>, threshold: f64) -> Condition {
    let place = place.into();
    Box::new(move |state: &State| state.get(&place).copied().unwrap_or(0.0) < threshold)
}

/// A condition that triggers when every given condition is true.
pub fn all_of(conditions: Vec<Condition>) -> Condition {
    Box::new(move |state: &State| conditions.iter().all(|c| c(state)))
}

/// A condition that triggers when any given condition is true.
pub fn any_of(conditions: Vec<Condition>) -> Condition {
    Box::new(move |state: &State| conditions.iter().any(|c| c(state)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::net::PetriNet;

    fn simple_net() -> PetriNet {
        // A -> t1 -> B, first-order kinetics.
        PetriNet::build()
            .place("A", 10.0)
            .place("B", 0.0)
            .transition("t1")
            .arc("A", "t1", 1.0)
            .arc("t1", "B", 1.0)
            .done()
    }

    #[test]
    fn new_engine_defaults_state_and_rates_from_the_net() {
        let net = simple_net();
        let engine = Engine::new(net, None, None);
        let state = engine.get_state();
        assert_eq!(state.get("A"), Some(&10.0));
        assert_eq!(state.get("B"), Some(&0.0));
    }

    #[test]
    fn set_state_merges_rather_than_replaces() {
        let net = simple_net();
        let engine = Engine::new(net, None, None);
        let mut patch = State::new();
        patch.insert("A".to_string(), 5.0);
        engine.set_state(&patch);
        let state = engine.get_state();
        assert_eq!(state.get("A"), Some(&5.0));
        assert_eq!(state.get("B"), Some(&0.0), "B must survive an A-only patch");
    }

    #[test]
    fn step_moves_mass_from_a_to_b() {
        let net = simple_net();
        let mut rates = HashMap::new();
        rates.insert("t1".to_string(), 1.0);
        let engine = Engine::new(net, None, Some(rates));

        let before = engine.get_state()["A"];
        engine.step(1.0);
        let after = engine.get_state();
        assert!(after["A"] < before, "A must decrease as t1 fires");
        assert!(after["B"] > 0.0, "B must gain mass");
    }

    #[test]
    fn threshold_condition_triggers_an_action() {
        let net = simple_net();
        let mut rates = HashMap::new();
        rates.insert("t1".to_string(), 1.0);
        let engine = Engine::new(net, None, Some(rates));

        let fired = Arc::new(Mutex::new(false));
        let fired_clone = Arc::clone(&fired);
        engine.add_rule(
            "b-has-mass",
            threshold_exceeded("B", 0.01),
            Box::new(move |_state| {
                *fired_clone.lock().unwrap() = true;
                Ok(())
            }),
        );

        for _ in 0..20 {
            engine.step(1.0);
            if *fired.lock().unwrap() {
                break;
            }
        }
        assert!(*fired.lock().unwrap(), "the threshold rule must eventually fire");
    }

    #[test]
    fn all_of_and_any_of_compose_conditions() {
        let mut a = State::new();
        a.insert("x".to_string(), 5.0);
        a.insert("y".to_string(), 5.0);

        let all = all_of(vec![
            threshold_exceeded("x", 1.0),
            threshold_exceeded("y", 1.0),
        ]);
        assert!(all(&a));

        let any = any_of(vec![threshold_exceeded("x", 100.0), threshold_below("y", 100.0)]);
        assert!(any(&a));
    }

    #[test]
    fn run_and_stop_toggle_is_running() {
        let net = simple_net();
        let engine = Arc::new(Engine::new(net, None, Some(HashMap::new())));
        assert!(!engine.is_running());
        engine.run(Duration::from_millis(10), 0.1);
        assert!(engine.is_running());
        engine.stop();
        assert!(!engine.is_running());
    }

    #[test]
    fn simulate_is_a_batch_operation_that_does_not_mutate_live_state() {
        let net = simple_net();
        let mut rates = HashMap::new();
        rates.insert("t1".to_string(), 1.0);
        let engine = Engine::new(net, None, Some(rates));

        let before = engine.get_state();
        let sol = engine.simulate(5.0, &Options::default_opts());
        assert!(sol.get_final_state().is_some());
        assert_eq!(engine.get_state(), before, "simulate must not touch live state");
    }
}
