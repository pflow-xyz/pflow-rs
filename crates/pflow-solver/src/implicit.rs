//! Implicit ODE methods for stiff systems.

use pflow_core::State;

use crate::methods;
use crate::ode::{copy_state, solve, Options, Problem, Solution};

/// Backward Euler method for stiff ODEs.
///
/// Uses fixed-point iteration to solve the implicit equation.
pub fn implicit_euler(prob: &Problem, opts: &Options) -> Solution {
    let dt = opts.dt;
    let maxiters = opts.maxiters;
    let abstol = opts.abstol;

    let t0 = prob.tspan[0];
    let tf = prob.tspan[1];
    let f = &prob.f;
    let state_labels = &prob.state_labels;

    let mut t_out = vec![t0];
    let mut u_out = vec![copy_state(&prob.u0)];
    let mut tcur = t0;
    let mut ucur = copy_state(&prob.u0);
    let mut nsteps = 0usize;

    let max_fixed_point = 50;
    let fixed_point_tol = abstol * 10.0;

    while tcur < tf && nsteps < maxiters {
        let mut dtcur = dt;
        if tcur + dtcur > tf {
            dtcur = tf - tcur;
        }

        let tnext = tcur + dtcur;

        // Initial guess: explicit Euler
        let mut unext = copy_state(&ucur);
        let du = f(tcur, &ucur);
        for key in state_labels {
            if let (Some(un), Some(d)) = (unext.get_mut(key), du.get(key)) {
                *un += dtcur * d;
            }
        }

        // Fixed-point iteration
        for _ in 0..max_fixed_point {
            let mut unew = copy_state(&ucur);
            let dunext = f(tnext, &unext);
            for key in state_labels {
                if let (Some(un), Some(d)) = (unew.get_mut(key), dunext.get(key)) {
                    *un += dtcur * d;
                }
            }

            let mut max_diff = 0.0f64;
            for key in state_labels {
                let diff = (unew.get(key).unwrap_or(&0.0) - unext.get(key).unwrap_or(&0.0)).abs();
                max_diff = max_diff.max(diff);
            }

            unext = unew;
            if max_diff < fixed_point_tol {
                break;
            }
        }

        tcur = tnext;
        ucur = unext;
        t_out.push(tcur);
        u_out.push(copy_state(&ucur));
        nsteps += 1;
    }

    Solution {
        t: t_out,
        u: u_out,
        state_labels: state_labels.clone(),
    }
}

/// Detects whether the problem is stiff using a heuristic.
pub fn detect_stiffness(prob: &Problem) -> bool {
    let du = (prob.f)(prob.tspan[0], &prob.u0);

    let mut max_du = 0.0f64;
    let mut min_du = f64::MAX;

    for v in du.values() {
        let abs_v = v.abs();
        if abs_v > 1e-10 {
            max_du = max_du.max(abs_v);
            min_du = min_du.min(abs_v);
        }
    }

    if min_du < 1e-10 || max_du < 1e-10 {
        return false;
    }

    max_du / min_du > 1000.0
}

/// Chooses between explicit and implicit methods based on stiffness detection.
pub fn solve_implicit(prob: &Problem, opts: &Options) -> Solution {
    if detect_stiffness(prob) {
        let implicit_opts = Options {
            adaptive: false,
            ..opts.clone()
        };
        implicit_euler(prob, &implicit_opts)
    } else {
        solve(prob, &methods::tsit5(), opts)
    }
}

/// TR-BDF2 method: two-stage implicit method combining trapezoidal rule with BDF2.
pub fn trbdf2(prob: &Problem, opts: &Options) -> Solution {
    let dt = opts.dt;
    let maxiters = opts.maxiters;
    let abstol = opts.abstol;

    let t0 = prob.tspan[0];
    let tf = prob.tspan[1];
    let f = &prob.f;
    let state_labels = &prob.state_labels;

    let mut t_out = vec![t0];
    let mut u_out = vec![copy_state(&prob.u0)];
    let mut tcur = t0;
    let mut ucur = copy_state(&prob.u0);
    let mut nsteps = 0usize;

    let gamma = 2.0 - f64::sqrt(2.0);
    let max_fixed_point = 50;
    let fixed_point_tol = abstol * 10.0;

    while tcur < tf && nsteps < maxiters {
        let mut dtcur = dt;
        if tcur + dtcur > tf {
            dtcur = tf - tcur;
        }

        // Stage 1: Trapezoidal rule from t to t + gamma*dt
        let tgamma = tcur + gamma * dtcur;
        let mut ugamma = copy_state(&ucur);
        let du0 = f(tcur, &ucur);

        for key in state_labels {
            if let (Some(ug), Some(d)) = (ugamma.get_mut(key), du0.get(key)) {
                *ug += gamma * dtcur * d;
            }
        }

        for _ in 0..max_fixed_point {
            let dugamma = f(tgamma, &ugamma);
            let mut unew = copy_state(&ucur);
            for key in state_labels {
                if let (Some(un), Some(d0), Some(dg)) =
                    (unew.get_mut(key), du0.get(key), dugamma.get(key))
                {
                    *un += 0.5 * gamma * dtcur * (d0 + dg);
                }
            }

            let mut max_diff = 0.0f64;
            for key in state_labels {
                let diff = (unew.get(key).unwrap_or(&0.0) - ugamma.get(key).unwrap_or(&0.0)).abs();
                max_diff = max_diff.max(diff);
            }

            ugamma = unew;
            if max_diff < fixed_point_tol {
                break;
            }
        }

        // Stage 2: BDF2-like step
        let tnext = tcur + dtcur;
        let mut unext = copy_state(&ugamma);

        let dugamma = f(tgamma, &ugamma);
        for key in state_labels {
            if let (Some(un), Some(dg)) = (unext.get_mut(key), dugamma.get(key)) {
                *un += (1.0 - gamma) * dtcur * dg;
            }
        }

        let w1 = 1.0 / (gamma * (2.0 - gamma));
        let w0 = -((1.0 - gamma) * (1.0 - gamma)) / (gamma * (2.0 - gamma));
        let wf = (1.0 - gamma) / (2.0 - gamma);

        for _ in 0..max_fixed_point {
            let dunext = f(tnext, &unext);
            let mut unew: State = State::new();
            for key in state_labels {
                let ug = ugamma.get(key).copied().unwrap_or(0.0);
                let uc = ucur.get(key).copied().unwrap_or(0.0);
                let dn = dunext.get(key).copied().unwrap_or(0.0);
                unew.insert(key.clone(), w1 * ug + w0 * uc + wf * dtcur * dn);
            }

            let mut max_diff = 0.0f64;
            for key in state_labels {
                let diff = (unew.get(key).unwrap_or(&0.0) - unext.get(key).unwrap_or(&0.0)).abs();
                max_diff = max_diff.max(diff);
            }

            unext = unew;
            if max_diff < fixed_point_tol {
                break;
            }
        }

        tcur = tnext;
        ucur = unext;
        t_out.push(tcur);
        u_out.push(copy_state(&ucur));
        nsteps += 1;
    }

    Solution {
        t: t_out,
        u: u_out,
        state_labels: state_labels.clone(),
    }
}
