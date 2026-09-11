//! Computes insights from simulation results, ported from go-pflow's
//! `results/analysis.go`.

use std::collections::HashMap;

use super::types::{
    Analysis, Conservation, Crossing, Peak, Results, Stat, SteadyState, TokenBalance,
};

/// Computes insights from a [`Results`] value.
pub struct Analyzer<'a> {
    results: &'a Results,
}

impl<'a> Analyzer<'a> {
    /// Creates an analyzer for `results`.
    pub fn new(results: &'a Results) -> Self {
        Self { results }
    }

    /// Runs every analysis function and returns the combined [`Analysis`].
    pub fn compute_all(&self) -> Analysis {
        let mut analysis = Analysis::default();

        let time = &self.results.data.timeseries.time.downsampled;

        // Sorted so output order is deterministic (go-pflow's map iteration
        // is not, but nothing there depends on order either — sorting here
        // costs nothing and makes this port's output reproducible).
        let mut var_names: Vec<&String> = self.results.data.timeseries.variables.keys().collect();
        var_names.sort();

        for var_name in var_names {
            let var_data = &self.results.data.timeseries.variables[var_name];
            let data = &var_data.downsampled;

            for mut p in Self::find_peaks(time, data) {
                p.variable = var_name.clone();
                analysis.peaks.push(p);
            }
            for mut t in Self::find_troughs(time, data) {
                t.variable = var_name.clone();
                analysis.troughs.push(t);
            }

            analysis
                .statistics
                .insert(var_name.clone(), Self::compute_stats(data));
        }

        analysis.crossings = self.find_crossings();
        analysis.steady_state = Some(self.detect_steady_state(0.01, 10.0));
        analysis.conservation = Some(self.check_conservation());

        analysis
    }

    /// Detects local maxima.
    fn find_peaks(time: &[f64], data: &[f64]) -> Vec<Peak> {
        if data.len() < 3 {
            return Vec::new();
        }

        let mut peaks = Vec::new();
        for i in 1..data.len() - 1 {
            if data[i] > data[i - 1] && data[i] > data[i + 1] {
                let mut left_min = data[i - 1];
                let mut right_min = data[i + 1];
                for &v in &data[..i.saturating_sub(1)] {
                    if v < left_min {
                        left_min = v;
                    }
                }
                for &v in &data[(i + 2)..] {
                    if v < right_min {
                        right_min = v;
                    }
                }
                let prominence = data[i] - left_min.max(right_min);

                peaks.push(Peak {
                    variable: String::new(),
                    time: time[i],
                    value: data[i],
                    prominence,
                });
            }
        }
        peaks
    }

    /// Detects local minima.
    fn find_troughs(time: &[f64], data: &[f64]) -> Vec<Peak> {
        if data.len() < 3 {
            return Vec::new();
        }

        let mut troughs = Vec::new();
        for i in 1..data.len() - 1 {
            if data[i] < data[i - 1] && data[i] < data[i + 1] {
                troughs.push(Peak {
                    variable: String::new(),
                    time: time[i],
                    value: data[i],
                    prominence: 0.0,
                });
            }
        }
        troughs
    }

    /// Detects where variables intersect.
    fn find_crossings(&self) -> Vec<Crossing> {
        let mut crossings = Vec::new();

        let time = &self.results.data.timeseries.time.downsampled;
        let vars = &self.results.data.timeseries.variables;

        let mut var_names: Vec<&String> = vars.keys().collect();
        var_names.sort();

        for i in 0..var_names.len() {
            for j in (i + 1)..var_names.len() {
                let var1 = var_names[i];
                let var2 = var_names[j];

                let data1 = &vars[var1].downsampled;
                let data2 = &vars[var2].downsampled;

                for k in 0..time.len().saturating_sub(1) {
                    let diff1 = data1[k] - data2[k];
                    let diff2 = data1[k + 1] - data2[k + 1];

                    if diff1 * diff2 < 0.0 {
                        let t_cross =
                            time[k] + (time[k + 1] - time[k]) * (-diff1) / (diff2 - diff1);
                        let v_cross = data1[k]
                            + (data1[k + 1] - data1[k]) * (t_cross - time[k])
                                / (time[k + 1] - time[k]);

                        crossings.push(Crossing {
                            var1: var1.clone(),
                            var2: var2.clone(),
                            time: t_cross,
                            value: v_cross,
                        });
                    }
                }
            }
        }

        crossings
    }

