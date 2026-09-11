//! Constructs [`Results`] from simulation output, ported from go-pflow's
//! `results/builder.go`.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use pflow_core::net::PetriNet;
use pflow_solver::ode::{Options, Solution};

use super::types::{
    Data, Metadata, Model, Results, Simulation, SolverOptions, Summary, TimeData, Timeseries,
    SCHEMA_VERSION,
};

/// Builds a [`Results`] value incrementally.
pub struct Builder {
    results: Results,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    /// Creates a new results builder.
    pub fn new() -> Self {
        Self {
            results: Results {
                version: SCHEMA_VERSION.to_string(),
                metadata: Metadata {
                    timestamp: rfc3339_now(),
                    solver: String::new(),
                    status: String::new(),
                    error: String::new(),
                    compute_time: 0.0,
                },
                model: Model {
                    name: String::new(),
                    places: Vec::new(),
                    transitions: Vec::new(),
                    arcs: 0,
                    structure: None,
                },
                simulation: Simulation {
                    timespan: [0.0, 0.0],
                    initial_state: HashMap::new(),
                    rates: HashMap::new(),
                    options: None,
                },
                data: Data {
                    summary: Summary {
                        points: 0,
                        final_time: 0.0,
                        final_state: HashMap::new(),
                    },
                    timeseries: Timeseries {
                        time: TimeData::default(),
                        variables: HashMap::new(),
                    },
                },
                analysis: None,
                events: Vec::new(),
            },
        }
    }

    /// Sets model information.
    pub fn with_model(mut self, net: &PetriNet, name: &str) -> Self {
        let mut places: Vec<String> = net.places.keys().cloned().collect();
        places.sort();

        let mut transitions: Vec<String> = net.transitions.keys().cloned().collect();
        transitions.sort();

        self.results.model = Model {
            name: name.to_string(),
            places,
            transitions,
            arcs: net.arcs.len(),
            structure: None,
        };
        self
    }

    /// Sets simulation parameters.
    pub fn with_simulation(
        mut self,
        initial_state: &HashMap<String, f64>,
        rates: &HashMap<String, f64>,
        timespan: [f64; 2],
        opts: Option<&Options>,
    ) -> Self {
        self.results.simulation = Simulation {
            timespan,
            initial_state: super::analysis::copy_map(initial_state),
            rates: super::analysis::copy_map(rates),
            options: opts.map(|o| SolverOptions {
                dt: o.dt,
                abstol: o.abstol,
                reltol: o.reltol,
                adaptive: o.adaptive,
            }),
        };
        self
    }

    /// Processes solver output.
    pub fn with_solution(
        mut self,
        sol: &Solution,
        solver_name: &str,
        compute_time: f64,
        downsample_target: usize,
    ) -> Self {
        self.results.metadata.solver = solver_name.to_string();
        self.results.metadata.status = "success".to_string();
        self.results.metadata.compute_time = compute_time;

        let final_state = sol.get_final_state().cloned().unwrap_or_default();
        self.results.data.summary = Summary {
            points: sol.t.len(),
            final_time: sol.t.last().copied().unwrap_or(0.0),
            final_state: final_state.clone(),
        };

        let time_full = sol.t.clone();
        let time_downsampled = downsample(&time_full, downsample_target);

        let mut variables = HashMap::new();
        let mut names: Vec<&String> = final_state.keys().collect();
        names.sort();
        for name in names {
            let var_data = sol.get_variable(name);
            let var_downsampled = downsample_aligned(&time_full, &var_data, &time_downsampled);

            variables.insert(
                name.clone(),
                super::types::SeriesData {
                    full: var_data,
                    downsampled: var_downsampled,
                },
            );
        }

        self.results.data.timeseries = Timeseries {
            time: TimeData {
                full: time_full,
                downsampled: time_downsampled,
            },
            variables,
        };

        self
    }

    /// Sets error status.
    pub fn with_error(mut self, err: impl std::fmt::Display) -> Self {
        self.results.metadata.status = "error".to_string();
        self.results.metadata.error = err.to_string();
        self
    }

    /// Returns the constructed [`Results`].
    pub fn build(self) -> Results {
        self.results
    }
}

/// Reduces `data` to approximately `target_points`, matching go-pflow's
/// `downsample` line for line — including its behavior at the edges:
/// `target_points == 1` writes `data[0]` and then immediately overwrites
/// index 0 with `data[last]`, so the single point returned is the *last*
/// sample, not the first. Faithfully reproduced rather than "fixed" per the
/// port ground rule; callers in practice always pass a target well above 1
/// (the showcase default is 60).
pub fn downsample(data: &[f64], target_points: usize) -> Vec<f64> {
    if data.len() <= target_points {
        return data.to_vec();
    }

    let mut result = vec![0.0; target_points];
    result[0] = data[0];
    result[target_points - 1] = data[data.len() - 1];

    let step = (data.len() - 1) as f64 / (target_points - 1) as f64;
    // `i` is a value in the step formula, not just an index — `enumerate()`
    // over `result` wouldn't give the same thing.
    #[allow(clippy::needless_range_loop)]
    for i in 1..target_points.saturating_sub(1) {
        let idx = (i as f64 * step).round() as usize;
        result[i] = data[idx];
    }

    result
}

