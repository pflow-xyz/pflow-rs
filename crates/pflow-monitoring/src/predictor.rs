//! Ported from go-pflow's `monitoring/predictor.go`.

use std::collections::HashMap;

use pflow_core::net::PetriNet;
use pflow_core::State;
use pflow_solver::{solve, Options};

use crate::types::{Case, NextActivity};

/// Uses simulation to predict case outcomes.
pub struct Predictor {
    /// The model as supplied, deliberately NOT pre-unfolded: `Problem::new`
    /// unfolds it itself internally via mass-action kinetics over the raw
    /// arcs, matching go-pflow's own comment that `solver.NewProblem`
    /// handles unfolding so `Solution`'s base-name reads keep working.
    net: PetriNet,
    /// The color unfolding, used for the discrete enablement checks that
    /// have no solver to do it for them.
    expanded: PetriNet,
    rates: HashMap<String, f64>,
}

/// Results from simulation-based prediction.
#[derive(Debug, Clone, Default)]
pub struct SimulationPrediction {
    pub current_time: f64,
    pub predicted_end_time: f64,
    pub confidence: f64,
    pub enabled_transitions: Vec<String>,
}

impl Predictor {
    /// Creates a prediction engine from a learned model.
    ///
    /// Multi-color nets are supported: the ODE forecast runs per color
    /// (mass-action kinetics over the raw, un-unfolded net) while
    /// completion is still measured as the total token mass reaching
    /// `"end"`, and enablement is decided per color.
    pub fn new(net: PetriNet, rates: HashMap<String, f64>) -> Self {
        let (expanded, _) = net.expand_colors();
        Self {
            net,
            expanded,
            rates,
        }
    }

    /// Runs simulation from `current_state` to predict completion time.
    /// This is the core predictive capability — ODE simulation with
    /// learned rates.
    ///
    /// # Contract
    ///
    /// Assumes the net models a case workflow with a place named `"end"`
    /// where token mass arriving means the case is complete — same
    /// assumption go-pflow's `Predictor` documents. A net using a different
    /// name produces a degenerate prediction (the max-horizon default).
    pub fn predict_from_state(&self, current_state: &State, current_time: f64) -> SimulationPrediction {
        let max_horizon = 86_400.0; // 24 hours in seconds

        let prob = pflow_solver::Problem::new(
            self.net.clone(),
            current_state.clone(),
            [current_time, current_time + max_horizon],
            self.rates.clone(),
        );

        let opts = Options {
            dt: 60.0,
            dtmin: 0.1,
            dtmax: 600.0,
            abstol: 1e-6,
            reltol: 1e-4,
            maxiters: 10_000,
            adaptive: true,
        };
        let solver = pflow_solver::methods::tsit5();
        let sol = solve(&prob, &solver, &opts);

        let end_times = sol.get_variable("end");
        let mut predicted_end_time = current_time + max_horizon;
        let threshold = 0.5;
        for (i, &end_tokens) in end_times.iter().enumerate() {
            if end_tokens >= threshold {
                predicted_end_time = sol.t[i];
                break;
            }
        }

        let final_state = sol.get_final_state();
        let confidence = final_state
            .and_then(|s| s.get("end"))
            .copied()
            .unwrap_or(0.0);

        let enabled_transitions = self.enabled_transitions(current_state);

        SimulationPrediction {
            current_time,
            predicted_end_time,
            confidence,
            enabled_transitions,
        }
    }

