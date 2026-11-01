//! ODE solver core: Problem, Solution, Options, Solve, mass-action kinetics.
//!
//! Mass action has no read or inhibitor arcs. A [`Problem`] refuses a net that
//! has them unless the caller says what to do with them ([`ContextualArcs`]),
//! the same refusal the SDE gives ([`crate::ssa::sde`]). go-pflow's
//! `solver/ode.go` does not refuse: it integrates such an arc as a flow arc, so
//! an inhibitor drains the place that should block the transition. No ODE
//! parity fixture has a contextual arc, so nothing pins that behaviour.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use pflow_core::net::{Arc, PetriNet};
use pflow_core::{ArcKind, State};

use crate::methods::Solver;
use crate::ssa::sde::{inhibitor_arc_reason, read_arc_reason};

/// A function that computes the derivative du/dt given time t and state u.
pub type ODEFunc = Box<dyn Fn(f64, &State) -> State>;

// Internal vectorized ODE function: (t, u) -> du using dense arrays.
type VecF = Box<dyn Fn(f64, &[f64]) -> Vec<f64>>;

/// What a [`Problem`] does with a net's read and inhibitor arcs.
///
/// Mirrors the `contextual` keyword of pflow-jl's `to_labelled_petri_net`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContextualArcs {
    /// Refuse the net. The default: neither kind of arc has a mass-action form.
    #[default]
    Refuse,
    /// Encode a read arc as consume-then-restore, a self-loop of the same
    /// weight. The place then scales the flux and nets no change, which is
    /// exact under interleaving semantics, not under partial-order semantics.
    /// An inhibitor has no such encoding, so it is still refused.
    SelfLoop,
    /// Ignore every read and inhibitor arc, as if it were not drawn.
    Drop,
}

impl FromStr for ContextualArcs {
    type Err = String;

    /// `refuse`, `selfloop` or `drop`: the spelling the CLI and MCP tools take.
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "refuse" => Ok(ContextualArcs::Refuse),
            "selfloop" => Ok(ContextualArcs::SelfLoop),
            "drop" => Ok(ContextualArcs::Drop),
            other => Err(format!(
                "contextual must be refuse, selfloop or drop, not {other:?}"
            )),
        }
    }
}

/// One read or inhibitor arc named in a [`ContextualArcError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextualArc {
    pub place: String,
    pub transition: String,
}

/// A net has read or inhibitor arcs that the chosen [`ContextualArcs`] does
/// not resolve. The lists follow the net's arc order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextualArcError {
    pub reads: Vec<ContextualArc>,
    pub inhibitors: Vec<ContextualArc>,
    pub policy: ContextualArcs,
}

impl fmt::Display for ContextualArcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut reasons = Vec::new();
        if !self.reads.is_empty() {
            let arcs: Vec<String> = self
                .reads
                .iter()
                .map(|a| format!("{} -o {}", a.transition, a.place))
                .collect();
            reasons.push(format!(
                "{} ({})",
                read_arc_reason(self.reads.len()),
                arcs.join(", ")
            ));
        }
        if !self.inhibitors.is_empty() {
            let arcs: Vec<String> = self
                .inhibitors
                .iter()
                .map(|a| format!("{} -o {}", a.place, a.transition))
                .collect();
            reasons.push(format!(
                "{} ({})",
                inhibitor_arc_reason(self.inhibitors.len()),
                arcs.join(", ")
            ));
        }
        let hint = match self.policy {
            ContextualArcs::SelfLoop => {
                "inhibitor arcs cannot be encoded as self-loops; use drop to ignore them"
            }
            _ => "use selfloop (read arcs only) or drop to integrate the net anyway",
        };
        write!(f, "{}; {hint}", reasons.join("; "))
    }
}

impl std::error::Error for ContextualArcError {}

