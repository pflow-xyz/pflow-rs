//! Evaluates hypothetical states via ODE simulation — the core pattern for
//! game AI, move evaluation, sensitivity analysis and what-if scenarios —
//! ported from go-pflow's `hypothesis` package. This is the library go-pflow
//! itself names as the shape petri-pilot's `petri_scenario`/`sim.Compare`
//! answer a what-if with (ROADMAP.md Phase 4): override a marking, run
//! forward, score the outcome.
//!
//! go-pflow's `EvaluateManyParallel`/`FindBestParallel` variants exist only
//! to spread the same computation across goroutines; see `sensitivity`'s
//! module doc for why the sequential methods here are the whole of the
//! parity surface.

use std::collections::HashMap;

use pflow_core::net::PetriNet;
use pflow_core::stateutil;
use pflow_core::State;
use pflow_solver::methods::{tsit5, Solver};
use pflow_solver::ode::{solve, Options, Problem};

/// Evaluates a final state and returns a score. Higher scores are
/// considered better.
pub type Scorer = Box<dyn Fn(&State) -> f64>;

/// Checks whether evaluation should stop early. Returns `true` if the state
/// is infeasible or evaluation should be skipped.
pub type EarlyTerminator = Box<dyn Fn(&State) -> bool>;

/// Evaluates hypothetical states by running ODE simulations.
pub struct Evaluator {
    net: PetriNet,
    rates: HashMap<String, f64>,
    tspan: [f64; 2],
    opts: Options,
    solver: Solver,
    scorer: Scorer,
    early_terminator: Option<EarlyTerminator>,
    infeasible_score: f64,
}

impl Evaluator {
    /// Creates a new hypothesis evaluator.
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use pflow_core::Builder;
    /// use pflow_results::hypothesis::Evaluator;
    ///
    /// let net = Builder::new()
    ///     .place("pos5", 1.0)
    ///     .transition("advance")
    ///     .arc("pos5", "advance", 1.0)
    ///     .done();
    /// let rates: HashMap<String, f64> = HashMap::from([("advance".to_string(), 1.0)]);
    /// let eval = Evaluator::new(net, rates, Box::new(|final_state| {
    ///     final_state.get("pos5").copied().unwrap_or(0.0)
    /// }));
    /// let _ = eval;
    /// ```
    pub fn new(net: PetriNet, rates: HashMap<String, f64>, scorer: Scorer) -> Self {
        Self {
            net,
            rates,
            tspan: [0.0, 5.0],
            opts: Options::fast(),
            solver: tsit5(),
            scorer,
            early_terminator: None,
            infeasible_score: f64::NEG_INFINITY,
        }
    }

    /// Sets custom solver options.
    pub fn with_options(mut self, opts: Options) -> Self {
        self.opts = opts;
        self
    }

    /// Sets the simulation time span.
    pub fn with_time_span(mut self, t0: f64, tf: f64) -> Self {
        self.tspan = [t0, tf];
        self
    }

    /// Sets a function to check for infeasible states. If it returns
    /// `true`, evaluation is skipped and `infeasible_score` is returned.
    pub fn with_early_termination(mut self, terminator: EarlyTerminator) -> Self {
        self.early_terminator = Some(terminator);
        self
    }

    /// Sets the score returned for infeasible states. Default is negative
    /// infinity.
    pub fn with_infeasible_score(mut self, score: f64) -> Self {
        self.infeasible_score = score;
        self
    }

    /// Runs a simulation with `updates` applied to `base` and returns the
    /// score.
    pub fn evaluate(&self, base: &State, updates: &State) -> f64 {
        let hyp_state = stateutil::apply(base, updates);
        self.evaluate_state(&hyp_state)
    }

    /// Runs a simulation with the given state and returns the score.
    pub fn evaluate_state(&self, state: &State) -> f64 {
        if let Some(terminator) = &self.early_terminator {
            if terminator(state) {
                return self.infeasible_score;
            }
        }

        let prob = Problem::new(
            self.net.clone(),
            state.clone(),
            self.tspan,
            self.rates.clone(),
        );
        let sol = solve(&prob, &self.solver, &self.opts);
        let final_state = sol.get_final_state().cloned().unwrap_or_default();
        (self.scorer)(&final_state)
    }

    /// Evaluates multiple state-update sets and returns all results.
    pub fn evaluate_many(&self, base: &State, updates: &[State]) -> Vec<EvalResult> {
        updates
            .iter()
            .enumerate()
            .map(|(i, u)| {
                let hyp_state = stateutil::apply(base, u);
                let score = self.evaluate_state(&hyp_state);
                EvalResult {
                    index: i,
                    score,
                    state: hyp_state,
                }
            })
            .collect()
    }

    /// Evaluates every candidate and returns the index and score of the
    /// best. Returns `(-1, -Inf)` if no candidates are provided.
    pub fn find_best(&self, base: &State, updates: &[State]) -> (i64, f64) {
        if updates.is_empty() {
            return (-1, f64::NEG_INFINITY);
        }

        let mut best_index = -1_i64;
        let mut best_score = f64::NEG_INFINITY;

        for (i, u) in updates.iter().enumerate() {
            let score = self.evaluate(base, u);
            if score > best_score {
                best_index = i as i64;
                best_score = score;
            }
        }

        (best_index, best_score)
    }