    /// Transitions enabled in `state`. On a multi-color model every color
    /// must independently satisfy its own arc weight, so `state` is mapped
    /// into the unfolding first. Already-expanded keys pass through
    /// [`PetriNet::expand_state`] untouched, so this is safe to call either
    /// way.
    pub fn enabled_transitions(&self, state: &State) -> Vec<String> {
        let expanded_state = self.net.expand_state(state);
        let mut enabled = Vec::new();

        for trans_label in self.expanded.transitions.keys() {
            let mut is_enabled = true;
            for arc in &self.expanded.arcs {
                if &arc.target == trans_label && self.expanded.places.contains_key(&arc.source) {
                    let weight = arc.weight_sum();
                    if expanded_state.get(&arc.source).copied().unwrap_or(0.0) < weight {
                        is_enabled = false;
                        break;
                    }
                }
            }
            if is_enabled {
                enabled.push(trans_label.clone());
            }
        }
        enabled
    }
}

/// Predicts time until completion using simulation, integrating learned
/// model dynamics with a case's current state.
pub fn predict_remaining_time(
    case: &Case,
    predictor: &Predictor,
    now_ms: i64,
) -> (i64, f64) {
    let current_state = estimate_current_state(case, &predictor.net);
    let current_time = ((now_ms - case.start_time_ms).max(0)) as f64 / 1000.0;

    let pred = predictor.predict_from_state(&current_state, current_time);
    let remaining_seconds = pred.predicted_end_time - pred.current_time;
    ((remaining_seconds * 1000.0) as i64, pred.confidence)
}

/// Predicts which activity will occur next, from enabled transitions and
/// their firing rates.
///
/// In continuous Petri nets with mass-action kinetics: each transition
/// fires at rate `rate * product(input_place_tokens)`; time until firing is
/// exponentially distributed with that effective rate; probability of
/// firing first is `effective_rate / sum(all_effective_rates)`.
pub fn predict_next_activity(case: &Case, predictor: &Predictor) -> Vec<NextActivity> {
    let current_state = estimate_current_state(case, &predictor.net);
    let enabled_transitions = predictor.enabled_transitions(&current_state);
    if enabled_transitions.is_empty() {
        return Vec::new();
    }

    let mut effective_rates: HashMap<String, f64> = HashMap::new();
    let mut total_rate = 0.0;

    for trans_name in &enabled_transitions {
        let rate = predictor.rates.get(trans_name).copied().unwrap_or(0.0);
        let rate = if rate == 0.0 { 1.0 } else { rate };

        let mut flux = rate;
        for arc in &predictor.net.arcs {
            if &arc.target == trans_name && predictor.net.places.contains_key(&arc.source) {
                let place_tokens = current_state.get(&arc.source).copied().unwrap_or(0.0);
                if place_tokens <= 0.0 {
                    flux = 0.0;
                    break;
                }
                flux *= place_tokens;
            }
        }
        effective_rates.insert(trans_name.clone(), flux);
        total_rate += flux;
    }

    let mut predictions = Vec::with_capacity(enabled_transitions.len());
    for trans_name in &enabled_transitions {
        let eff_rate = effective_rates.get(trans_name).copied().unwrap_or(0.0);
        if eff_rate <= 0.0 {
            continue;
        }
        let probability = eff_rate / total_rate;
        let expected_time_ms = ((1.0 / eff_rate) * 1000.0) as i64;
        predictions.push(NextActivity {
            activity: trans_name.clone(),
            probability,
            expected_time_ms,
        });
    }
    predictions
}