/// Applies `policy`: the net with no contextual arc left, or the refusal.
fn resolve_contextual(
    mut net: PetriNet,
    policy: ContextualArcs,
) -> Result<PetriNet, ContextualArcError> {
    let mut reads = Vec::new();
    let mut inhibitors = Vec::new();
    for a in &net.arcs {
        match net.arc_kind(a) {
            ArcKind::Normal => {}
            ArcKind::Read => reads.push(ContextualArc {
                place: a.target.clone(),
                transition: a.source.clone(),
            }),
            ArcKind::Inhibitor => inhibitors.push(ContextualArc {
                place: a.source.clone(),
                transition: a.target.clone(),
            }),
        }
    }
    if reads.is_empty() && inhibitors.is_empty() {
        return Ok(net);
    }
    match policy {
        ContextualArcs::Refuse => {
            return Err(ContextualArcError {
                reads,
                inhibitors,
                policy,
            })
        }
        ContextualArcs::SelfLoop if !inhibitors.is_empty() => {
            return Err(ContextualArcError {
                reads: Vec::new(),
                inhibitors,
                policy,
            })
        }
        ContextualArcs::SelfLoop | ContextualArcs::Drop => {}
    }

    let drawn = std::mem::take(&mut net.arcs);
    for a in drawn {
        if !a.inhibit_transition {
            net.arcs.push(a);
        } else if policy == ContextualArcs::SelfLoop {
            // Drawn transition -> place: a read arc, since an inhibitor has
            // already been refused above.
            net.arcs
                .push(Arc::new(&a.target, &a.source, a.weight.clone(), false));
            net.arcs.push(Arc::new(a.source, a.target, a.weight, false));
        }
    }
    Ok(net)
}

/// An ODE initial value problem for a Petri net.
pub struct Problem {
    /// The net as integrated: with [`ContextualArcs::SelfLoop`] or
    /// [`ContextualArcs::Drop`], its contextual arcs are already resolved.
    pub net: PetriNet,
    pub u0: State,
    pub tspan: [f64; 2],
    pub rates: HashMap<String, f64>,
    pub f: ODEFunc,
    pub state_labels: Vec<String>,
    // Vectorized internals for fast solve()
    #[allow(dead_code)]
    state_index: HashMap<String, usize>,
    vec_u0: Vec<f64>,
    vec_f: VecF,
}

impl Problem {
    /// Creates a new ODE problem from a Petri net, refusing a net with read or
    /// inhibitor arcs. Use [`Problem::with_contextual`] to integrate one anyway.
    pub fn new(
        net: PetriNet,
        initial_state: State,
        tspan: [f64; 2],
        rates: HashMap<String, f64>,
    ) -> Result<Self, ContextualArcError> {
        Self::with_contextual(net, initial_state, tspan, rates, ContextualArcs::Refuse)
    }

    /// [`Problem::new`], with what to do about read and inhibitor arcs stated.
    pub fn with_contextual(
        net: PetriNet,
        initial_state: State,
        tspan: [f64; 2],
        rates: HashMap<String, f64>,
        contextual: ContextualArcs,
    ) -> Result<Self, ContextualArcError> {
        let net = resolve_contextual(net, contextual)?;
        let f = build_ode_function(&net, &rates);
        // Sorted, not `HashMap` order: the state vector's layout (and the
        // public `state_labels` order) must not vary between instances.
        let state_labels = sorted_labels(initial_state.keys());
        let state_index: HashMap<String, usize> = state_labels
            .iter()
            .enumerate()
            .map(|(i, label)| (label.clone(), i))
            .collect();
        let vec_u0: Vec<f64> = state_labels
            .iter()
            .map(|label| initial_state.get(label).copied().unwrap_or(0.0))
            .collect();
        let n_places = state_labels.len();
        let vec_f = build_vec_ode_function(&net, &rates, &state_index, n_places);
        Ok(Self {
            net,
            u0: initial_state,
            tspan,
            rates,
            f,
            state_labels,
            state_index,
            vec_u0,
            vec_f,
        })
    }
}

