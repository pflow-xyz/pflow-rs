//! Adam and Armijo-backtracking steepest descent, over an arbitrary
//! `&[f64] -> (f64, Vec<f64>)` (value, gradient) closure.
//!
//! Mirrors go-pflow's `learn/gradopt.go`. Both optimizers track and return the
//! best-seen point (a rejected/`+Inf` evaluation can leave the *current* iterate worse
//! than the best one already visited), and treat a `+Inf` value as a rejected point to
//! retry from rather than a hard error — the caller's closure is expected to return
//! `+Inf` for a truncated solve or a non-finite loss/gradient, never to panic.

use super::max_abs;

/// Options shared by [`adam_minimize`] and [`descent_backtracking`] (and their
/// [`minimize_gradient`] dispatcher). Scoped to this module — no
/// [`crate::problem::LearnableProblem`]/[`crate::fit::FitOptions`] coupling — so
/// `optim` stays usable as a plain numerical-optimization module on its own.
#[derive(Debug, Clone, Copy)]
pub struct GradOptSettings {
    pub max_iters: usize,
    /// Converged when the per-iteration loss change drops below this.
    pub tolerance: f64,
    /// Step size. `0.0` selects a per-optimizer default (Adam: `0.05`;
    /// backtracking descent: `1.0` initial step).
    pub learn_rate: f64,
    /// Converged when `max|grad| < grad_tol`. `0.0` selects `1e-6`.
    pub grad_tol: f64,
    pub verbose: bool,
}

impl Default for GradOptSettings {
    fn default() -> Self {
        Self {
            max_iters: 1000,
            tolerance: 1e-4,
            learn_rate: 0.0,
            grad_tol: 0.0,
            verbose: false,
        }
    }
}

/// Adam (Kingma & Ba) with bias correction. β₁ = 0.9, β₂ = 0.999, ε = 1e-8 are
/// constants folded exactly as go-pflow's own compile-time constants are (`1 - beta1`
/// etc. computed via `powf`, not hardcoded to a different ULP) — see the bias-correction
/// loop below.
///
/// A `+Inf` evaluation halves an internal step scale and retries from the pre-step
/// point (at most 10 halvings, then stop, returning the best point seen so far).
pub fn adam_minimize(
    fg: &dyn Fn(&[f64]) -> (f64, Vec<f64>),
    x0: &[f64],
    settings: &GradOptSettings,
) -> super::FitResult {
    const BETA1: f64 = 0.9;
    const BETA2: f64 = 0.999;
    const EPS: f64 = 1e-8;

    let lr = if settings.learn_rate == 0.0 {
        0.05
    } else {
        settings.learn_rate
    };
    let grad_tol = if settings.grad_tol == 0.0 {
        1e-6
    } else {
        settings.grad_tol
    };

    let n = x0.len();
    let mut x = x0.to_vec();
    let (mut loss, mut g) = fg(&x);
    let initial_loss = loss;
    if loss.is_infinite() && loss > 0.0 {
        return super::FitResult {
            params: x,
            initial_loss,
            final_loss: f64::INFINITY,
            iters: 0,
            evals: 1,
            converged: false,
        };
    }

    let mut best = x.clone();
    let mut best_loss = loss;

    let mut m = vec![0.0; n];
    let mut v = vec![0.0; n];
    let mut t_step: i32 = 0;
    let mut evals = 1;
    let mut iters = 0;
    let mut converged = false;

    for iter in 1..=settings.max_iters {
        iters = iter;
        if max_abs(&g) < grad_tol {
            converged = true;
            iters = iter - 1;
            break;
        }

        t_step += 1;
        let bc1 = 1.0 - BETA1.powi(t_step);
        let bc2 = 1.0 - BETA2.powi(t_step);
        let mut step = vec![0.0; n];
        for i in 0..n {
            m[i] = BETA1 * m[i] + (1.0 - BETA1) * g[i];
            v[i] = BETA2 * v[i] + (1.0 - BETA2) * g[i] * g[i];
            step[i] = lr * (m[i] / bc1) / ((v[i] / bc2).sqrt() + EPS);
        }

        let mut scale = 1.0;
        let mut accepted = false;
        let mut cand = vec![0.0; n];
        let mut cand_loss = f64::INFINITY;
        let mut cand_grad = Vec::new();
        for _retry in 0..=10 {
            for i in 0..n {
                cand[i] = x[i] - scale * step[i];
            }
            let (cl, cg) = fg(&cand);
            evals += 1;
            cand_loss = cl;
            cand_grad = cg;
            if !cand_grad.is_empty() && !(cand_loss.is_infinite() && cand_loss > 0.0) {
                accepted = true;
                break;
            }
            scale *= 0.5;
        }
        if !accepted {
            converged = false;
            break;
        }

        let prev_loss = loss;
        x = cand;
        loss = cand_loss;
        g = cand_grad;
        if loss < best_loss {
            best_loss = loss;
            best = x.clone();
        }

        if settings.verbose && iter % 100 == 0 {
            eprintln!("adam iter {iter}: loss = {loss}");
        }

        if (prev_loss - loss).abs() < settings.tolerance {
            converged = true;
            break;
        }
    }

    super::FitResult {
        params: best,
        initial_loss,
        final_loss: best_loss,
        iters,
        evals,
        converged,
    }
}

