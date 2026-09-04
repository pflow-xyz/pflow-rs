//! Gradient-check: forward sensitivities (`solve_with_sensitivities`) against central
//! finite differences of the plain solve, the same acceptance gate go-pflow's own
//! `learn/sensitivity_test.go` uses.

use std::cell::RefCell;
use std::rc::Rc;

use pflow_core::net::PetriNet;
use pflow_core::State;
use pflow_learn::{Activation, LearnableProblem, MLPRateFunc, RateFunc, ScalarRateFunc};
use pflow_solver::methods;
use pflow_solver::ode::Options;

/// A -> t1 -> B, single rate parameter. Analytically A(t) = A0 * exp(-theta t), so this
/// also serves as a sanity check against a closed form, not just against FD.
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

/// A + B -> t1 -> C -> t2 -> D, two independent rate parameters and a bimolecular
/// (two-input) transition, to exercise the prefix/suffix product-rule term.
fn bimolecular_problem(theta1: f64, theta2: f64) -> LearnableProblem {
    let net = PetriNet::build()
        .place("A", 5.0)
        .place("B", 3.0)
        .place("C", 0.0)
        .place("D", 0.0)
        .transition("t1")
        .transition("t2")
        .arc("A", "t1", 1.0)
        .arc("B", "t1", 1.0)
        .arc("t1", "C", 1.0)
        .arc("C", "t2", 1.0)
        .arc("t2", "D", 1.0)
        .done();
    let u0: State = [
        ("A".to_string(), 5.0),
        ("B".to_string(), 3.0),
        ("C".to_string(), 0.0),
        ("D".to_string(), 0.0),
    ]
    .into_iter()
    .collect();
    let mut prob = LearnableProblem::new(net, u0, [0.0, 1.0]);
    prob.set_rate_func("t1", Rc::new(RefCell::new(ScalarRateFunc::new(theta1))));
    prob.set_rate_func("t2", Rc::new(RefCell::new(ScalarRateFunc::new(theta2))));
    prob
}

// Tolerance chosen to balance runtime against FD-check precision: the augmented
// sensitivity ODE has n*(1+P) components and the adaptive controller's abstol/reltol
// apply uniformly across all of them, so pushing this much tighter than what the FD
// comparison actually needs (relative ~1e-3, per the crate's own coverage-gap note)
// buys nothing but a much longer run.
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

/// Central-difference the final dense state w.r.t. one parameter, by rebuilding the
/// problem at theta +/- h and reading back the final state at `tf` (always an accepted
/// output point: the stepper clamps its last step so it never overshoots `tspan[1]`).
fn fd_dstate_dtheta(
    build: impl Fn(f64) -> LearnableProblem,
    theta: f64,
    param_idx: usize,
    labels: &[String],
) -> Vec<f64> {
    let h = 1e-6 * (1.0 + theta.abs());

    let plus = build(theta);
    let (mut params, _) = plus.get_all_params();
    params[param_idx] += h;
    plus.set_all_params(&params);
    let sol_plus = plus.solve(&methods::tsit5(), &solve_opts()).unwrap();
    let final_plus = sol_plus.get_final_state().unwrap().clone();

    let minus = build(theta);
    let (mut params, _) = minus.get_all_params();
    params[param_idx] -= h;
    minus.set_all_params(&params);
    let sol_minus = minus.solve(&methods::tsit5(), &solve_opts()).unwrap();
    let final_minus = sol_minus.get_final_state().unwrap().clone();

    labels
        .iter()
        .map(|l| {
            let vp = final_plus.get(l).copied().unwrap_or(0.0);
            let vm = final_minus.get(l).copied().unwrap_or(0.0);
            (vp - vm) / (2.0 * h)
        })
        .collect()
}

fn assert_close(name: &str, analytic: f64, fd: f64, atol: f64, rtol: f64) {
    let scale = atol + rtol * analytic.abs().max(fd.abs());
    let diff = (analytic - fd).abs();
    assert!(
        diff <= scale,
        "{name}: analytic sensitivity {analytic} vs FD {fd} (|diff|={diff} > tol={scale})"
    );
}

#[test]
fn decay_sensitivity_matches_finite_difference() {
    let theta = 0.7;
    let prob = decay_problem(theta);
    let sens = pflow_learn::solve_with_sensitivities(&prob, &methods::tsit5(), &solve_opts())
        .expect("sensitivity solve");

    let last = sens.t.len() - 1;
    let labels = prob.state_labels().to_vec();
    let fd = fd_dstate_dtheta(decay_problem, theta, 0, &labels);

    for (i, label) in labels.iter().enumerate() {
        let analytic = sens.at(last, label, 0).unwrap();
        assert_close(label, analytic, fd[i], 1e-6, 1e-4);
    }

    // Closed-form check: dA/dtheta = -t * A(t) for A(t) = A0 exp(-theta t).
    let tf = sens.t[last];
    let a_final = sens.sol.get_final_state().unwrap()["A"];
    let expected_da_dtheta = -tf * a_final;
    let analytic_da_dtheta = sens.at(last, "A", 0).unwrap();
    assert_close(
        "A (closed form)",
        analytic_da_dtheta,
        expected_da_dtheta,
        1e-6,
        1e-4,
    );
}