/// Collects labels in lexicographic order.
///
/// Every place/transition walk that builds an index or accumulates floats goes
/// through this, so results do not depend on `HashMap` iteration order (which
/// is randomized per map instance). Summing a place's flux terms in a
/// different order changes the last bits of `du`, and an adaptive step
/// controller can amplify that into a different accepted step. Same rule
/// `pflow-learn`'s `LearnableProblem` applies to its state vector.
///
/// go-pflow's `solver` iterates Go maps here (randomized per run), so no fixed
/// order matches it on every run; lexicographic is the reproducible choice.
fn sorted_labels<'a>(labels: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut v: Vec<String> = labels.cloned().collect();
    v.sort();
    v
}

/// Constructs the ODE derivative function for a Petri net using mass-action kinetics.
fn build_ode_function(net: &PetriNet, rates: &HashMap<String, f64>) -> ODEFunc {
    // Pre-compute structure for the closure. Transitions are walked in sorted
    // order so each place's flux terms accumulate deterministically.
    let place_labels = sorted_labels(net.places.keys());
    let trans_labels = sorted_labels(net.transitions.keys());
    let arcs: Vec<(String, String, f64)> = net
        .arcs
        .iter()
        .map(|a| (a.source.clone(), a.target.clone(), a.weight_sum()))
        .collect();
    let place_set: std::collections::HashSet<String> = net.places.keys().cloned().collect();
    let rates = rates.clone();

    Box::new(move |_t: f64, u: &State| -> State {
        let mut du: State = place_labels.iter().map(|l| (l.clone(), 0.0)).collect();

        for trans_label in &trans_labels {
            let rate = rates.get(trans_label).copied().unwrap_or(1.0);
            let mut flux = rate;

            // Compute flux using mass-action kinetics
            for (source, target, _weight) in &arcs {
                if target == trans_label && place_set.contains(source) {
                    let place_state = u.get(source).copied().unwrap_or(0.0);
                    if place_state <= 0.0 {
                        flux = 0.0;
                        break;
                    }
                    flux *= place_state;
                }
            }

            // Apply flux to connected places
            if flux > 0.0 {
                for (source, target, weight) in &arcs {
                    if target == trans_label && place_set.contains(source) {
                        // Input arc: consume tokens
                        if let Some(v) = du.get_mut(source) {
                            *v -= flux * weight;
                        }
                    } else if source == trans_label && place_set.contains(target) {
                        // Output arc: produce tokens
                        if let Some(v) = du.get_mut(target) {
                            *v += flux * weight;
                        }
                    }
                }
            }
        }
        du
    })
}

/// Constructs a vectorized ODE derivative function with pre-indexed arcs.
///
/// This replaces HashMap lookups with array indexing and pre-groups arcs
/// by transition, reducing per-call cost from O(T*A) to O(A).
fn build_vec_ode_function(
    net: &PetriNet,
    rates: &HashMap<String, f64>,
    state_index: &HashMap<String, usize>,
    n_places: usize,
) -> VecF {
    // Pre-group arcs by transition: O(A) construction
    let mut input_map: HashMap<&str, Vec<(usize, f64)>> = HashMap::new();
    let mut output_map: HashMap<&str, Vec<(usize, f64)>> = HashMap::new();

    for arc in &net.arcs {
        let w = arc.weight_sum();
        if net.transitions.contains_key(&arc.target) {
            if let Some(&idx) = state_index.get(&arc.source) {
                input_map
                    .entry(arc.target.as_str())
                    .or_default()
                    .push((idx, w));
            }
        }
        if net.transitions.contains_key(&arc.source) {
            if let Some(&idx) = state_index.get(&arc.target) {
                output_map
                    .entry(arc.source.as_str())
                    .or_default()
                    .push((idx, w));
            }
        }
    }

    // Build compact transition table: (rate, inputs, outputs), in sorted
    // transition order so `du[idx]` accumulates in the same order every time.
    let transitions: Vec<(f64, Vec<(usize, f64)>, Vec<(usize, f64)>)> =
        sorted_labels(net.transitions.keys())
            .iter()
            .map(|label| {
                let rate = rates.get(label).copied().unwrap_or(1.0);
                let inputs = input_map.remove(label.as_str()).unwrap_or_default();
                let outputs = output_map.remove(label.as_str()).unwrap_or_default();
                (rate, inputs, outputs)
            })
            .collect();

    Box::new(move |_t: f64, u: &[f64]| -> Vec<f64> {
        let mut du = vec![0.0; n_places];

        for (rate, inputs, outputs) in &transitions {
            let mut flux = *rate;

            // Mass-action kinetics: flux = rate * product(input tokens)
            for &(idx, _w) in inputs {
                let v = u[idx];
                if v <= 0.0 {
                    flux = 0.0;
                    break;
                }
                flux *= v;
            }

            if flux > 0.0 {
                for &(idx, w) in inputs {
                    du[idx] -= flux * w;
                }
                for &(idx, w) in outputs {
                    du[idx] += flux * w;
                }
            }
        }

        du
    })
}