    /// Evaluates two states and returns which is better: `1` if `state_a`
    /// is better, `-1` if `state_b` is better, `0` if equal.
    pub fn compare(&self, state_a: &State, state_b: &State) -> i32 {
        let score_a = self.evaluate_state(state_a);
        let score_b = self.evaluate_state(state_b);

        if score_a > score_b {
            1
        } else if score_b > score_a {
            -1
        } else {
            0
        }
    }

    /// Evaluates the impact of disabling each transition. Returns a map
    /// from transition name to the score when that transition is disabled,
    /// plus `"_baseline"` for the unmodified score.
    ///
    /// go-pflow mutates `e.rates` in place and restores it afterward
    /// (`hypothesis.go`'s `SensitivityAnalysis`); this port takes `&mut
    /// self` for the same reason rather than cloning the rate map on every
    /// transition, and restores every rate before returning.
    pub fn sensitivity_analysis(&mut self, state: &State) -> HashMap<String, f64> {
        let mut results = HashMap::new();

        let base_score = self.evaluate_state(state);
        results.insert("_baseline".to_string(), base_score);

        let transitions: Vec<String> = self.net.transitions.keys().cloned().collect();
        for trans in transitions {
            let orig_rate = self.rates.get(&trans).copied().unwrap_or(0.0);

            self.rates.insert(trans.clone(), 0.0);
            let score = self.evaluate_state(state);
            results.insert(trans.clone(), score);

            self.rates.insert(trans, orig_rate);
        }

        results
    }

    /// Returns the impact of each transition relative to baseline.
    /// Positive values mean disabling the transition improves the score;
    /// negative values mean disabling it worsens the score.
    pub fn sensitivity_impact(&mut self, state: &State) -> HashMap<String, f64> {
        let raw = self.sensitivity_analysis(state);
        let baseline = raw.get("_baseline").copied().unwrap_or(0.0);

        raw.into_iter()
            .filter(|(trans, _)| trans != "_baseline")
            .map(|(trans, score)| (trans, score - baseline))
            .collect()
    }
}

/// The result of evaluating a single candidate.
#[derive(Debug, Clone)]
pub struct EvalResult {
    pub index: usize,
    pub score: f64,
    pub state: State,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::Builder;

    /// A -> convert -> B, matching go-pflow's `hypothesis_test.go`
    /// `createSimpleNet`.
    fn simple_net() -> (PetriNet, HashMap<String, f64>) {
        let net = Builder::new()
            .place("A", 10.0)
            .place("B", 0.0)
            .transition("convert")
            .arc("A", "convert", 1.0)
            .arc("convert", "B", 1.0)
            .done();
        (net, HashMap::from([("convert".to_string(), 1.0)]))
    }

    /// tokens -> {to_win -> win, to_lose -> lose}, matching go-pflow's
    /// `createGameNet`.
    fn game_net() -> (PetriNet, HashMap<String, f64>) {
        let net = Builder::new()
            .place("tokens", 10.0)
            .place("win", 0.0)
            .place("lose", 0.0)
            .transition("to_win")
            .transition("to_lose")
            .arc("tokens", "to_win", 1.0)
            .arc("to_win", "win", 1.0)
            .arc("tokens", "to_lose", 1.0)
            .arc("to_lose", "lose", 1.0)
            .done();
        (
            net,
            HashMap::from([("to_win".to_string(), 0.6), ("to_lose".to_string(), 0.4)]),
        )
    }

    fn scorer_b() -> Scorer {
        Box::new(|final_state: &State| final_state.get("B").copied().unwrap_or(0.0))
    }

    #[test]
    fn evaluate_scores_a_state_update() {
        let (net, rates) = simple_net();
        let eval = Evaluator::new(net, rates, scorer_b())
            .with_time_span(0.0, 10.0)
            .with_options(Options::default_opts());

        let base = HashMap::from([("A".to_string(), 10.0), ("B".to_string(), 0.0)]);
        let score = eval.evaluate(&base, &HashMap::from([("A".to_string(), 20.0)]));

        assert!(score > 0.0);
    }

    #[test]
    fn evaluate_state_scores_a_state_directly() {
        let (net, rates) = simple_net();
        let eval = Evaluator::new(net, rates, scorer_b()).with_time_span(0.0, 10.0);

        let state = HashMap::from([("A".to_string(), 10.0), ("B".to_string(), 0.0)]);
        assert!(eval.evaluate_state(&state) > 0.0);
    }

