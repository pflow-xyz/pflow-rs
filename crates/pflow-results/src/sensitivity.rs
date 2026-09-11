//! Tools for analyzing how a Petri net's behavior changes with different
//! parameters — rate sensitivity, parameter sweeps, gradient estimation —
//! ported from go-pflow's `sensitivity` package.
//!
//! go-pflow's `AnalyzeRatesParallel`/`AllGradientsParallel` variants exist
//! only to run the same computation across goroutines; `pflow-solver` (this
//! workspace's dependency-free convention) pulls in no threading crate, and
//! every sequential method here produces the identical numbers those would.
//! Parallelising the loops below is a pure performance change, not a parity
//! gap, and is left for whichever caller needs the throughput.

use std::collections::HashMap;

use pflow_core::net::PetriNet;
use pflow_core::State;
use pflow_solver::methods::{tsit5, Solver};
use pflow_solver::ode::{solve, Options, Problem, Solution};

/// Evaluates a simulation result and returns a score.
pub type Scorer = Box<dyn Fn(&Solution) -> f64>;

/// A [`Scorer`] that evaluates the final state.
pub fn final_state_scorer(f: impl Fn(&State) -> f64 + 'static) -> Scorer {
    Box::new(move |sol: &Solution| {
        let final_state = sol.get_final_state().cloned().unwrap_or_default();
        f(&final_state)
    })
}

/// A [`Scorer`] that returns the final value of a specific place.
pub fn place_scorer(place: impl Into<String>) -> Scorer {
    let place = place.into();
    Box::new(move |sol: &Solution| {
        sol.get_final_state()
            .and_then(|s| s.get(&place))
            .copied()
            .unwrap_or(0.0)
    })
}

/// A [`Scorer`] that returns the difference between two places' final
/// values.
pub fn diff_scorer(place_a: impl Into<String>, place_b: impl Into<String>) -> Scorer {
    let place_a = place_a.into();
    let place_b = place_b.into();
    Box::new(move |sol: &Solution| {
        let final_state = sol.get_final_state();
        let a = final_state
            .and_then(|s| s.get(&place_a))
            .copied()
            .unwrap_or(0.0);
        let b = final_state
            .and_then(|s| s.get(&place_b))
            .copied()
            .unwrap_or(0.0);
        a - b
    })
}

/// The result of a sensitivity analysis.
#[derive(Debug, Clone)]
pub struct SensitivityResult {
    /// Score with the original parameters.
    pub baseline: f64,
    /// Score when each parameter is modified.
    pub scores: HashMap<String, f64>,
    /// Change from baseline (`score - baseline`).
    pub impact: HashMap<String, f64>,
    /// Parameters sorted by absolute impact, descending.
    pub ranking: Vec<RankedParam>,
}

/// A parameter and its impact.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedParam {
    pub name: String,
    pub impact: f64,
}

/// Performs sensitivity analysis on a Petri net.
pub struct Analyzer {
    net: PetriNet,
    state: State,
    rates: HashMap<String, f64>,
    tspan: [f64; 2],
    opts: Options,
    solver: Solver,
    scorer: Scorer,
}

impl Analyzer {
    /// Creates a new sensitivity analyzer.
    pub fn new(net: PetriNet, state: State, rates: HashMap<String, f64>, scorer: Scorer) -> Self {
        Self {
            net,
            state,
            rates,
            tspan: [0.0, 10.0],
            opts: Options::default_opts(),
            solver: tsit5(),
            scorer,
        }
    }

    /// Sets the simulation time span.
    pub fn with_time_span(mut self, t0: f64, tf: f64) -> Self {
        self.tspan = [t0, tf];
        self
    }

    /// Sets the solver options.
    pub fn with_options(mut self, opts: Options) -> Self {
        self.opts = opts;
        self
    }

    /// Runs a simulation and returns the score.
    fn simulate(&self, rates: &HashMap<String, f64>) -> f64 {
        let prob = Problem::new(
            self.net.clone(),
            self.state.clone(),
            self.tspan,
            rates.clone(),
        );
        let sol = solve(&prob, &self.solver, &self.opts);
        (self.scorer)(&sol)
    }

