//! `pflow-learn`: parameter fitting, sensitivities, and optimizers for Petri net ODEs.
//!
//! Ported from go-pflow's `learn` package (branch `tsit5-error-estimate`), with
//! pflow-xyz's `public/petri-learn.js` as a second, working reference for design
//! choices. This is currently a narrow first cut: forward sensitivities (the augmented
//! `x` + `dx/dtheta` ODE), the [`crate::ratefunc::RateFunc`] abstraction they need, a
//! minimal [`crate::dataset::Dataset`], and MSE/RMSE/RelativeMSE losses with gradients
//! chained through the sensitivities ([`crate::lossgrad`]).
//! Also ported: [`crate::optim`] (Adam, Armijo-backtracking gradient descent,
//! Nelder-Mead, coordinate descent — `learn/gradopt.go` + `learn/optimize.go`) and
//! [`crate::fit`], the single entry point dispatching to all four by
//! [`crate::fit::FitMethod`] (`learn/optimize.go::Fit` + `learn/gradopt.go::FitGradient`
//! folded together).
//!
//! Also ported: [`crate::adjoint`] — reverse-mode sensitivities (`learn/adjoint.go`).
//! pflow-xyz's `petri-learn.js` carries its own adjoint port and `parity/learn/goldens.json`
//! carries Go-adjoint fixtures (`point1Adjoint`/`point2Adjoint`) for it, so this module
//! IS checked against a cross-language golden (see `parity/README.md` and
//! `tests/parity_goldens.rs`) — not only against forward-mode's own gradient
//! (`solve_with_sensitivities` + `mse_loss_grad`) on the same problem, which
//! `tests/adjoint.rs` checks as a second, independent gate.
//!
//! Also ported: [`crate::tied`] — tied (shared) parameters (`learn/tied.go`,
//! `TiedScalar` mirroring Go's `SharedScalar`) and [`crate::ranking`] — the hinge
//! ranking loss (`learn/ranking.go`).
//!
//! Also ported: [`crate::likelihood`] — the exact CTMC log-likelihood and its
//! closed-form gradient over a discretely observed sample path
//! (`stochastic/likelihood.go`'s `NegLogLikelihood`/`FitDiscrete`), go-pflow's
//! counterpart to forward-sensitivity ODE fitting for data that is a firing
//! sequence rather than a trajectory. It is why this crate depends on
//! `pflow-metamodel`.
//!
//! Dependency shape is otherwise deliberately narrow: `pflow-core`,
//! `pflow-solver` and `pflow-metamodel` at runtime — no autodiff/optimization
//! crate — matching `pflow-solver`'s own zero-dependency posture and the
//! ecosystem's "no ML dependencies" rule.

pub mod adjoint;
pub mod dataset;
pub mod error;
pub mod fit;
pub mod gradrate;
pub mod likelihood;
pub mod lossgrad;
pub mod optim;
pub mod problem;
pub mod ranking;
pub mod ratefunc;
pub mod sensitivity;
pub mod tied;

pub use adjoint::{
    mse_loss_adjoint, relative_mse_loss_adjoint, solve_adjoint, AdjointResult, PointLossGrad,
};
pub use dataset::{interpolate_at, interpolate_solution, Dataset};
pub use error::LearnError;
pub use fit::{fit, fit_mse, FitMethod, FitOptions};
pub use gradrate::{fd_rate_grad, rate_grad, SharedRateFunc};
pub use likelihood::{fit_discrete, neg_log_likelihood, DiscretePath, FireEvent, LikelihoodError};
pub use lossgrad::{
    mse_loss, mse_loss_grad, relative_mse_loss, relative_mse_loss_grad, rmse_loss, rmse_loss_grad,
};
pub use optim::FitResult;
pub use problem::LearnableProblem;
pub use ranking::{hinge_rank_loss, Decision};
pub use ratefunc::{
    Activation, ConstantRateFunc, LinearRateFunc, MLPRateFunc, RateFunc, ScalarRateFunc,
};
pub use sensitivity::{solve_with_sensitivities, Sensitivities};
pub use tied::TiedScalar;