    /// Checks if the system reached equilibrium.
    fn detect_steady_state(&self, rel_tol: f64, window_duration: f64) -> SteadyState {
        let time = &self.results.data.timeseries.time.downsampled;
        if time.len() < 2 {
            return SteadyState {
                reached: false,
                time: 0.0,
                values: None,
                tolerance: rel_tol,
            };
        }

        let dt = time[1] - time[0];
        let mut window_size = (window_duration / dt) as usize;
        if window_size < 2 {
            window_size = 2;
        }
        if window_size > time.len() / 2 {
            window_size = time.len() / 2;
        }

        let mut all_steady = true;
        let mut steady_time = time[time.len() - 1];

        for var_data in self.results.data.timeseries.variables.values() {
            let data = &var_data.downsampled;

            let mut var_steady = false;
            if window_size < data.len() {
                // `i` indexes both `data` and `time`, so `enumerate()` over
                // either alone would not give the other.
                #[allow(clippy::needless_range_loop)]
                for i in window_size..data.len() {
                    let mut max_change = 0.0_f64;
                    for j in (i - window_size)..i {
                        if data[j] != 0.0 {
                            let change = ((data[j + 1] - data[j]) / data[j]).abs();
                            max_change = max_change.max(change);
                        }
                    }
                    if max_change < rel_tol {
                        var_steady = true;
                        if time[i] < steady_time {
                            steady_time = time[i];
                        }
                        break;
                    }
                }
            }

            if !var_steady {
                all_steady = false;
            }

            if !var_steady && data.len() > window_size {
                let mut max_abs_change = 0.0_f64;
                for j in (data.len() - window_size)..(data.len() - 1) {
                    let change = (data[j + 1] - data[j]).abs();
                    max_abs_change = max_abs_change.max(change);
                }
                if max_abs_change < 1e-6 {
                    var_steady = true;
                }
            }

            if !var_steady {
                all_steady = false;
            }
        }

        let mut ss = SteadyState {
            reached: all_steady,
            time: 0.0,
            values: None,
            tolerance: rel_tol,
        };

        if all_steady {
            ss.time = steady_time;
            ss.values = Some(self.results.data.summary.final_state.clone());
        }

        ss
    }

    /// Verifies mass balance.
    fn check_conservation(&self) -> Conservation {
        let initial = &self.results.simulation.initial_state;
        let r#final = &self.results.data.summary.final_state;

        let initial_total: f64 = initial.values().sum();
        let final_total: f64 = r#final.values().sum();

        let conserved = (final_total - initial_total).abs() < 1e-6;

        let mut c = Conservation {
            total_tokens: TokenBalance {
                initial: initial_total,
                r#final: final_total,
                conserved,
            },
            invariants: Vec::new(),
        };

        if conserved {
            let mut places: Vec<String> = initial.keys().cloned().collect();
            places.sort();
            let coeffs = vec![1.0; places.len()];

            c.invariants.push(super::types::Invariant {
                places,
                coefficients: coeffs,
                value: initial_total,
            });
        }

        c
    }

    /// Calculates a statistical summary.
    fn compute_stats(data: &[f64]) -> Stat {
        if data.is_empty() {
            return Stat::default();
        }

        let mut min = data[0];
        let mut max = data[0];
        let mut sum = 0.0;

        for &v in data {
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
            sum += v;
        }

        let mean = sum / data.len() as f64;

        let sum_sq: f64 = data.iter().map(|v| (v - mean).powi(2)).sum();
        let std = (sum_sq / data.len() as f64).sqrt();

        let mut sorted = data.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mid = sorted.len() / 2;
        #[allow(clippy::manual_is_multiple_of)]
        let median = if sorted.len() % 2 == 0 {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        };

        Stat {
            min,
            max,
            mean,
            median,
            std,
        }
    }
}