    /// Tests the impact of disabling each transition (rate = 0).
    pub fn analyze_rates(&self) -> SensitivityResult {
        let mut scores = HashMap::new();
        let mut impact = HashMap::new();

        let baseline = self.simulate(&self.rates);

        for trans in self.net.transitions.keys() {
            let mut test_rates = self.rates.clone();
            test_rates.insert(trans.clone(), 0.0);

            let score = self.simulate(&test_rates);
            scores.insert(trans.clone(), score);
            impact.insert(trans.clone(), score - baseline);
        }

        let ranking = rank_by_impact(&impact);

        SensitivityResult {
            baseline,
            scores,
            impact,
            ranking,
        }
    }

    /// Tests a range of values for a single rate parameter.
    pub fn sweep_rate(&self, transition: &str, values: &[f64]) -> SweepResult {
        let mut scores = Vec::with_capacity(values.len());

        let mut best_score = f64::NEG_INFINITY;
        let mut best_value = 0.0;
        let mut worst_score = f64::INFINITY;
        let mut worst_value = 0.0;

        for &val in values {
            let mut test_rates = self.rates.clone();
            test_rates.insert(transition.to_string(), val);

            let score = self.simulate(&test_rates);
            scores.push(score);

            if score > best_score {
                best_score = score;
                best_value = val;
            }
            if score < worst_score {
                worst_score = score;
                worst_value = val;
            }
        }

        SweepResult {
            parameter: transition.to_string(),
            values: values.to_vec(),
            scores,
            best: SweepPoint {
                value: best_value,
                score: best_score,
            },
            worst: SweepPoint {
                value: worst_value,
                score: worst_score,
            },
        }
    }

    /// Tests evenly spaced values in a range.
    pub fn sweep_rate_range(
        &self,
        transition: &str,
        min: f64,
        max: f64,
        steps: usize,
    ) -> SweepResult {
        let values = linspace(min, max, steps);
        self.sweep_rate(transition, &values)
    }

    /// Estimates the gradient of the score with respect to a rate
    /// parameter, via central difference: `(f(x+h) - f(x-h)) / (2h)`.
    pub fn gradient(&self, transition: &str, h: f64) -> f64 {
        let orig_rate = self.rates.get(transition).copied().unwrap_or(0.0);
        let h = if h == 0.0 {
            let h = 0.01 * orig_rate;
            if h == 0.0 {
                0.01
            } else {
                h
            }
        } else {
            h
        };

        let mut test_rates_plus = self.rates.clone();
        test_rates_plus.insert(transition.to_string(), orig_rate + h);
        let score_plus = self.simulate(&test_rates_plus);

        let mut test_rates_minus = self.rates.clone();
        let mut minus = orig_rate - h;
        if minus < 0.0 {
            minus = 0.0;
        }
        test_rates_minus.insert(transition.to_string(), minus);
        let score_minus = self.simulate(&test_rates_minus);

        (score_plus - score_minus) / (2.0 * h)
    }

    /// Computes gradients for every rate parameter.
    pub fn all_gradients(&self, h: f64) -> HashMap<String, f64> {
        self.net
            .transitions
            .keys()
            .map(|trans| (trans.clone(), self.gradient(trans, h)))
            .collect()
    }
}