/// Replays a case's event history through `net` to estimate its current
/// marking.
///
/// # Contract
///
/// Assumes a place named `"start"` (where each case's token begins) and a
/// place named `"end"` (where token mass arriving means the case is
/// complete). Nets using other names produce degenerate estimates — the
/// replay starts from an all-zero marking plus one token in `"start"`.
pub fn estimate_current_state(case: &Case, net: &PetriNet) -> State {
    let mut state: State = net.places.keys().map(|p| (p.clone(), 0.0)).collect();
    state.insert("start".to_string(), 1.0);

    // Replay per color, the same rule `Problem::new` uses: the one starting
    // token has no color of its own, so `expand_state` splits it in the
    // proportions "start" declares.
    let raw = net;
    let (expanded, _) = net.expand_colors();
    let mut state = raw.expand_state(&state);

    for event in &case.history {
        let activity_name = &event.activity;
        if !expanded.transitions.contains_key(activity_name) {
            // Activity not in the model — skip (could be noise).
            continue;
        }

        // Fire the transition: consume tokens from input places, produce
        // in output places. go-pflow force-fires even when not enabled
        // (best-effort state estimation over noisy/concurrent logs) — this
        // is a faithful port of that, not a stricter replacement.
        for arc in &expanded.arcs {
            let weight = arc.weight_sum();
            if &arc.target == activity_name {
                if let Some(v) = state.get_mut(&arc.source) {
                    if expanded.places.contains_key(&arc.source) {
                        *v -= weight;
                        if *v < 0.0 {
                            *v = 0.0;
                        }
                    }
                }
            } else if &arc.source == activity_name && expanded.places.contains_key(&arc.target) {
                *state.entry(arc.target.clone()).or_insert(0.0) += weight;
            }
        }
    }

    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Event;

    fn workflow_net() -> PetriNet {
        PetriNet::build()
            .place("start", 0.0)
            .place("middle", 0.0)
            .place("end", 0.0)
            .transition("t1")
            .transition("t2")
            .arc("start", "t1", 1.0)
            .arc("t1", "middle", 1.0)
            .arc("middle", "t2", 1.0)
            .arc("t2", "end", 1.0)
            .done()
    }

    fn case_at(activities: &[&str]) -> Case {
        Case {
            id: "c1".into(),
            start_time_ms: 0,
            history: activities
                .iter()
                .map(|a| Event {
                    case_id: "c1".into(),
                    activity: a.to_string(),
                    timestamp_ms: 0,
                    resource: String::new(),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn estimate_current_state_starts_with_one_token_in_start() {
        let net = workflow_net();
        let case = case_at(&[]);
        let state = estimate_current_state(&case, &net);
        assert_eq!(state.get("start"), Some(&1.0));
        assert_eq!(state.get("middle"), Some(&0.0));
    }

    #[test]
    fn estimate_current_state_replays_history() {
        let net = workflow_net();
        let case = case_at(&["t1"]);
        let state = estimate_current_state(&case, &net);
        assert_eq!(state.get("start"), Some(&0.0));
        assert_eq!(state.get("middle"), Some(&1.0));
    }

    #[test]
    fn estimate_current_state_skips_unknown_activities() {
        let net = workflow_net();
        let case = case_at(&["not-a-transition"]);
        let state = estimate_current_state(&case, &net);
        assert_eq!(state.get("start"), Some(&1.0), "unknown activity must not change state");
    }

    #[test]
    fn enabled_transitions_reflects_the_marking() {
        let net = workflow_net();
        let mut rates = HashMap::new();
        rates.insert("t1".to_string(), 1.0);
        rates.insert("t2".to_string(), 1.0);
        let predictor = Predictor::new(net, rates);

        let case = case_at(&[]);
        let state = estimate_current_state(&case, &predictor.net);
        assert_eq!(predictor.enabled_transitions(&state), vec!["t1".to_string()]);
    }

    #[test]
    fn predict_next_activity_returns_the_only_enabled_transition() {
        let net = workflow_net();
        let mut rates = HashMap::new();
        rates.insert("t1".to_string(), 2.0);
        rates.insert("t2".to_string(), 1.0);
        let predictor = Predictor::new(net, rates);

        let case = case_at(&[]);
        let next = predict_next_activity(&case, &predictor);
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].activity, "t1");
        assert!((next[0].probability - 1.0).abs() < 1e-9);
    }

    #[test]
    fn predict_next_activity_is_empty_with_nothing_enabled() {
        let net = workflow_net();
        let predictor = Predictor::new(net, HashMap::new());
        let case = case_at(&["t1", "t2"]); // token now sits in "end"
        let next = predict_next_activity(&case, &predictor);
        assert!(next.is_empty());
    }
}