/// The solution to an ODE problem.
pub struct Solution {
    pub t: Vec<f64>,
    pub u: Vec<State>,
    pub state_labels: Vec<String>,
}

impl Solution {
    /// Extracts the time series for a specific state variable by label.
    pub fn get_variable(&self, label: &str) -> Vec<f64> {
        self.u
            .iter()
            .map(|s| s.get(label).copied().unwrap_or(0.0))
            .collect()
    }

    /// Returns the final state of the system.
    pub fn get_final_state(&self) -> Option<&State> {
        self.u.last()
    }

    /// Returns the state at a specific time point index.
    pub fn get_state(&self, i: usize) -> Option<&State> {
        self.u.get(i)
    }
}

/// Solver configuration parameters.
#[derive(Debug, Clone)]
pub struct Options {
    pub dt: f64,
    pub dtmin: f64,
    pub dtmax: f64,
    pub abstol: f64,
    pub reltol: f64,
    pub maxiters: usize,
    pub adaptive: bool,
}

impl Options {
    /// Default solver options — balanced for most problems.
    pub fn default_opts() -> Self {
        Self {
            dt: 0.01,
            dtmin: 1e-6,
            dtmax: 0.1,
            abstol: 1e-6,
            reltol: 1e-3,
            maxiters: 100_000,
            adaptive: true,
        }
    }

    /// Options that match the pflow.xyz JavaScript solver.
    pub fn js_parity() -> Self {
        Self {
            dt: 0.01,
            dtmin: 1e-6,
            dtmax: 1.0,
            abstol: 1e-6,
            reltol: 1e-3,
            maxiters: 100_000,
            adaptive: true,
        }
    }

    /// Fast options: speed over accuracy (~10x faster).
    pub fn fast() -> Self {
        Self {
            dt: 0.1,
            dtmin: 1e-4,
            dtmax: 1.0,
            abstol: 1e-2,
            reltol: 1e-2,
            maxiters: 1_000,
            adaptive: true,
        }
    }

    /// Accurate options: high precision.
    pub fn accurate() -> Self {
        Self {
            dt: 0.001,
            dtmin: 1e-8,
            dtmax: 0.1,
            abstol: 1e-9,
            reltol: 1e-6,
            maxiters: 1_000_000,
            adaptive: true,
        }
    }

    /// Options for stiff ODE systems.
    pub fn stiff() -> Self {
        Self {
            dt: 0.001,
            dtmin: 1e-10,
            dtmax: 0.01,
            abstol: 1e-8,
            reltol: 1e-5,
            maxiters: 500_000,
            adaptive: true,
        }
    }

