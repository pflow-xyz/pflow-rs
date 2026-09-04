//! Gradient-check: MSE/RMSE/RelativeMSE loss gradients (`lossgrad.rs`, chained through
//! `solve_with_sensitivities`) against central finite differences of the corresponding
//! plain-solve loss — the same style of check `tests/finite_diff.rs` runs for the
//! sensitivities themselves.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pflow_core::net::PetriNet;
use pflow_core::State;
use pflow_learn::{
    mse_loss, mse_loss_grad, relative_mse_loss, relative_mse_loss_grad, rmse_loss, rmse_loss_grad,
    Dataset, LearnableProblem, ScalarRateFunc,
};
use pflow_solver::methods;
use pflow_solver::ode::Options;

/// A -> t1 -> B, single rate parameter (closed form A(t) = A0 * exp(-theta t)), fit
/// against a synthetic "observed" trajectory built from a nearby theta so the residual
/// is nonzero (avoids the rmse zero-loss special case in the first test).
fn decay_problem(theta: f64) -> LearnableProblem {
    let net = PetriNet::build()
        .place("A", 10.0)
        .place("B", 0.0)
        .transition("t1")
        .arc("A", "t1", 1.0)
        .arc("t1", "B", 1.0)
        .done();
    let u0: State = [("A".to_string(), 10.0), ("B".to_string(), 0.0)]
        .into_iter()
        .collect();
    let mut prob = LearnableProblem::new(net, u0, [0.0, 2.0]);
    prob.set_rate_func("t1", Rc::new(RefCell::new(ScalarRateFunc::new(theta))));
    prob
}

fn solve_opts() -> Options {
    Options {
        dt: 1e-3,
        dtmin: 1e-7,
        dtmax: 0.1,
        abstol: 1e-8,
        reltol: 1e-6,
        maxiters: 500_000,
        adaptive: true,
    }
}

/// Observed data generated at `theta_true`, sampled at a handful of times strictly
/// inside `[0, 2]` (so interpolation, not the endpoint clamp, is what's exercised).
fn synthetic_dataset(theta_true: f64) -> Dataset {
    let prob = decay_problem(theta_true);
    let sol = prob.solve(&methods::tsit5(), &solve_opts()).unwrap();
    let times = vec![0.25, 0.6, 1.1, 1.7];
    let a: Vec<f64> = times
        .iter()
        .map(|&t| pflow_learn::interpolate_at(&sol.t, &sol.get_variable("A"), t))
        .collect();
    let b: Vec<f64> = times
        .iter()
        .map(|&t| pflow_learn::interpolate_at(&sol.t, &sol.get_variable("B"), t))
        .collect();
    let mut observations = HashMap::new();
    observations.insert("A".to_string(), a);
    observations.insert("B".to_string(), b);
    Dataset::new(times, observations, vec!["A".to_string(), "B".to_string()]).unwrap()
}

fn assert_close(name: &str, analytic: f64, fd: f64, atol: f64, rtol: f64) {
    let scale = atol + rtol * analytic.abs().max(fd.abs());
    let diff = (analytic - fd).abs();
    assert!(
        diff <= scale,
        "{name}: analytic {analytic} vs FD {fd} (|diff|={diff} > tol={scale})"
    );
}

/// Central-differences `loss_fn(plain_solve(theta))` w.r.t. theta (the problem's one
/// parameter) by rebuilding and re-solving at theta +/- h.
fn fd_dloss_dtheta(
    theta: f64,
    data: &Dataset,
    loss_fn: impl Fn(&pflow_solver::ode::Solution, &Dataset) -> f64,
) -> f64 {
    let h = 1e-6 * (1.0 + theta.abs());

    let plus = decay_problem(theta + h);
    let sol_plus = plus.solve(&methods::tsit5(), &solve_opts()).unwrap();
    let loss_plus = loss_fn(&sol_plus, data);

    let minus = decay_problem(theta - h);
    let sol_minus = minus.solve(&methods::tsit5(), &solve_opts()).unwrap();
    let loss_minus = loss_fn(&sol_minus, data);

    (loss_plus - loss_minus) / (2.0 * h)
}