/// Duplicates a map, matching go-pflow's `copyMap` helper.
pub(crate) fn copy_map(m: &HashMap<String, f64>) -> HashMap<String, f64> {
    m.clone()
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        Data, Metadata, Model, SeriesData, Simulation, Summary, TimeData, Timeseries,
    };
    use super::*;

    fn results_with(time: Vec<f64>, series: HashMap<String, Vec<f64>>) -> Results {
        let variables = series
            .into_iter()
            .map(|(name, downsampled)| {
                (
                    name,
                    SeriesData {
                        full: vec![],
                        downsampled,
                    },
                )
            })
            .collect();

        Results {
            version: "1.0.0".into(),
            metadata: Metadata {
                timestamp: String::new(),
                solver: String::new(),
                status: String::new(),
                error: String::new(),
                compute_time: 0.0,
            },
            model: Model {
                name: String::new(),
                places: vec![],
                transitions: vec![],
                arcs: 0,
                structure: None,
            },
            simulation: Simulation {
                timespan: [
                    time.first().copied().unwrap_or(0.0),
                    time.last().copied().unwrap_or(0.0),
                ],
                initial_state: HashMap::new(),
                rates: HashMap::new(),
                options: None,
            },
            data: Data {
                summary: Summary {
                    points: time.len(),
                    final_time: time.last().copied().unwrap_or(0.0),
                    final_state: HashMap::new(),
                },
                timeseries: Timeseries {
                    time: TimeData {
                        full: vec![],
                        downsampled: time,
                    },
                    variables,
                },
            },
            analysis: None,
            events: vec![],
        }
    }

    #[test]
    fn find_peaks_and_troughs_locate_local_extrema() {
        let time = vec![0.0, 1.0, 2.0];

        let peaks = Analyzer::find_peaks(&time, &[0.0, 5.0, 0.0]);
        assert_eq!(peaks.len(), 1);
        assert_eq!(peaks[0].time, 1.0);
        assert_eq!(peaks[0].value, 5.0);
        // Prominence: height above the higher of the two surrounding minima.
        assert_eq!(peaks[0].prominence, 5.0);

        let troughs = Analyzer::find_troughs(&time, &[5.0, 0.0, 5.0]);
        assert_eq!(troughs.len(), 1);
        assert_eq!(troughs[0].time, 1.0);
        assert_eq!(troughs[0].value, 0.0);
    }

    #[test]
    fn find_peaks_reports_every_local_maximum() {
        // Two humps: index 1 and index 3 are each local maxima.
        let time: Vec<f64> = (0..5).map(|i| i as f64).collect();
        let data = vec![0.0, 5.0, 1.0, 4.0, 0.0];

        let peaks = Analyzer::find_peaks(&time, &data);
        assert_eq!(peaks.len(), 2);
        assert_eq!(peaks[0].value, 5.0);
        assert_eq!(peaks[1].value, 4.0);
    }

    #[test]
    fn find_peaks_needs_at_least_three_points() {
        let time = vec![0.0, 1.0];
        let data = vec![1.0, 2.0];
        assert!(Analyzer::find_peaks(&time, &data).is_empty());
        assert!(Analyzer::find_troughs(&time, &data).is_empty());
    }

    #[test]
    fn compute_stats_matches_hand_computed_summary() {
        let stat = Analyzer::compute_stats(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(stat.min, 1.0);
        assert_eq!(stat.max, 4.0);
        assert_eq!(stat.mean, 2.5);
        assert_eq!(stat.median, 2.5);
        assert!((stat.std - 1.118033988749895).abs() < 1e-12);
    }

    #[test]
    fn compute_stats_of_empty_data_is_zeroed() {
        assert_eq!(Analyzer::compute_stats(&[]), Stat::default());
    }

    #[test]
    fn find_crossings_locates_a_sign_change_by_interpolation() {
        // A stays below B, then overtakes it strictly between t=1 and t=2 —
        // meeting exactly on a grid point (the more obvious test data)
        // leaves `diff1*diff2 == 0`, which go-pflow's algorithm does not
        // count as a sign change, so the crossing has to fall strictly
        // between two sample points to be found at all.
        let time = vec![0.0, 1.0, 2.0];
        let r = results_with(
            time,
            HashMap::from([
                ("A".to_string(), vec![0.0, 1.0, 4.0]),
                ("B".to_string(), vec![3.0, 2.0, 0.0]),
            ]),
        );
        let crossings = Analyzer::new(&r).find_crossings();
        assert_eq!(crossings.len(), 1);
        assert!(crossings[0].time > 1.0 && crossings[0].time < 2.0);
        // Linear interpolation: diff goes from -1 (t=1) to +4 (t=2), zero at
        // t = 1 + 1/5 = 1.2; A there is 1 + 3*0.2 = 1.6.
        assert!((crossings[0].time - 1.2).abs() < 1e-9);
        assert!((crossings[0].value - 1.6).abs() < 1e-9);
    }

    #[test]
    fn detect_steady_state_reports_unreached_with_too_little_data() {
        let r = results_with(vec![0.0], HashMap::new());
        let ss = Analyzer::new(&r).detect_steady_state(0.01, 10.0);
        assert!(!ss.reached);
    }

    #[test]
    fn detect_steady_state_finds_a_flat_tail() {
        // 20 points, flat after index 10 — well within the relative
        // tolerance in a window shorter than the flat tail.
        let time: Vec<f64> = (0..20).map(|i| i as f64).collect();
        let mut data = vec![0.0; 20];
        for (i, v) in data.iter_mut().enumerate() {
            *v = if i < 10 { i as f64 } else { 10.0 };
        }
        let mut r = results_with(time, HashMap::from([("A".to_string(), data)]));
        r.data.summary.final_state = HashMap::from([("A".to_string(), 10.0)]);
        let ss = Analyzer::new(&r).detect_steady_state(0.01, 3.0);
        assert!(ss.reached);
        assert_eq!(ss.values.unwrap()["A"], 10.0);
    }

    #[test]
    fn check_conservation_flags_a_closed_net_and_records_an_invariant() {
        let mut r = results_with(vec![0.0, 1.0], HashMap::new());
        r.simulation.initial_state =
            HashMap::from([("A".to_string(), 3.0), ("B".to_string(), 2.0)]);
        r.data.summary.final_state =
            HashMap::from([("A".to_string(), 1.0), ("B".to_string(), 4.0)]);

        let c = Analyzer::new(&r).check_conservation();
        assert!(c.total_tokens.conserved);
        assert_eq!(c.total_tokens.initial, 5.0);
        assert_eq!(c.total_tokens.r#final, 5.0);
        assert_eq!(c.invariants.len(), 1);
        assert_eq!(
            c.invariants[0].places,
            vec!["A".to_string(), "B".to_string()]
        );
    }

    #[test]
    fn check_conservation_finds_no_invariant_when_totals_differ() {
        let mut r = results_with(vec![0.0], HashMap::new());
        r.simulation.initial_state = HashMap::from([("A".to_string(), 3.0)]);
        r.data.summary.final_state = HashMap::from([("A".to_string(), 1.0)]);

        let c = Analyzer::new(&r).check_conservation();
        assert!(!c.total_tokens.conserved);
        assert!(c.invariants.is_empty());
    }

    #[test]
    fn compute_all_populates_every_section() {
        let time: Vec<f64> = (0..5).map(|i| i as f64).collect();
        let mut r = results_with(
            time,
            HashMap::from([("A".to_string(), vec![0.0, 5.0, 1.0, 4.0, 0.0])]),
        );
        r.simulation.initial_state = HashMap::from([("A".to_string(), 0.0)]);
        r.data.summary.final_state = HashMap::from([("A".to_string(), 0.0)]);

        let analysis = Analyzer::new(&r).compute_all();
        assert_eq!(analysis.peaks.len(), 2);
        assert_eq!(analysis.peaks[0].variable, "A");
        assert_eq!(analysis.troughs.len(), 1);
        assert!(analysis.statistics.contains_key("A"));
        assert!(analysis.steady_state.is_some());
        assert!(analysis.conservation.is_some());
    }
}