    /// Game AI options: fast move evaluation.
    pub fn game_ai() -> Self {
        Self {
            dt: 0.1,
            dtmin: 1e-3,
            dtmax: 1.0,
            abstol: 1e-2,
            reltol: 1e-2,
            maxiters: 500,
            adaptive: true,
        }
    }

    /// Epidemic/population modeling options.
    pub fn epidemic() -> Self {
        Self {
            dt: 0.01,
            dtmin: 1e-6,
            dtmax: 0.5,
            abstol: 1e-6,
            reltol: 1e-4,
            maxiters: 200_000,
            adaptive: true,
        }
    }

    /// Workflow/process simulation options.
    pub fn workflow() -> Self {
        Self {
            dt: 0.1,
            dtmin: 1e-4,
            dtmax: 10.0,
            abstol: 1e-4,
            reltol: 1e-3,
            maxiters: 50_000,
            adaptive: true,
        }
    }

    /// Long-run simulation options.
    pub fn long_run() -> Self {
        Self {
            dt: 0.1,
            dtmin: 1e-4,
            dtmax: 10.0,
            abstol: 1e-5,
            reltol: 1e-3,
            maxiters: 500_000,
            adaptive: true,
        }
    }
}

/// Copies a state map.
pub fn copy_state(s: &State) -> State {
    s.clone()
}

/// Converts a dense vector back to a labeled State map.
fn vec_to_state(v: &[f64], labels: &[String]) -> State {
    labels
        .iter()
        .enumerate()
        .map(|(i, label)| (label.clone(), v[i]))
        .collect()
}

/// Integrates a raw vectorized ODE `du/dt = f(t, u)` from `u0` over `tspan` using the
/// given solver and options. This is the direction-agnostic core shared by [`solve`] and
/// by any caller (e.g. `pflow-learn`) that needs to integrate an augmented vector (state
/// plus extra columns) through the same adaptive Runge-Kutta loop.
///
/// `tspan` must have `tspan[1] > tspan[0]` — the loop runs `while tcur < tf`, driven by a
/// `dtcur` that is only ever clamped within `[opts.dtmin, opts.dtmax]` (always positive).
/// A reversed span executes zero steps and silently returns the initial condition. This
/// function is, and stays, forward-only by construction; callers that need a "backward"
/// integration (e.g. an adjoint costate solve) must reformulate their own IVP as a forward
/// integration (the standard time-reversal substitution) rather than expecting this loop
/// to grow a direction flag.
///
/// Returns `(t_out, u_out)`: the accepted time points and the dense state vector at each.
pub fn integrate_vec(
    f: &dyn Fn(f64, &[f64]) -> Vec<f64>,
    u0: &[f64],
    tspan: [f64; 2],
    solver: &Solver,
    opts: &Options,
) -> (Vec<f64>, Vec<Vec<f64>>) {
    let dt = opts.dt;
    let dtmin = opts.dtmin;
    let dtmax = opts.dtmax;
    let abstol = opts.abstol;
    let reltol = opts.reltol;
    let maxiters = opts.maxiters;
    let adaptive = opts.adaptive;

    let t0 = tspan[0];
    let tf = tspan[1];
    let n = u0.len();

    let mut t_out = vec![t0];
    let mut u_out: Vec<Vec<f64>> = vec![u0.to_vec()];
    let mut tcur = t0;
    let mut ucur = u0.to_vec();
    let mut dtcur = dt;
    let mut nsteps = 0usize;

    while tcur < tf && nsteps < maxiters {
        // Don't overshoot
        if tcur + dtcur > tf {
            dtcur = tf - tcur;
        }

        // Compute Runge-Kutta stages
        let num_stages = solver.c.len();
        let mut k: Vec<Vec<f64>> = Vec::with_capacity(num_stages);
        k.push(f(tcur, &ucur));

        for stage in 1..num_stages {
            let tstage = tcur + solver.c[stage] * dtcur;
            let mut ustage = ucur.clone();
            for j in 0..stage {
                let aj = if stage < solver.a.len() && j < solver.a[stage].len() {
                    solver.a[stage][j]
                } else {
                    0.0
                };
                if aj != 0.0 {
                    let scale = dtcur * aj;
                    for i in 0..n {
                        ustage[i] += scale * k[j][i];
                    }
                }
            }
            k.push(f(tstage, &ustage));
        }

        // Compute solution at next step
        let mut unext = ucur.clone();
        for j in 0..solver.b.len() {
            if solver.b[j] != 0.0 {
                let scale = dtcur * solver.b[j];
                for i in 0..n {
                    unext[i] += scale * k[j][i];
                }
            }
        }

        // Compute error estimate
        let mut err = 0.0;
        if adaptive {
            for i in 0..n {
                let mut errest = 0.0;
                for j in 0..solver.b_hat.len() {
                    errest += dtcur * solver.b_hat[j] * k[j][i];
                }
                let uc = ucur[i];
                let un = unext[i];
                let mut scale = abstol + reltol * uc.abs().max(un.abs());
                if scale == 0.0 {
                    scale = abstol;
                }
                let val = errest.abs() / scale;
                if val > err {
                    err = val;
                }
            }
        }

        // Accept or reject step
        if !adaptive || err <= 1.0 || dtcur <= dtmin {
            tcur += dtcur;
            ucur = unext;
            t_out.push(tcur);
            u_out.push(ucur.clone());
            nsteps += 1;

            if adaptive && err > 0.0 {
                let factor = 0.9 * (1.0 / err).powf(1.0 / (solver.order as f64 + 1.0));
                let factor = factor.min(5.0);
                dtcur = dtmax.min(dtmin.max(dtcur * factor));
            }
        } else {
            let factor = 0.9 * (1.0 / err).powf(1.0 / (solver.order as f64 + 1.0));
            let factor = factor.max(0.1);
            dtcur = dtmin.max(dtcur * factor);
        }
    }

    (t_out, u_out)
}