/// Steepest descent with an Armijo backtracking line search. The initial step is
/// `settings.learn_rate` (`0.0` selects `1.0`); a trial point is accepted when
/// `f(x - alpha*g) <= f(x) - 1e-4*alpha*||g||^2`, else alpha halves (at most 30
/// halvings, after which the search is treated as converged and the best point
/// returned — exhausting the line search near a true minimum, not a failure).
///
/// `fval` is a cheap value-only closure for line-search trial points (typically a
/// plain solve rather than a full sensitivity solve); `fg` is called once per outer
/// iteration for the actual gradient step.
pub fn descent_backtracking(
    fg: &dyn Fn(&[f64]) -> (f64, Vec<f64>),
    fval: &dyn Fn(&[f64]) -> f64,
    x0: &[f64],
    settings: &GradOptSettings,
) -> super::FitResult {
    let lr = if settings.learn_rate == 0.0 {
        1.0
    } else {
        settings.learn_rate
    };
    let grad_tol = if settings.grad_tol == 0.0 {
        1e-6
    } else {
        settings.grad_tol
    };

    let n = x0.len();
    let mut x = x0.to_vec();
    let (mut f0, mut g) = fg(&x);
    let initial_loss = f0;
    let mut evals = 1;
    if g.is_empty() || (f0.is_infinite() && f0 > 0.0) {
        return super::FitResult {
            params: x,
            initial_loss,
            final_loss: f64::INFINITY,
            iters: 0,
            evals,
            converged: false,
        };
    }

    let mut best = x.clone();
    let mut best_loss = f0;
    let mut iters = 0;
    let mut converged = false;

    for iter in 1..=settings.max_iters {
        iters = iter;
        if max_abs(&g) < grad_tol {
            converged = true;
            iters = iter - 1;
            break;
        }

        let gnorm2: f64 = g.iter().map(|gi| gi * gi).sum();
        let mut alpha = lr;
        let mut accepted = false;
        let mut cand = vec![0.0; n];
        for _h in 0..=30 {
            for i in 0..n {
                cand[i] = x[i] - alpha * g[i];
            }
            let fc = fval(&cand);
            evals += 1;
            if !(fc.is_infinite() && fc > 0.0) && fc <= f0 - 1e-4 * alpha * gnorm2 {
                accepted = true;
                break;
            }
            alpha *= 0.5;
        }
        if !accepted {
            // No descent step exists at any tried scale: converged.
            converged = true;
            break;
        }

        let prev = f0;
        x = cand;
        let (new_f0, new_g) = fg(&x);
        evals += 1;
        if new_g.is_empty() || (new_f0.is_infinite() && new_f0 > 0.0) {
            converged = false;
            break;
        }
        f0 = new_f0;
        g = new_g;

        if f0 < best_loss {
            best_loss = f0;
            best = x.clone();
        }

        if settings.verbose && iter % 100 == 0 {
            eprintln!("descent iter {iter}: loss = {f0}");
        }

        if (prev - f0).abs() < settings.tolerance {
            converged = true;
            break;
        }
    }

    super::FitResult {
        params: best,
        initial_loss,
        final_loss: best_loss,
        iters,
        evals,
        converged,
    }
}

/// Method selector for [`minimize_gradient`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradMethod {
    Adam,
    GradientDescent,
}

/// Dispatches to [`adam_minimize`] or [`descent_backtracking`]. For `GradientDescent`,
/// `fg`'s own value (discarding the gradient) doubles as the line search's cheap
/// value closure — a caller with a genuinely cheaper value-only path should call
/// [`descent_backtracking`] directly instead.
pub fn minimize_gradient(
    fg: &dyn Fn(&[f64]) -> (f64, Vec<f64>),
    x0: &[f64],
    method: GradMethod,
    settings: &GradOptSettings,
) -> super::FitResult {
    match method {
        GradMethod::Adam => adam_minimize(fg, x0, settings),
        GradMethod::GradientDescent => {
            let fval = |x: &[f64]| fg(x).0;
            descent_backtracking(fg, &fval, x0, settings)
        }
    }
}
