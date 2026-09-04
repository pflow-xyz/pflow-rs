//! [`fit`]: one entry point over [`crate::problem::LearnableProblem`] +
//! [`crate::dataset::Dataset`] that dispatches to all four [`crate::optim`] optimizers.
//!
//! Mirrors go-pflow's `learn/optimize.go::Fit` + `learn/gradopt.go::FitGradient`,
//! folded into a single function selected by [`FitMethod`] (Go splits these across two
//! entry points because `Fit`'s objective is a plain `LossFunc` while `FitGradient`'s is
//! a `GradLossFunc` with a different signature — Rust's closures make one function
//! straightforward). Tied-parameter packing ([`crate::tied`]) already works
//! transparently here: [`LearnableProblem::get_all_params`]/`set_all_params` dedup tied
//! parameters regardless of caller, so `fit()` needs no special-casing for them. Adjoint
//! sensitivities ([`crate::adjoint`]) are ported but not wired into this entry point —
//! [`FitMethod::Adam`]/[`FitMethod::GradientDescent`] always build their objective from
//! forward sensitivities ([`crate::sensitivity::solve_with_sensitivities`]); there is no
//! `Sensitivity` selector to route through [`crate::adjoint::solve_adjoint`] instead.

use pflow_solver::methods::{self, Solver};
use pflow_solver::ode::Options;

use crate::dataset::Dataset;
use crate::error::LearnError;
use crate::lossgrad::{mse_loss, mse_loss_grad};
use crate::optim::gradopt::{self, GradMethod, GradOptSettings};
use crate::optim::nelder_mead::{self, SimplexMethod, SimplexSettings};
use crate::optim::FitResult;
use crate::problem::LearnableProblem;
use crate::sensitivity::solve_with_sensitivities;

/// Which optimizer [`fit`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMethod {
    NelderMead,
    CoordinateDescent,
    Adam,
    GradientDescent,
}

/// Loss+gradient objective for the two gradient methods:
/// `(loss, d(loss)/d(flat params))`, chained through a
/// [`crate::sensitivity::Sensitivities`] the way [`crate::lossgrad::mse_loss_grad`] and
/// its `rmse`/`relative_mse` siblings already are.
pub type GradLossFn<'a> =
    dyn Fn(&crate::sensitivity::Sensitivities, &Dataset) -> (f64, Vec<f64>) + 'a;

/// Plain (no-gradient) loss for the two derivative-free methods:
/// `(sol, data) -> loss`.
pub type LossFn<'a> = dyn Fn(&pflow_solver::ode::Solution, &Dataset) -> f64 + 'a;

/// Configures [`fit`]. Field-for-field this is go-pflow's `FitOptions`, minus
/// `Sensitivity`/`AdjointLoss` (adjoint isn't ported yet) and `GradLoss` as a settable
/// field (passed as an explicit parameter to [`fit`] instead, since Rust's closures
/// don't need a nilable-function-pointer-or-default dance).
#[derive(Clone)]
pub struct FitOptions {
    pub max_iters: usize,
    pub tolerance: f64,
    pub method: FitMethod,
    /// Initial per-coordinate step for [`FitMethod::CoordinateDescent`].
    pub step_size: f64,
    pub verbose: bool,
    pub solver_method: Solver,
    pub solver_options: Options,
    /// Gradient-method step size. `0.0` selects a per-optimizer default (see
    /// [`GradOptSettings`]).
    pub learn_rate: f64,
    /// Gradient-method convergence threshold. `0.0` selects `1e-6`.
    pub grad_tol: f64,
}

impl Default for FitOptions {
    fn default() -> Self {
        Self {
            max_iters: 1000,
            tolerance: 1e-4,
            method: FitMethod::NelderMead,
            step_size: 0.01,
            verbose: false,
            solver_method: methods::tsit5(),
            solver_options: Options::default_opts(),
            learn_rate: 0.0,
            grad_tol: 0.0,
        }
    }
}

/// Fits `prob`'s parameters against `data`, minimizing `loss` (for the two
/// derivative-free methods) or `grad_loss` (for the two gradient methods) per
/// `opts.method`, and leaves `prob`'s parameters set to the best point found.
///
/// A solve that truncates, errors, or produces a non-finite loss/gradient is treated
/// as a rejected point (`+Inf`), never a hard error mid-fit — matching go-pflow's
/// `fitGradientCore`/`objective` closures.
///
/// Takes `prob` by shared reference (see the note on
/// [`crate::problem::LearnableProblem::set_all_params`]): every objective closure below
/// mutates `prob`'s installed rate functions through their own `RefCell`s, so several
/// closures can hold `prob` at once without a borrow-checker conflict, and the caller's
/// own `&LearnableProblem`/`&mut LearnableProblem` doesn't need to change shape around
/// the call.
pub fn fit(
    prob: &LearnableProblem,
    data: &Dataset,
    loss: &LossFn,
    grad_loss: &GradLossFn,
    opts: &FitOptions,
) -> Result<FitResult, LearnError> {
    let (params0, _indices) = prob.get_all_params();
    if params0.is_empty() {
        return Err(LearnError::ZeroParams);
    }

    match opts.method {
        FitMethod::Adam | FitMethod::GradientDescent => {
            fit_gradient_core(prob, data, grad_loss, opts, &params0)
        }
        FitMethod::NelderMead | FitMethod::CoordinateDescent => {
            fit_simplex(prob, data, loss, opts, &params0)
        }
    }
}