/// Integrates the ODE problem using the given solver and options.
///
/// Internally uses vectorized (dense array) state representation for performance.
pub fn solve(prob: &Problem, solver: &Solver, opts: &Options) -> Solution {
    let (t_out, u_out) = integrate_vec(&*prob.vec_f, &prob.vec_u0, prob.tspan, solver, opts);

    // Convert dense trajectory to State maps for backward compatibility
    let state_u: Vec<State> = u_out
        .iter()
        .map(|v| vec_to_state(v, &prob.state_labels))
        .collect();

    Solution {
        t: t_out,
        u: state_u,
        state_labels: prob.state_labels.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods;

    #[test]
    fn test_simple_decay() {
        // A -> t1 -> B, should transfer tokens from A to B
        let net = PetriNet::build()
            .place("A", 10.0)
            .place("B", 0.0)
            .transition("t1")
            .arc("A", "t1", 1.0)
            .arc("t1", "B", 1.0)
            .done();

        let state = net.set_state(None);
        let rates = net.set_rates(None);
        let prob = Problem::new(net, state, [0.0, 10.0], rates).unwrap();
        let sol = solve(&prob, &methods::tsit5(), &Options::default_opts());

        let final_state = sol.get_final_state().unwrap();
        let total = final_state["A"] + final_state["B"];
        // Conservation: A + B should be approximately 10
        assert!((total - 10.0).abs() < 0.1);
    }

    /// With a 5th-order error estimate, tightening reltol by a factor of 100
    /// should cost roughly 100^(1/5) ~ 2.5x the steps. The old +1/66 weight
    /// made the estimate first order, so the same tightening cost ~100x.
    #[test]
    fn tsit5_step_count_scales_as_fifth_root_of_reltol() {
        let build = || {
            PetriNet::build()
                .place("S", 990.0)
                .place("I", 10.0)
                .place("R", 0.0)
                .transition("infect")
                .transition("recover")
                .arc("S", "infect", 1.0)
                .arc("I", "infect", 1.0)
                .arc("infect", "I", 2.0)
                .arc("I", "recover", 1.0)
                .arc("recover", "R", 1.0)
                .done()
        };
        let steps = |reltol: f64| {
            let net = build();
            let state = net.set_state(None);
            let mut rates = net.set_rates(None);
            rates.insert("infect".into(), 0.0005);
            rates.insert("recover".into(), 0.1);
            let prob = Problem::new(net, state, [0.0, 100.0], rates).unwrap();
            let opts = Options {
                dt: 0.01,
                dtmin: 1e-10,
                dtmax: 100.0,
                abstol: 1e-12,
                reltol,
                maxiters: 1_000_000,
                adaptive: true,
            };
            solve(&prob, &methods::tsit5(), &opts).t.len() - 1
        };
        let coarse = steps(1e-3);
        let fine = steps(1e-5);
        let ratio = fine as f64 / coarse as f64;
        assert!(
            (1.5..8.0).contains(&ratio),
            "steps at 1e-3: {coarse}, at 1e-5: {fine}, ratio {ratio} (expected ~2.5)"
        );
    }

    /// `a -> t -> b` with an inhibitor from `i`: A=10, B=0, I=5.
    fn inhibited() -> PetriNet {
        PetriNet::build()
            .place("A", 10.0)
            .place("B", 0.0)
            .place("I", 5.0)
            .transition("t")
            .arc("A", "t", 1.0)
            .arc("t", "B", 1.0)
            .inhibitor_arc("I", "t", 1.0)
            .done()
    }

    /// `a -> t -> b` reading `R`: A=10, B=0, R=1.
    fn reading() -> PetriNet {
        PetriNet::build()
            .place("A", 10.0)
            .place("B", 0.0)
            .place("R", 1.0)
            .transition("t")
            .arc("A", "t", 1.0)
            .arc("t", "B", 1.0)
            .read_arc("t", "R", 1.0)
            .done()
    }

    fn problem(net: PetriNet, c: ContextualArcs) -> Result<Problem, ContextualArcError> {
        let state = net.set_state(None);
        let rates = net.set_rates(None);
        Problem::with_contextual(net, state, [0.0, 10.0], rates, c)
    }

    fn final_state(net: PetriNet, c: ContextualArcs) -> State {
        let prob = problem(net, c).unwrap();
        let sol = solve(&prob, &crate::methods::tsit5(), &Options::default_opts());
        sol.get_final_state().unwrap().clone()
    }

    #[test]
    fn inhibitor_arc_is_refused_by_default() {
        let net = inhibited();
        let state = net.set_state(None);
        let rates = net.set_rates(None);
        let Err(e) = Problem::new(net, state, [0.0, 10.0], rates) else {
            panic!("an inhibitor arc was integrated");
        };
        assert_eq!(e.reads, vec![]);
        assert_eq!(
            e.inhibitors,
            vec![ContextualArc { place: "I".into(), transition: "t".into() }]
        );
        assert_eq!(
            e.to_string(),
            "1 inhibitor arc(s) block a firing above a threshold; a continuous solver cannot \
             test them (I -o t); use selfloop (read arcs only) or drop to integrate the net anyway"
        );
    }

    #[test]
    fn read_arc_is_refused_by_default() {
        let net = reading();
        let state = net.set_state(None);
        let rates = net.set_rates(None);
        let Err(e) = Problem::new(net, state, [0.0, 10.0], rates) else {
            panic!("a read arc was integrated");
        };
        assert_eq!(
            e.reads,
            vec![ContextualArc { place: "R".into(), transition: "t".into() }]
        );
        assert_eq!(
            e.to_string(),
            "1 read arc(s) gate a firing without consuming; a continuous solver cannot test \
             them (t -o R); use selfloop (read arcs only) or drop to integrate the net anyway"
        );
    }

    /// The bugs this refusal replaces: as flow arcs, the inhibitor drained I
    /// to about 0 and the read arc grew R to about 10.9. Both opt-outs must
    /// leave the contextual place where it started.
    #[test]
    fn drop_ignores_the_arc() {
        let u = final_state(inhibited(), ContextualArcs::Drop);
        assert_eq!(u["I"], 5.0);
        assert!(u["B"] > 9.99 && (u["A"] + u["B"] - 10.0).abs() < 1e-9, "{u:?}");

        let u = final_state(reading(), ContextualArcs::Drop);
        assert_eq!(u["R"], 1.0);
        assert!(u["B"] > 9.99 && (u["A"] + u["B"] - 10.0).abs() < 1e-9, "{u:?}");
    }

    #[test]
    fn selfloop_reads_without_moving_tokens() {
        let u = final_state(reading(), ContextualArcs::SelfLoop);
        assert!((u["R"] - 1.0).abs() < 1e-12, "{u:?}");
        // R=1 is first-order decay of A, and R scales the flux: at R=2 it is
        // twice as fast.
        assert!((u["A"] - 10.0 * (-10.0f64).exp()).abs() < 1e-3, "{u:?}");
        let doubled = PetriNet::build()
            .place("A", 10.0)
            .place("B", 0.0)
            .place("R", 2.0)
            .transition("t")
            .arc("A", "t", 1.0)
            .arc("t", "B", 1.0)
            .read_arc("t", "R", 2.0)
            .done();
        let prob = problem(doubled, ContextualArcs::SelfLoop).unwrap();
        // The weight is the arc's own on both sides of the loop.
        assert_eq!(prob.net.arcs.iter().filter(|a| a.weight == [2.0]).count(), 2);
        assert!(prob.net.arcs.iter().all(|a| !a.inhibit_transition));
        let sol = solve(&prob, &crate::methods::tsit5(), &Options::default_opts());
        let u = sol.get_final_state().unwrap();
        assert!((u["R"] - 2.0).abs() < 1e-12, "{u:?}");
        assert!((u["A"] - 10.0 * (-20.0f64).exp()).abs() < 1e-3, "{u:?}");
    }

    #[test]
    fn selfloop_still_refuses_an_inhibitor() {
        let Err(e) = problem(inhibited(), ContextualArcs::SelfLoop) else {
            panic!("an inhibitor became a self-loop");
        };
        assert!(e.to_string().ends_with(
            "inhibitor arcs cannot be encoded as self-loops; use drop to ignore them"
        ));
    }

    /// The closure and the vectorized path are built separately, so both
    /// must see the resolved net.
    #[test]
    fn both_derivatives_see_the_resolved_net() {
        let prob = problem(inhibited(), ContextualArcs::Drop).unwrap();
        let du = (prob.f)(0.0, &prob.u0);
        assert_eq!(du["I"], 0.0);
        assert_eq!(du["A"], -10.0);
        let dv = (prob.vec_f)(0.0, &prob.vec_u0);
        assert_eq!(dv[prob.state_index["I"]], 0.0);
        assert_eq!(dv[prob.state_index["A"]], -10.0);
    }

    #[test]
    fn a_net_with_no_contextual_arc_is_untouched_by_any_policy() {
        for c in [ContextualArcs::Refuse, ContextualArcs::SelfLoop, ContextualArcs::Drop] {
            let (net, _) = PetriNet::build().sir(999.0, 1.0, 0.0).with_rates(1.0);
            assert_eq!(problem(net, c).unwrap().net.arcs.len(), 5);
        }
    }

    #[test]
    fn contextual_policy_parses_from_its_flag_spelling() {
        assert_eq!("refuse".parse(), Ok(ContextualArcs::Refuse));
        assert_eq!("selfloop".parse(), Ok(ContextualArcs::SelfLoop));
        assert_eq!("drop".parse(), Ok(ContextualArcs::Drop));
        assert!("ignore".parse::<ContextualArcs>().is_err());
    }
}