fn rank_by_impact(impact: &HashMap<String, f64>) -> Vec<RankedParam> {
    let mut ranking: Vec<RankedParam> = impact
        .iter()
        .map(|(name, &imp)| RankedParam {
            name: name.clone(),
            impact: imp,
        })
        .collect();
    ranking.sort_by(|a, b| {
        b.impact
            .abs()
            .partial_cmp(&a.impact.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranking
}

/// Results from a parameter sweep.
#[derive(Debug, Clone)]
pub struct SweepResult {
    pub parameter: String,
    pub values: Vec<f64>,
    pub scores: Vec<f64>,
    pub best: SweepPoint,
    pub worst: SweepPoint,
}

/// One (value, score) pair from a [`SweepResult`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepPoint {
    pub value: f64,
    pub score: f64,
}

/// A grid search over multiple parameters.
pub struct GridSearch<'a> {
    analyzer: &'a Analyzer,
    parameters: Vec<(String, Vec<f64>)>,
}

impl<'a> GridSearch<'a> {
    /// Creates a new grid search over `analyzer`.
    pub fn new(analyzer: &'a Analyzer) -> Self {
        Self {
            analyzer,
            parameters: Vec::new(),
        }
    }

    /// Adds a parameter to sweep with specific values.
    pub fn add_parameter(mut self, transition: &str, values: Vec<f64>) -> Self {
        set_parameter(&mut self.parameters, transition, values);
        self
    }

    /// Adds a parameter to sweep with evenly spaced values.
    pub fn add_parameter_range(
        mut self,
        transition: &str,
        min: f64,
        max: f64,
        steps: usize,
    ) -> Self {
        set_parameter(&mut self.parameters, transition, linspace(min, max, steps));
        self
    }

    /// Executes the grid search.
    pub fn run(&self) -> GridResult {
        let combinations = self.generate_combinations();

        let mut scores = Vec::with_capacity(combinations.len());
        let mut best_score = f64::NEG_INFINITY;
        let mut best_parameters = HashMap::new();
        let mut best_index = 0;

        for (i, combo) in combinations.iter().enumerate() {
            let mut test_rates = self.analyzer.rates.clone();
            for (k, v) in combo {
                test_rates.insert(k.clone(), *v);
            }

            let score = self.analyzer.simulate(&test_rates);
            scores.push(score);

            if score > best_score {
                best_score = score;
                best_parameters = combo.clone();
                best_index = i;
            }
        }

        GridResult {
            combinations,
            scores,
            best: GridBest {
                parameters: best_parameters,
                score: best_score,
                index: best_index,
            },
        }
    }

    /// Generates every parameter combination, in the same mixed-radix order
    /// as go-pflow's `generateCombinations` (parameter names sorted, least
    /// significant first).
    fn generate_combinations(&self) -> Vec<HashMap<String, f64>> {
        let mut params: Vec<&(String, Vec<f64>)> = self.parameters.iter().collect();
        params.sort_by(|a, b| a.0.cmp(&b.0));

        let total: usize = params.iter().map(|(_, v)| v.len()).product();
        let mut combinations = Vec::with_capacity(total);

        for i in 0..total {
            let mut combo = HashMap::new();
            let mut idx = i;
            for (name, values) in &params {
                combo.insert(name.clone(), values[idx % values.len()]);
                idx /= values.len();
            }
            combinations.push(combo);
        }

        combinations
    }
}

fn set_parameter(parameters: &mut Vec<(String, Vec<f64>)>, transition: &str, values: Vec<f64>) {
    if let Some(entry) = parameters.iter_mut().find(|(k, _)| k == transition) {
        entry.1 = values;
    } else {
        parameters.push((transition.to_string(), values));
    }
}

/// Results from a grid search.
#[derive(Debug, Clone)]
pub struct GridResult {
    pub combinations: Vec<HashMap<String, f64>>,
    pub scores: Vec<f64>,
    pub best: GridBest,
}

/// The best combination found by a [`GridSearch`].
#[derive(Debug, Clone)]
pub struct GridBest {
    pub parameters: HashMap<String, f64>,
    pub score: f64,
    pub index: usize,
}

/// `steps` evenly spaced values from `min` to `max`, inclusive, matching
/// go-pflow's `SweepRateRange`/`AddParameterRange` loop
/// (`min + (max-min)*i/(steps-1)`).
fn linspace(min: f64, max: f64, steps: usize) -> Vec<f64> {
    if steps == 0 {
        return Vec::new();
    }
    if steps == 1 {
        return vec![min];
    }
    (0..steps)
        .map(|i| min + (max - min) * i as f64 / (steps - 1) as f64)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::Builder;

    /// tokens -> to_win -> win, tokens -> to_lose -> lose, matching
    /// go-pflow's `sensitivity_test.go` `createGameNet`.
    fn game_net() -> (PetriNet, State, HashMap<String, f64>) {
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

        let state = net.set_state(None);
        let rates = HashMap::from([("to_win".to_string(), 0.6), ("to_lose".to_string(), 0.4)]);
        (net, state, rates)
    }

    #[test]
    fn place_scorer_reads_final_value() {
        let (net, state, rates) = game_net();
        let prob = Problem::new(net, state, [0.0, 10.0], rates);
        let sol = solve(&prob, &tsit5(), &Options::default_opts());

        let scorer = place_scorer("win");
        assert!(scorer(&sol) > 0.0);
    }

    #[test]
    fn diff_scorer_reads_two_places() {
        let (net, state, rates) = game_net();
        let prob = Problem::new(net, state, [0.0, 10.0], rates);
        let sol = solve(&prob, &tsit5(), &Options::default_opts());

        let scorer = diff_scorer("win", "lose");
        assert!(scorer(&sol) > 0.0);
    }

    #[test]
    fn analyze_rates_ranks_by_absolute_impact() {
        let (net, state, rates) = game_net();
        let analyzer =
            Analyzer::new(net, state, rates, diff_scorer("win", "lose")).with_time_span(0.0, 10.0);

        let result = analyzer.analyze_rates();

        assert!(result.baseline != 0.0);
        assert!(result.scores.contains_key("to_win"));
        assert!(result.scores.contains_key("to_lose"));

        // Disabling to_win should hurt; disabling to_lose should help.
        assert!(result.impact["to_win"] < 0.0);
        assert!(result.impact["to_lose"] > 0.0);

        assert_eq!(result.ranking.len(), 2);
        assert!(result.ranking[0].impact.abs() >= result.ranking[1].impact.abs());
    }

    #[test]
    fn sweep_rate_finds_best_at_highest_rate() {
        let (net, state, rates) = game_net();
        let analyzer =
            Analyzer::new(net, state, rates, diff_scorer("win", "lose")).with_time_span(0.0, 10.0);

        let result = analyzer.sweep_rate("to_win", &[0.0, 0.25, 0.5, 0.75, 1.0]);

        assert_eq!(result.scores.len(), 5);
        assert!(result.scores[4] > result.scores[0]);
        assert_eq!(result.best.value, 1.0);
    }

    #[test]
    fn sweep_rate_range_is_evenly_spaced() {
        let (net, state, rates) = game_net();
        let analyzer =
            Analyzer::new(net, state, rates, diff_scorer("win", "lose")).with_time_span(0.0, 10.0);

        let result = analyzer.sweep_rate_range("to_win", 0.0, 1.0, 5);
        let expected = [0.0, 0.25, 0.5, 0.75, 1.0];
        for (v, e) in result.values.iter().zip(expected) {
            assert!((v - e).abs() < 1e-9);
        }
    }

    #[test]
    fn gradient_signs_match_which_outcome_the_rate_favors() {
        let (net, state, rates) = game_net();
        let analyzer =
            Analyzer::new(net, state, rates, diff_scorer("win", "lose")).with_time_span(0.0, 10.0);

        assert!(analyzer.gradient("to_win", 0.01) > 0.0);
        assert!(analyzer.gradient("to_lose", 0.01) < 0.0);
    }

    #[test]
    fn all_gradients_covers_every_transition() {
        let (net, state, rates) = game_net();
        let analyzer =
            Analyzer::new(net, state, rates, diff_scorer("win", "lose")).with_time_span(0.0, 10.0);

        let gradients = analyzer.all_gradients(0.01);
        assert_eq!(gradients.len(), 2);
        assert!(gradients.contains_key("to_win"));
    }

    #[test]
    fn grid_search_runs_every_combination_and_finds_the_best() {
        let (net, state, rates) = game_net();
        let analyzer =
            Analyzer::new(net, state, rates, diff_scorer("win", "lose")).with_time_span(0.0, 5.0);

        let grid = GridSearch::new(&analyzer)
            .add_parameter("to_win", vec![0.3, 0.6, 0.9])
            .add_parameter("to_lose", vec![0.2, 0.4]);

        let result = grid.run();

        assert_eq!(result.combinations.len(), 6);
        assert_eq!(result.scores.len(), 6);
        assert_eq!(result.best.parameters["to_win"], 0.9);
        assert_eq!(result.best.parameters["to_lose"], 0.2);
    }

    #[test]
    fn grid_search_add_parameter_range() {
        let (net, state, rates) = game_net();
        let analyzer =
            Analyzer::new(net, state, rates, place_scorer("win")).with_time_span(0.0, 5.0);

        let grid = GridSearch::new(&analyzer).add_parameter_range("to_win", 0.0, 1.0, 3);
        let result = grid.run();

        assert_eq!(result.combinations.len(), 3);
    }
}
