//! Gradient and derivative-free optimizers, operating on plain `&[f64] -> f64` /
//! `&[f64] -> (f64, Vec<f64>)` closures with no [`crate::problem::LearnableProblem`]
//! coupling — the safest, most purely-additive part of the port. [`crate::fit`] is the
//! thin wiring layer on top that builds these closures from a problem + dataset.
//!
//! Mirrors go-pflow's `learn/gradopt.go` (Adam, backtracking gradient descent) and
//! `learn/optimize.go` (Nelder-Mead, coordinate descent).

pub mod gradopt;
pub mod nelder_mead;

pub use gradopt::{adam_minimize, descent_backtracking, minimize_gradient};
pub use nelder_mead::{coordinate_descent, minimize, nelder_mead};

/// Common result shape every optimizer in this module returns.
#[derive(Debug, Clone)]
pub struct FitResult {
    pub params: Vec<f64>,
    pub initial_loss: f64,
    pub final_loss: f64,
    pub iters: usize,
    pub evals: usize,
    pub converged: bool,
}

/// `max |v_i|` — shared by both gradient-based optimizers' convergence check.
pub(crate) fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0_f64, |m, &x| m.max(x.abs()))
}
