//! MSE/RMSE/RelativeMSE losses, and their gradients w.r.t. a [`crate::problem::LearnableProblem`]'s
//! flat parameter vector, chained through [`crate::sensitivity::Sensitivities`].
//!
//! Mirrors go-pflow's `learn/lossgrad.go`. The key identity this module leans on:
//! **linear interpolation commutes with the chain rule**. A solve's adaptive grid
//! generally lands at different times than a [`crate::dataset::Dataset`]'s own
//! observation times, so both the simulated trajectory *and* its sensitivities are
//! interpolated onto the dataset's times before comparing — and because interpolation
//! is linear in the interpolated series, interpolating the *sensitivity* series onto
//! observation times is exactly the derivative of the interpolated simulation, so no
//! separate re-derivation is needed for the loss gradient's chain rule through
//! interpolation itself.

use std::collections::HashMap;

use pflow_solver::ode::Solution;

use crate::dataset::{interpolate_at, interpolate_solution, Dataset};
use crate::sensitivity::Sensitivities;

/// Per-place weight of 1.0 for every place named in `data.places` — the plain (not
/// relative) weighting `mse_loss`/`mse_loss_grad` use.
fn unit_weights(data: &Dataset) -> HashMap<String, f64> {
    data.places.iter().map(|p| (p.clone(), 1.0)).collect()
}

/// Per-place weight `1 / meanObs^2`, with `meanObs` falling back to `1.0` (not the
/// weight falling back to `1.0`) when a place's mean observation is exactly zero — the
/// weighting `relative_mse_loss`/`relative_mse_loss_grad` use.
fn relative_weights(data: &Dataset) -> HashMap<String, f64> {
    data.places
        .iter()
        .map(|p| {
            let obs = data
                .observations
                .get(p)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let mean = if obs.is_empty() {
                0.0
            } else {
                obs.iter().sum::<f64>() / obs.len() as f64
            };
            let denom = if mean == 0.0 { 1.0 } else { mean };
            (p.clone(), 1.0 / (denom * denom))
        })
        .collect()
}

/// Total number of (place, time) residual terms `data` contributes — the normalizer for
/// every loss below. `Dataset::new` already guarantees `times` is non-empty and every
/// named place's observation vector matches its length, so this is just the product;
/// zero only when `data.places` is empty, in which case every loss here reports 0.0
/// rather than dividing by zero.
fn residual_count(data: &Dataset) -> usize {
    data.places.len() * data.times.len()
}

fn weighted_sq_error(sol: &Solution, data: &Dataset, weights: &HashMap<String, f64>) -> f64 {
    let n = residual_count(data);
    if n == 0 {
        return 0.0;
    }
    let mut total = 0.0;
    for place in &data.places {
        let w = weights.get(place).copied().unwrap_or(1.0);
        let obs = &data.observations[place];
        let sim = interpolate_solution(sol, &data.times, place);
        for j in 0..data.times.len() {
            let residual = sim[j] - obs[j];
            total += w * residual * residual;
        }
    }
    total / n as f64
}

/// Mean squared error between `sol` (interpolated onto `data.times`) and `data`.
pub fn mse_loss(sol: &Solution, data: &Dataset) -> f64 {
    weighted_sq_error(sol, data, &unit_weights(data))
}

/// `sqrt(mse_loss(sol, data))`.
pub fn rmse_loss(sol: &Solution, data: &Dataset) -> f64 {
    mse_loss(sol, data).sqrt()
}

/// MSE with each place's squared residual normalized by that place's mean observation
/// squared (`meanObs` falls back to `1.0` when a place's observations mean to exactly
/// zero, so a place that's identically zero doesn't blow up the loss).
pub fn relative_mse_loss(sol: &Solution, data: &Dataset) -> f64 {
    weighted_sq_error(sol, data, &relative_weights(data))
}

/// Value of `sens_series(place, param)` at every accepted grid point in `sens`,
/// interpolated onto `times` — the sensitivity-series analog of
/// [`crate::dataset::interpolate_solution`].
fn interpolate_sensitivity_series(
    sens: &Sensitivities,
    place: &str,
    param: usize,
    times: &[f64],
) -> Vec<f64> {
    let series: Vec<f64> = (0..sens.t.len())
        .map(|k| sens.at(k, place, param).unwrap_or(0.0))
        .collect();
    times
        .iter()
        .map(|&t| interpolate_at(&sens.t, &series, t))
        .collect()
}

/// Weighted MSE loss and its gradient w.r.t. every parameter in `sens`'s flat parameter
/// vector, via the chain rule `d(mean w*(sim-obs)^2)/dtheta_p = mean 2*w*(sim-obs)*dsim/dtheta_p`.
fn weighted_mse_grad(
    sens: &Sensitivities,
    data: &Dataset,
    weights: &HashMap<String, f64>,
) -> (f64, Vec<f64>) {
    let n = residual_count(data);
    let mut grad = vec![0.0; sens.num_params];
    if n == 0 {
        return (0.0, grad);
    }
    let mut loss = 0.0;
    for place in &data.places {
        let w = weights.get(place).copied().unwrap_or(1.0);
        let obs = &data.observations[place];
        let sim = interpolate_solution(&sens.sol, &data.times, place);
        for p in 0..sens.num_params {
            let dsim = interpolate_sensitivity_series(sens, place, p, &data.times);
            for j in 0..data.times.len() {
                let residual = sim[j] - obs[j];
                grad[p] += 2.0 * w * residual * dsim[j];
            }
        }
        for j in 0..data.times.len() {
            let residual = sim[j] - obs[j];
            loss += w * residual * residual;
        }
    }
    let n = n as f64;
    loss /= n;
    for g in grad.iter_mut() {
        *g /= n;
    }
    (loss, grad)
}

/// `mse_loss` and its gradient, computed from `sens` (whose `sens.sol` is the same
/// trajectory `mse_loss` would interpolate).
pub fn mse_loss_grad(sens: &Sensitivities, data: &Dataset) -> (f64, Vec<f64>) {
    weighted_mse_grad(sens, data, &unit_weights(data))
}

/// `rmse_loss` and its gradient: `d(sqrt(mse))/dtheta = (d(mse)/dtheta) / (2*sqrt(mse))`.
/// At exactly zero loss (a perfect fit) the gradient is reported as all-zero rather than
/// dividing by zero — go-pflow's own special case, since a zero-residual fit has no
/// local direction to improve in anyway.
pub fn rmse_loss_grad(sens: &Sensitivities, data: &Dataset) -> (f64, Vec<f64>) {
    let (mse, mse_grad) = mse_loss_grad(sens, data);
    if mse == 0.0 {
        return (0.0, vec![0.0; mse_grad.len()]);
    }
    let rmse = mse.sqrt();
    let grad = mse_grad.iter().map(|&g| g / (2.0 * rmse)).collect();
    (rmse, grad)
}

/// `relative_mse_loss` and its gradient.
pub fn relative_mse_loss_grad(sens: &Sensitivities, data: &Dataset) -> (f64, Vec<f64>) {
    weighted_mse_grad(sens, data, &relative_weights(data))
}