    #[test]
    fn early_termination_short_circuits_to_the_infeasible_score() {
        let (net, rates) = simple_net();
        let eval = Evaluator::new(net, rates, scorer_b())
            .with_early_termination(Box::new(|state: &State| {
                state.get("A").copied().unwrap_or(0.0) < 0.0
            }))
            .with_infeasible_score(-999.0);

        let feasible = eval.evaluate_state(&HashMap::from([
            ("A".to_string(), 10.0),
            ("B".to_string(), 0.0),
        ]));
        assert_ne!(feasible, -999.0);

        let infeasible = eval.evaluate_state(&HashMap::from([
            ("A".to_string(), -1.0),
            ("B".to_string(), 0.0),
        ]));
        assert_eq!(infeasible, -999.0);
    }

    #[test]
    fn evaluate_many_scores_every_update() {
        let (net, rates) = simple_net();
        let eval = Evaluator::new(net, rates, scorer_b()).with_time_span(0.0, 5.0);

        let base = HashMap::from([("A".to_string(), 10.0), ("B".to_string(), 0.0)]);
        let updates = vec![
            HashMap::from([("A".to_string(), 5.0)]),
            HashMap::from([("A".to_string(), 10.0)]),
            HashMap::from([("A".to_string(), 20.0)]),
        ];

        let results = eval.evaluate_many(&base, &updates);
        assert_eq!(results.len(), 3);
        assert!(results[2].score > results[0].score);
    }

    #[test]
    fn find_best_picks_the_highest_score() {
        let (net, rates) = simple_net();
        let eval = Evaluator::new(net, rates, scorer_b()).with_time_span(0.0, 5.0);

        let base = HashMap::from([("A".to_string(), 10.0), ("B".to_string(), 0.0)]);
        let updates = vec![
            HashMap::from([("A".to_string(), 5.0)]),
            HashMap::from([("A".to_string(), 20.0)]),
            HashMap::from([("A".to_string(), 10.0)]),
        ];

        let (best_idx, best_score) = eval.find_best(&base, &updates);
        assert_eq!(best_idx, 1);
        assert!(best_score > 0.0);

        let (empty_idx, empty_score) = eval.find_best(&base, &[]);
        assert_eq!(empty_idx, -1);
        assert!(empty_score.is_infinite() && empty_score < 0.0);
    }

    #[test]
    fn compare_orders_states_by_score() {
        let (net, rates) = simple_net();
        let eval = Evaluator::new(net, rates, scorer_b()).with_time_span(0.0, 5.0);

        let more = HashMap::from([("A".to_string(), 20.0), ("B".to_string(), 0.0)]);
        let less = HashMap::from([("A".to_string(), 5.0), ("B".to_string(), 0.0)]);

        assert_eq!(eval.compare(&more, &less), 1);
        assert_eq!(eval.compare(&less, &more), -1);
    }

    #[test]
    fn sensitivity_analysis_reports_baseline_and_every_transition() {
        let (net, rates) = game_net();
        let scorer: Scorer = Box::new(|final_state: &State| {
            final_state.get("win").copied().unwrap_or(0.0)
                - final_state.get("lose").copied().unwrap_or(0.0)
        });
        let mut eval = Evaluator::new(net, rates, scorer).with_time_span(0.0, 10.0);

        let state = HashMap::from([
            ("tokens".to_string(), 10.0),
            ("win".to_string(), 0.0),
            ("lose".to_string(), 0.0),
        ]);
        let analysis = eval.sensitivity_analysis(&state);

        assert!(analysis.contains_key("_baseline"));
        assert!(analysis.contains_key("to_win"));
        assert!(analysis.contains_key("to_lose"));

        assert!(analysis["to_win"] < analysis["_baseline"]);
        assert!(analysis["to_lose"] > analysis["_baseline"]);
    }

    #[test]
    fn sensitivity_impact_excludes_the_baseline_key() {
        let (net, rates) = game_net();
        let scorer: Scorer = Box::new(|final_state: &State| {
            final_state.get("win").copied().unwrap_or(0.0)
                - final_state.get("lose").copied().unwrap_or(0.0)
        });
        let mut eval = Evaluator::new(net, rates, scorer).with_time_span(0.0, 10.0);

        let state = HashMap::from([
            ("tokens".to_string(), 10.0),
            ("win".to_string(), 0.0),
            ("lose".to_string(), 0.0),
        ]);
        let impact = eval.sensitivity_impact(&state);

        assert!(!impact.contains_key("_baseline"));
        assert!(impact["to_win"] < 0.0);
        assert!(impact["to_lose"] > 0.0);
    }

    #[test]
    fn sensitivity_analysis_restores_rates_afterward() {
        let (net, rates) = game_net();
        let scorer: Scorer =
            Box::new(|final_state: &State| final_state.get("win").copied().unwrap_or(0.0));
        let mut eval = Evaluator::new(net, rates, scorer).with_time_span(0.0, 10.0);

        let state = HashMap::from([
            ("tokens".to_string(), 10.0),
            ("win".to_string(), 0.0),
            ("lose".to_string(), 0.0),
        ]);
        eval.sensitivity_analysis(&state);

        assert_eq!(eval.rates["to_win"], 0.6);
        assert_eq!(eval.rates["to_lose"], 0.4);
    }
}