#[test]
fn mse_loss_grad_matches_finite_difference() {
    let theta_true = 0.7;
    let theta = 0.5; // fitting point != theta_true, so the residual (and gradient) is nonzero
    let data = synthetic_dataset(theta_true);

    let prob = decay_problem(theta);
    let sens = pflow_learn::solve_with_sensitivities(&prob, &methods::tsit5(), &solve_opts())
        .expect("sensitivity solve");
    let (loss, grad) = mse_loss_grad(&sens, &data);

    // Loss reported alongside the gradient must agree with the plain-solve loss.
    let sol = prob.solve(&methods::tsit5(), &solve_opts()).unwrap();
    assert_close("mse loss value", loss, mse_loss(&sol, &data), 1e-9, 1e-6);

    let fd = fd_dloss_dtheta(theta, &data, mse_loss);
    assert_close("mse d(loss)/dtheta", grad[0], fd, 1e-6, 1e-4);
}

#[test]
fn rmse_loss_grad_matches_finite_difference() {
    let theta_true = 0.7;
    let theta = 0.5;
    let data = synthetic_dataset(theta_true);

    let prob = decay_problem(theta);
    let sens = pflow_learn::solve_with_sensitivities(&prob, &methods::tsit5(), &solve_opts())
        .expect("sensitivity solve");
    let (loss, grad) = rmse_loss_grad(&sens, &data);

    let sol = prob.solve(&methods::tsit5(), &solve_opts()).unwrap();
    assert_close("rmse loss value", loss, rmse_loss(&sol, &data), 1e-9, 1e-6);

    let fd = fd_dloss_dtheta(theta, &data, rmse_loss);
    assert_close("rmse d(loss)/dtheta", grad[0], fd, 1e-6, 1e-3);
}

#[test]
fn relative_mse_loss_grad_matches_finite_difference() {
    let theta_true = 0.7;
    let theta = 0.5;
    let data = synthetic_dataset(theta_true);

    let prob = decay_problem(theta);
    let sens = pflow_learn::solve_with_sensitivities(&prob, &methods::tsit5(), &solve_opts())
        .expect("sensitivity solve");
    let (loss, grad) = relative_mse_loss_grad(&sens, &data);

    let sol = prob.solve(&methods::tsit5(), &solve_opts()).unwrap();
    assert_close(
        "relative mse loss value",
        loss,
        relative_mse_loss(&sol, &data),
        1e-9,
        1e-6,
    );

    let fd = fd_dloss_dtheta(theta, &data, relative_mse_loss);
    assert_close("relative mse d(loss)/dtheta", grad[0], fd, 1e-6, 1e-4);
}

#[test]
fn rmse_loss_grad_is_zero_at_zero_loss() {
    // Fit exactly against *its own* solve's trajectory (not a separately-solved one, whose
    // adaptive grid can land at slightly different points and reintroduce a tiny nonzero
    // residual through interpolation): residual, and therefore loss, is then exactly zero
    // at every observation time - the 0/0 special case rmse_loss_grad must not divide
    // through.
    let theta = 0.7;
    let prob = decay_problem(theta);
    let sens = pflow_learn::solve_with_sensitivities(&prob, &methods::tsit5(), &solve_opts())
        .expect("sensitivity solve");

    let times = vec![0.25, 0.6, 1.1, 1.7];
    let a: Vec<f64> = times
        .iter()
        .map(|&t| pflow_learn::interpolate_at(&sens.sol.t, &sens.sol.get_variable("A"), t))
        .collect();
    let b: Vec<f64> = times
        .iter()
        .map(|&t| pflow_learn::interpolate_at(&sens.sol.t, &sens.sol.get_variable("B"), t))
        .collect();
    let mut observations = HashMap::new();
    observations.insert("A".to_string(), a);
    observations.insert("B".to_string(), b);
    let data = Dataset::new(times, observations, vec!["A".to_string(), "B".to_string()]).unwrap();

    let (loss, grad) = rmse_loss_grad(&sens, &data);

    assert_eq!(loss, 0.0);
    for g in grad {
        assert_eq!(g, 0.0);
    }
}

#[test]
fn dataset_rejects_length_mismatch() {
    let mut observations = HashMap::new();
    observations.insert("A".to_string(), vec![1.0, 2.0]); // 2 entries, times has 3
    let err = Dataset::new(vec![0.0, 1.0, 2.0], observations, vec!["A".to_string()]).unwrap_err();
    match err {
        pflow_learn::LearnError::DatasetLengthMismatch {
            place,
            expected,
            found,
        } => {
            assert_eq!(place, "A");
            assert_eq!(expected, 3);
            assert_eq!(found, 2);
        }
        other => panic!("expected DatasetLengthMismatch, got {other:?}"),
    }
}
