//! Fits a small decay/SIR-style net with all four optimizers and asserts each one
//! recovers the true rate parameter to a loose tolerance — the spirit of go-pflow's
//! `learn/gradopt_test.go`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pflow_core::net::PetriNet;
use pflow_core::State;
use pflow_learn::fit::{FitMethod, FitOptions};
use pflow_learn::{
    fit, mse_loss, mse_loss_grad, Activation, Dataset, LearnableProblem, MLPRateFunc, RateFunc,
    ScalarRateFunc,
};
use pflow_solver::methods;
use pflow_solver::ode::Options;

/// A -> t1 -> B, single rate parameter (closed form `A(t) = A0 * exp(-theta t)`).
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

/// Looser than `tests/lossgrad_finite_diff.rs`'s `solve_opts` on purpose: this suite
/// checks that each optimizer *converges*, not gradient accuracy (that's
/// `tests/finite_diff.rs`/`tests/lossgrad_finite_diff.rs`), and the tight tolerances
/// there make every one of the hundreds of solves an optimizer run needs far slower
/// than this smoke test needs to be.
fn solve_opts() -> Options {
    Options::default_opts()
}

/// A synthetic dataset generated at `theta_true`, sampled at a handful of times.
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

fn base_opts(method: FitMethod) -> FitOptions {
    FitOptions {
        max_iters: 200,
        tolerance: 1e-8,
        method,
        step_size: 0.05,
        verbose: false,
        solver_method: methods::tsit5(),
        solver_options: solve_opts(),
        learn_rate: 0.0,
        grad_tol: 0.0,
    }
}

const THETA_TRUE: f64 = 0.7;
const THETA_START: f64 = 0.3;
/// Loose: this is a convergence smoke test, not a gradient-accuracy check (that's
/// `tests/finite_diff.rs`/`tests/lossgrad_finite_diff.rs`).
const THETA_TOL: f64 = 0.05;

#[test]
fn nelder_mead_converges_to_true_theta() {
    let data = synthetic_dataset(THETA_TRUE);
    let prob = decay_problem(THETA_START);
    let opts = base_opts(FitMethod::NelderMead);

    let result = fit(&prob, &data, &mse_loss, &mse_loss_grad, &opts).expect("fit");

    assert!(
        result.final_loss < result.initial_loss,
        "nelder-mead: final loss {} should be less than initial {}",
        result.final_loss,
        result.initial_loss
    );
    assert!(
        (result.params[0] - THETA_TRUE).abs() < THETA_TOL,
        "nelder-mead: theta {} not within {THETA_TOL} of {THETA_TRUE}",
        result.params[0]
    );
}

#[test]
fn coordinate_descent_converges_to_true_theta() {
    let data = synthetic_dataset(THETA_TRUE);
    let prob = decay_problem(THETA_START);
    let mut opts = base_opts(FitMethod::CoordinateDescent);
    opts.max_iters = 2000;

    let result = fit(&prob, &data, &mse_loss, &mse_loss_grad, &opts).expect("fit");

    assert!(
        result.final_loss < result.initial_loss,
        "coordinate-descent: final loss {} should be less than initial {}",
        result.final_loss,
        result.initial_loss
    );
    assert!(
        (result.params[0] - THETA_TRUE).abs() < THETA_TOL,
        "coordinate-descent: theta {} not within {THETA_TOL} of {THETA_TRUE}",
        result.params[0]
    );
}

#[test]
fn adam_converges_to_true_theta() {
    let data = synthetic_dataset(THETA_TRUE);
    let prob = decay_problem(THETA_START);
    let opts = base_opts(FitMethod::Adam);

    let result = fit(&prob, &data, &mse_loss, &mse_loss_grad, &opts).expect("fit");

    assert!(
        result.final_loss < result.initial_loss,
        "adam: final loss {} should be less than initial {}",
        result.final_loss,
        result.initial_loss
    );
    assert!(
        (result.params[0] - THETA_TRUE).abs() < THETA_TOL,
        "adam: theta {} not within {THETA_TOL} of {THETA_TRUE}",
        result.params[0]
    );
}

#[test]
fn gradient_descent_converges_to_true_theta() {
    let data = synthetic_dataset(THETA_TRUE);
    let prob = decay_problem(THETA_START);
    let opts = base_opts(FitMethod::GradientDescent);

    let result = fit(&prob, &data, &mse_loss, &mse_loss_grad, &opts).expect("fit");

    assert!(
        result.final_loss < result.initial_loss,
        "gradient-descent: final loss {} should be less than initial {}",
        result.final_loss,
        result.initial_loss
    );
    assert!(
        (result.params[0] - THETA_TRUE).abs() < THETA_TOL,
        "gradient-descent: theta {} not within {THETA_TOL} of {THETA_TRUE}",
        result.params[0]
    );
}