/// Convenience wrapper: `fit` with plain MSE as both the reported loss and the
/// gradient objective (go-pflow's own default when `LossFunc`/`GradLoss` are nil).
pub fn fit_mse(
    prob: &LearnableProblem,
    data: &Dataset,
    opts: &FitOptions,
) -> Result<FitResult, LearnError> {
    fit(prob, data, &mse_loss, &mse_loss_grad, opts)
}

fn fit_simplex(
    prob: &LearnableProblem,
    data: &Dataset,
    loss: &LossFn,
    opts: &FitOptions,
    params0: &[f64],
) -> Result<FitResult, LearnError> {
    let sol0 = prob.solve(&opts.solver_method, &opts.solver_options)?;
    let initial_loss = loss(&sol0, data);

    let objective = |params: &[f64]| -> f64 {
        prob.set_all_params(params);
        match prob.solve(&opts.solver_method, &opts.solver_options) {
            Ok(sol) => {
                let l = loss(&sol, data);
                if l.is_nan() || l.is_infinite() {
                    f64::INFINITY
                } else {
                    l
                }
            }
            Err(_) => f64::INFINITY,
        }
    };

    let simplex_settings = SimplexSettings {
        max_iters: opts.max_iters,
        tolerance: opts.tolerance,
        step_size: opts.step_size,
        verbose: opts.verbose,
    };
    let method = match opts.method {
        FitMethod::NelderMead => SimplexMethod::NelderMead,
        FitMethod::CoordinateDescent => SimplexMethod::CoordinateDescent,
        _ => unreachable!("fit_simplex only called for simplex methods"),
    };
    // evals already counts every `objective` call the optimizer made; the one solve
    // above (for initial_loss) is the only extra one to add.
    let mut result = nelder_mead::minimize(&objective, params0, method, &simplex_settings);
    result.evals += 1;

    prob.set_all_params(&result.params);
    result.initial_loss = initial_loss;
    Ok(result)
}

fn fit_gradient_core(
    prob: &LearnableProblem,
    data: &Dataset,
    grad_loss: &GradLossFn,
    opts: &FitOptions,
    params0: &[f64],
) -> Result<FitResult, LearnError> {
    let p = params0.len();
    // evals counts plain-solve equivalents: a forward-sensitivity solve costs 1 + P
    // (matching go-pflow's Evals accounting), a plain solve costs 1. `Cell`, not a
    // captured `&mut usize`, because the objective closures below must be `Fn` (the
    // optim module's signature) even though they need to increment a shared counter.
    let evals = std::cell::Cell::new(0usize);

    // valueGrad: one forward-sensitivity solve.
    let value_grad = |theta: &[f64]| -> (f64, Vec<f64>) {
        evals.set(evals.get() + 1 + p);
        prob.set_all_params(theta);
        match solve_with_sensitivities(prob, &opts.solver_method, &opts.solver_options) {
            Ok(sens) => {
                let (loss, grad) = grad_loss(&sens, data);
                if loss.is_nan()
                    || loss.is_infinite()
                    || grad.iter().any(|g| g.is_nan() || g.is_infinite())
                {
                    (f64::INFINITY, Vec::new())
                } else {
                    (loss, grad)
                }
            }
            Err(_) => (f64::INFINITY, Vec::new()),
        }
    };
    // Cheap line-search value closure: a plain solve (no sensitivities), the way
    // go-pflow's `fitGradientCore` builds `value` when `GradLoss` is the MSE default.
    let value = |theta: &[f64]| -> f64 {
        evals.set(evals.get() + 1);
        prob.set_all_params(theta);
        match prob.solve(&opts.solver_method, &opts.solver_options) {
            Ok(sol) => {
                let l = mse_loss(&sol, data);
                if l.is_nan() || l.is_infinite() {
                    f64::INFINITY
                } else {
                    l
                }
            }
            Err(_) => f64::INFINITY,
        }
    };

    let initial_loss = {
        prob.set_all_params(params0);
        let sol = prob.solve(&opts.solver_method, &opts.solver_options)?;
        evals.set(evals.get() + 1);
        mse_loss(&sol, data)
    };

    let grad_settings = GradOptSettings {
        max_iters: opts.max_iters,
        tolerance: opts.tolerance,
        learn_rate: opts.learn_rate,
        grad_tol: opts.grad_tol,
        verbose: opts.verbose,
    };

    let mut result = match opts.method {
        FitMethod::Adam => {
            gradopt::minimize_gradient(&value_grad, params0, GradMethod::Adam, &grad_settings)
        }
        FitMethod::GradientDescent => {
            gradopt::descent_backtracking(&value_grad, &value, params0, &grad_settings)
        }
        _ => unreachable!("fit_gradient_core only called for gradient methods"),
    };

    prob.set_all_params(&result.params);
    let final_sol = prob.solve(&opts.solver_method, &opts.solver_options)?;
    evals.set(evals.get() + 1);
    let final_loss = mse_loss(&final_sol, data);

    result.initial_loss = initial_loss;
    result.final_loss = final_loss;
    result.evals = evals.get();
    Ok(result)
}