#[test]
fn bimolecular_two_param_sensitivity_matches_finite_difference() {
    let theta1 = 0.4;
    let theta2 = 0.9;
    let prob = bimolecular_problem(theta1, theta2);
    let sens = pflow_learn::solve_with_sensitivities(&prob, &methods::tsit5(), &solve_opts())
        .expect("sensitivity solve");

    let last = sens.t.len() - 1;
    let labels = prob.state_labels().to_vec();

    for param_idx in 0..2 {
        let fd = fd_dstate_dtheta(
            |t| {
                if param_idx == 0 {
                    bimolecular_problem(t, theta2)
                } else {
                    bimolecular_problem(theta1, t)
                }
            },
            if param_idx == 0 { theta1 } else { theta2 },
            param_idx,
            &labels,
        );

        for (i, label) in labels.iter().enumerate() {
            let analytic = sens.at(last, label, param_idx).unwrap();
            assert_close(
                &format!("{label}/theta{param_idx}"),
                analytic,
                fd[i],
                1e-5,
                1e-3,
            );
        }
    }
}

/// A -> t1 -> B where t1's rate is `MLPRateFunc` (state-dependent, hidden layer 3, ReLU),
/// composed with the forward-sensitivity augmented ODE exactly the way `ScalarRateFunc`
/// is above — this exercises `eval_grad`'s analytic MLP backprop as the `dk/dstate` and
/// `dk/dparams` terms feeding `sensitivity.rs`'s `b`/`c` accumulation, not just `eval_grad`
/// in isolation (that's covered directly in `src/ratefunc.rs`'s own unit tests).
fn mlp_decay_problem(params: &[f64]) -> LearnableProblem {
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
    let mut prob = LearnableProblem::new(net, u0, [0.0, 1.0]);
    let mut rf = MLPRateFunc::new(&["A".to_string()], 3, false, Activation::Relu, true);
    rf.set_params(params);
    prob.set_rate_func("t1", Rc::new(RefCell::new(rf)));
    prob
}

/// Like `fd_dstate_dtheta`, but perturbs one entry of a full flat parameter vector
/// (the MLP case, vs. `fd_dstate_dtheta`'s single scalar `theta`).
fn fd_dstate_dparam_vec(
    build: impl Fn(&[f64]) -> LearnableProblem,
    params0: &[f64],
    param_idx: usize,
    labels: &[String],
) -> Vec<f64> {
    let h = 1e-6 * (1.0 + params0[param_idx].abs());

    let mut p_plus = params0.to_vec();
    p_plus[param_idx] += h;
    let plus = build(&p_plus);
    let sol_plus = plus.solve(&methods::tsit5(), &solve_opts()).unwrap();
    let final_plus = sol_plus.get_final_state().unwrap().clone();

    let mut p_minus = params0.to_vec();
    p_minus[param_idx] -= h;
    let minus = build(&p_minus);
    let sol_minus = minus.solve(&methods::tsit5(), &solve_opts()).unwrap();
    let final_minus = sol_minus.get_final_state().unwrap().clone();

    labels
        .iter()
        .map(|l| {
            let vp = final_plus.get(l).copied().unwrap_or(0.0);
            let vm = final_minus.get(l).copied().unwrap_or(0.0);
            (vp - vm) / (2.0 * h)
        })
        .collect()
}

#[test]
fn mlp_rate_sensitivity_matches_finite_difference() {
    let params0 =
        MLPRateFunc::new(&["A".to_string()], 3, false, Activation::Relu, true).get_params();

    let prob = mlp_decay_problem(&params0);
    let sens = pflow_learn::solve_with_sensitivities(&prob, &methods::tsit5(), &solve_opts())
        .expect("sensitivity solve");

    let last = sens.t.len() - 1;
    let labels = prob.state_labels().to_vec();

    // Check every MLP parameter's sensitivity column against FD, not just the first.
    for param_idx in 0..params0.len() {
        let fd = fd_dstate_dparam_vec(mlp_decay_problem, &params0, param_idx, &labels);
        for (i, label) in labels.iter().enumerate() {
            let analytic = sens.at(last, label, param_idx).unwrap();
            assert_close(
                &format!("{label}/mlp_param{param_idx}"),
                analytic,
                fd[i],
                1e-5,
                1e-3,
            );
        }
    }
}

#[test]
fn zero_params_is_an_error() {
    use std::cell::RefCell;
    use std::rc::Rc;

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
    let mut prob = LearnableProblem::new(net, u0, [0.0, 1.0]);
    prob.set_rate_func(
        "t1",
        Rc::new(RefCell::new(pflow_learn::ConstantRateFunc::new(1.0))),
    );

    match pflow_learn::solve_with_sensitivities(&prob, &methods::tsit5(), &Options::default_opts())
    {
        Err(e) => assert_eq!(e, pflow_learn::LearnError::ZeroParams),
        Ok(_) => panic!("expected ZeroParams error"),
    }
}