/// A -> t1 -> B -> t2 -> C: t1 is mechanistic (`ScalarRateFunc`, one fitted rate), t2 is
/// neural (`MLPRateFunc`, state-dependent on B). The point of D4's "analytic layer
/// gradients composed with D1 (forward sensitivities)": one `solve_with_sensitivities`
/// call and one Adam step move *both* the mechanistic parameter and every MLP weight
/// together, through the same augmented ODE.
fn hybrid_problem(theta1: f64, mlp_params: &[f64]) -> LearnableProblem {
    let net = PetriNet::build()
        .place("A", 10.0)
        .place("B", 0.0)
        .place("C", 0.0)
        .transition("t1")
        .transition("t2")
        .arc("A", "t1", 1.0)
        .arc("t1", "B", 1.0)
        .arc("B", "t2", 1.0)
        .arc("t2", "C", 1.0)
        .done();
    let u0: State = [
        ("A".to_string(), 10.0),
        ("B".to_string(), 0.0),
        ("C".to_string(), 0.0),
    ]
    .into_iter()
    .collect();
    let mut prob = LearnableProblem::new(net, u0, [0.0, 2.0]);
    prob.set_rate_func("t1", Rc::new(RefCell::new(ScalarRateFunc::new(theta1))));
    let mut mlp = MLPRateFunc::new(
        &["B".to_string()],
        3,
        false,
        pflow_learn::Activation::Relu,
        true,
    );
    mlp.set_params(mlp_params);
    prob.set_rate_func("t2", Rc::new(RefCell::new(mlp)));
    prob
}

#[test]
fn hybrid_mechanistic_neural_fit_converges() {
    // "True" system: a known mechanistic rate for t1, and a hand-picked MLP for t2 with
    // weights well away from the default init (so the loss surface isn't trivially flat
    // at the fit's own starting point).
    let theta1_true = 0.8;
    let mlp_true = MLPRateFunc::new(&["B".to_string()], 3, false, Activation::Relu, true);
    let mut mlp_true_params = mlp_true.get_params();
    for (i, p) in mlp_true_params.iter_mut().enumerate() {
        *p += 0.15 * (1.0 + (i % 3) as f64);
    }

    let truth = hybrid_problem(theta1_true, &mlp_true_params);
    let sol = truth.solve(&methods::tsit5(), &solve_opts()).unwrap();
    let times = vec![0.2, 0.5, 0.9, 1.3, 1.8];
    let mut observations = HashMap::new();
    for place in ["A", "B", "C"] {
        let vals: Vec<f64> = times
            .iter()
            .map(|&t| pflow_learn::interpolate_at(&sol.t, &sol.get_variable(place), t))
            .collect();
        observations.insert(place.to_string(), vals);
    }
    let data = Dataset::new(
        times,
        observations,
        vec!["A".to_string(), "B".to_string(), "C".to_string()],
    )
    .unwrap();

    // Start from a perturbed theta1 and the MLP's own default init (deliberately far
    // from `mlp_true_params`).
    let theta1_start = 0.3;
    let mlp_start_params =
        MLPRateFunc::new(&["B".to_string()], 3, false, Activation::Relu, true).get_params();
    let prob = hybrid_problem(theta1_start, &mlp_start_params);

    let opts = FitOptions {
        max_iters: 150,
        tolerance: 1e-8,
        method: FitMethod::Adam,
        step_size: 0.05,
        verbose: false,
        solver_method: methods::tsit5(),
        solver_options: solve_opts(),
        learn_rate: 0.0,
        grad_tol: 0.0,
    };

    let result = fit(&prob, &data, &mse_loss, &mse_loss_grad, &opts).expect("fit");

    assert!(
        result.final_loss < 0.5 * result.initial_loss,
        "hybrid fit: final loss {} should be well below initial {}",
        result.final_loss,
        result.initial_loss
    );
    // The mechanistic parameter is the well-identified one (t1's rate is the sole
    // driver of A's decay, observed directly) — it should land close to truth even
    // though the MLP weights (t2) are, in general, not all individually identifiable
    // from three trajectories.
    assert!(
        (result.params[0] - theta1_true).abs() < 0.15,
        "hybrid fit: theta1 {} not within tolerance of {theta1_true}",
        result.params[0]
    );
}

#[test]
fn fit_rejects_zero_params_problem() {
    // No rate function installed anywhere -> zero learnable parameters.
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
    let prob = LearnableProblem::new(net, u0, [0.0, 2.0]);
    let data = synthetic_dataset(THETA_TRUE);
    let opts = base_opts(FitMethod::NelderMead);

    let err = fit(&prob, &data, &mse_loss, &mse_loss_grad, &opts).unwrap_err();
    assert_eq!(err, pflow_learn::LearnError::ZeroParams);
}