/// Downsamples `var_data` to match `time_downsampled`, matching go-pflow's
/// `downsampleAligned`.
pub fn downsample_aligned(
    time_full: &[f64],
    var_data: &[f64],
    time_downsampled: &[f64],
) -> Vec<f64> {
    time_downsampled
        .iter()
        .map(|&target_time| {
            let idx = find_closest_index(time_full, target_time);
            var_data.get(idx).copied().unwrap_or(0.0)
        })
        .collect()
}

/// Finds the index of the value in `data` closest to `target`.
fn find_closest_index(data: &[f64], target: f64) -> usize {
    if data.is_empty() {
        return 0;
    }

    let mut min_dist = (data[0] - target).abs();
    let mut min_idx = 0;

    for (i, &v) in data.iter().enumerate().skip(1) {
        let dist = (v - target).abs();
        if dist < min_dist {
            min_dist = dist;
            min_idx = i;
        }
    }

    min_idx
}

/// Formats "now" as RFC3339 UTC, with no external time dependency (nothing
/// else in this workspace pulls in `chrono` — see the workspace `CLAUDE.md`
/// dependency conventions). This mirrors what `json.Marshal` of a Go
/// `time.Time` produces closely enough for the field to be informative; it
/// is never compared byte-for-byte to a go-pflow golden.
fn rfc3339_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    civil_from_unix(secs)
}

/// Converts a Unix timestamp (UTC, second resolution) into an RFC3339
/// string, using the standard civil-from-days algorithm (Howard Hinnant's
/// `civil_from_days`) so no calendar library is required.
fn civil_from_unix(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = (secs % 86400) as i64;
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_keeps_short_series() {
        let data = vec![1.0, 2.0, 3.0];
        assert_eq!(downsample(&data, 10), data);
    }

    #[test]
    fn downsample_keeps_endpoints() {
        let data: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let out = downsample(&data, 10);
        assert_eq!(out.len(), 10);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[9], 99.0);
    }

    #[test]
    fn find_closest_index_picks_nearest() {
        let data = vec![0.0, 1.0, 2.0, 3.0];
        assert_eq!(find_closest_index(&data, 1.4), 1);
        assert_eq!(find_closest_index(&data, 1.6), 2);
    }

    #[test]
    fn civil_from_unix_epoch() {
        assert_eq!(civil_from_unix(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn civil_from_unix_known_date() {
        // 2024-01-01T00:00:00Z
        assert_eq!(civil_from_unix(1_704_067_200), "2024-01-01T00:00:00Z");
    }

    #[test]
    fn builds_results_from_a_real_ode_solve() {
        use pflow_core::Builder as NetBuilder;
        use pflow_solver::methods::tsit5;
        use pflow_solver::ode::{solve, Problem};
        use std::collections::HashMap;

        let net = NetBuilder::new()
            .place("A", 10.0)
            .place("B", 0.0)
            .transition("convert")
            .arc("A", "convert", 1.0)
            .arc("convert", "B", 1.0)
            .done();

        let state = net.set_state(None);
        let rates: HashMap<String, f64> = HashMap::from([("convert".to_string(), 1.0)]);
        let tspan = [0.0, 5.0];
        let opts = Options::default_opts();

        let prob = Problem::new(net.clone(), state.clone(), tspan, rates.clone());
        let sol = solve(&prob, &tsit5(), &opts);

        let results = Builder::new()
            .with_model(&net, "convert-net")
            .with_simulation(&state, &rates, tspan, Some(&opts))
            .with_solution(&sol, "Tsit5", 0.001, 10)
            .build();

        assert_eq!(results.model.name, "convert-net");
        assert_eq!(results.model.places, vec!["A".to_string(), "B".to_string()]);
        assert_eq!(results.metadata.status, "success");
        assert_eq!(results.metadata.solver, "Tsit5");
        assert!(results.data.summary.final_state["B"] > 0.0);
        assert_eq!(results.data.timeseries.time.downsampled.len(), 10);
        assert!(results.data.timeseries.variables.contains_key("A"));
        assert!(results.data.timeseries.variables.contains_key("B"));

        let analysis = super::super::analysis::Analyzer::new(&results).compute_all();
        assert!(analysis.conservation.unwrap().total_tokens.conserved);
    }

    #[test]
    fn with_error_marks_status_and_records_the_message() {
        let results = Builder::new().with_error("solver diverged").build();
        assert_eq!(results.metadata.status, "error");
        assert_eq!(results.metadata.error, "solver diverged");
    }
}
