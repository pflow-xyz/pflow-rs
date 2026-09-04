//! Observed data a [`crate::problem::LearnableProblem`] is fit against, plus the linear
//! interpolation `lossgrad.rs` needs to compare a solve's irregular adaptive-step grid
//! against a dataset's own (generally different) time points.
//!
//! Mirrors go-pflow's `learn/dataset.go`. Interpolation is a plain linear scan, not
//! binary search, deliberately: dataset sizes here are small, and matching the
//! reference's actual arithmetic (rather than "upgrading" it) reduces port risk.

use std::collections::HashMap;

use pflow_solver::ode::Solution;

use crate::error::LearnError;

/// Observed trajectory data for one or more places, at a shared set of times.
///
/// `places` is an explicit, caller-supplied order (not derived from `observations`'s
/// key order) — see the crate-level note on why `state_labels`/`Dataset::places` are
/// nondeterministic in go-pflow's own map-order iteration and are made explicit here
/// instead of "fixed" to silently pick some order.
#[derive(Debug)]
pub struct Dataset {
    pub times: Vec<f64>,
    pub observations: HashMap<String, Vec<f64>>,
    pub places: Vec<String>,
}

impl Dataset {
    /// Errors if `times` is empty, or if any named place's observation vector has a
    /// different length than `times`.
    pub fn new(
        times: Vec<f64>,
        observations: HashMap<String, Vec<f64>>,
        places: Vec<String>,
    ) -> Result<Dataset, LearnError> {
        if times.is_empty() {
            return Err(LearnError::EmptyTimes);
        }
        for place in &places {
            let len = observations.get(place).map(|v| v.len()).unwrap_or(0);
            if len != times.len() {
                return Err(LearnError::DatasetLengthMismatch {
                    place: place.clone(),
                    expected: times.len(),
                    found: len,
                });
            }
        }
        Ok(Dataset {
            times,
            observations,
            places,
        })
    }
}

/// Linear interpolation of `values` (sampled at `times`, ascending) at `t`, clamped to
/// the first/last value outside `[times[0], times[last]]`.
pub fn interpolate_at(times: &[f64], values: &[f64], t: f64) -> f64 {
    if t <= times[0] {
        return values[0];
    }
    let last = times.len() - 1;
    if t >= times[last] {
        return values[last];
    }
    for i in 0..last {
        let (t0, t1) = (times[i], times[i + 1]);
        if t >= t0 && t <= t1 {
            if t1 == t0 {
                return values[i];
            }
            let frac = (t - t0) / (t1 - t0);
            return values[i] + frac * (values[i + 1] - values[i]);
        }
    }
    values[last]
}

/// Interpolates a solved `Solution`'s `place` series onto `times` (usually a dataset's
/// own observation times).
pub fn interpolate_solution(sol: &Solution, times: &[f64], place: &str) -> Vec<f64> {
    let values = sol.get_variable(place);
    times
        .iter()
        .map(|&t| interpolate_at(&sol.t, &values, t))
        .collect()
}
