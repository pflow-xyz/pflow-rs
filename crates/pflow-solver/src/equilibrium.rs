//! Equilibrium detection for ODE solutions.

use pflow_core::State;

use crate::methods;
use crate::ode::{copy_state, Options, Problem, Solution};

/// Configuration for equilibrium detection.
#[derive(Debug, Clone)]
pub struct EquilibriumOptions {
    pub tolerance: f64,
    pub consecutive_steps: usize,
    pub min_time: f64,
    pub check_interval: usize,
}

impl EquilibriumOptions {
    pub fn default_opts() -> Self {
        Self {
            tolerance: 1e-6,
            consecutive_steps: 5,
            min_time: 0.1,
            check_interval: 10,
        }
    }

    pub fn fast() -> Self {
        Self {
            tolerance: 1e-4,
            consecutive_steps: 3,
            min_time: 0.01,
            check_interval: 5,
        }
    }

    pub fn strict() -> Self {
        Self {
            tolerance: 1e-9,
            consecutive_steps: 10,
            min_time: 1.0,
            check_interval: 1,
        }
    }
}

/// Result of equilibrium detection.
#[derive(Debug, Clone)]
pub struct EquilibriumResult {
    pub reached: bool,
    pub time: f64,
    pub state: State,
    pub max_change: f64,
    pub steps: usize,
    pub reason: String,
}

/// Integrates until equilibrium or time span exhausted.
pub fn solve_until_equilibrium(
    prob: &Problem,
    solver: &methods::Solver,
    opts: &Options,
    eq_opts: &EquilibriumOptions,
) -> (Solution, EquilibriumResult) {
    let dt = opts.dt;
    let dtmin = opts.dtmin;
    let dtmax = opts.dtmax;
    let abstol = opts.abstol;
    let reltol = opts.reltol;
    let maxiters = opts.maxiters;
    let adaptive = opts.adaptive;

    let t0 = prob.tspan[0];
    let tf = prob.tspan[1];
    let f = &prob.f;
    let state_labels = &prob.state_labels;

    let mut t_out = vec![t0];
    let mut u_out = vec![copy_state(&prob.u0)];
    let mut tcur = t0;
    let mut ucur = copy_state(&prob.u0);
    let mut dtcur = dt;
    let mut nsteps = 0usize;
    let mut consecutive_small = 0usize;
    let mut check_counter = 0usize;

    let mut eq_result = EquilibriumResult {
        reached: false,
        time: 0.0,
        state: State::new(),
        max_change: 0.0,
        steps: 0,
        reason: "time_exhausted".into(),
    };

    while tcur < tf && nsteps < maxiters {
        if tcur + dtcur > tf {
            dtcur = tf - tcur;
        }

        // Compute RK stages
        let num_stages = solver.c.len();
        let mut k: Vec<State> = Vec::with_capacity(num_stages);
        k.push(f(tcur, &ucur));

        for stage in 1..num_stages {
            let tstage = tcur + solver.c[stage] * dtcur;
            let mut ustage = copy_state(&ucur);
            for key in state_labels {
                for j in 0..stage {
                    let aj = if stage < solver.a.len() && j < solver.a[stage].len() {
                        solver.a[stage][j]
                    } else {
                        0.0
                    };
                    if let (Some(us), Some(kj)) = (ustage.get_mut(key), k[j].get(key)) {
                        *us += dtcur * aj * kj;
                    }
                }
            }
            k.push(f(tstage, &ustage));
        }

        let mut unext = copy_state(&ucur);
        for key in state_labels {
            for j in 0..solver.b.len() {
                if let (Some(un), Some(kj)) = (unext.get_mut(key), k[j].get(key)) {
                    *un += dtcur * solver.b[j] * kj;
                }
            }
        }

        let mut err = 0.0;
        if adaptive {
            for key in state_labels {
                let mut errest = 0.0;
                for j in 0..solver.b_hat.len() {
                    if let Some(kj) = k[j].get(key) {
                        errest += dtcur * solver.b_hat[j] * kj;
                    }
                }
                let uc = ucur.get(key).copied().unwrap_or(0.0);
                let un = unext.get(key).copied().unwrap_or(0.0);
                let mut scale = abstol + reltol * uc.abs().max(un.abs());
                if scale == 0.0 {
                    scale = abstol;
                }
                let val = errest.abs() / scale;
                if val > err {
                    err = val;
                }
            }
        }

        if !adaptive || err <= 1.0 || dtcur <= dtmin {
            tcur += dtcur;
            ucur = unext;
            t_out.push(tcur);
            u_out.push(copy_state(&ucur));
            nsteps += 1;

            // Check for equilibrium
            check_counter += 1;
            if tcur >= t0 + eq_opts.min_time
                && (eq_opts.check_interval == 0 || check_counter >= eq_opts.check_interval)
            {
                check_counter = 0;
                let max_change = compute_max_change(&k[0]);

                if max_change < eq_opts.tolerance {
                    consecutive_small += 1;
                    if consecutive_small >= eq_opts.consecutive_steps {
                        eq_result.reached = true;
                        eq_result.time = tcur;
                        eq_result.state = copy_state(&ucur);
                        eq_result.max_change = max_change;
                        eq_result.steps = nsteps;
                        eq_result.reason = "equilibrium_reached".into();
                        break;
                    }
                } else {
                    consecutive_small = 0;
                }
            }

            if adaptive && err > 0.0 {
                let factor = 0.9 * (1.0 / err).powf(1.0 / (solver.order as f64 + 1.0));
                let factor = factor.min(5.0);
                dtcur = dtmax.min(dtmin.max(dtcur * factor));
            }
        } else {
            let factor = 0.9 * (1.0 / err).powf(1.0 / (solver.order as f64 + 1.0));
            let factor = factor.max(0.1);
            dtcur = dtmin.max(dtcur * factor);
        }
    }

    if nsteps >= maxiters {
        eq_result.reason = "max_iterations".into();
    }

    eq_result.steps = nsteps;
    if !eq_result.reached {
        eq_result.time = tcur;
        eq_result.state = copy_state(&ucur);
        if !u_out.is_empty() {
            let du = f(tcur, &ucur);
            eq_result.max_change = compute_max_change(&du);
        }
    }

    let sol = Solution {
        t: t_out,
        u: u_out,
        state_labels: state_labels.clone(),
    };

    (sol, eq_result)
}

fn compute_max_change(du: &State) -> f64 {
    du.values().map(|v| v.abs()).fold(0.0f64, f64::max)
}

/// Checks if a state is at equilibrium for the given problem.
pub fn is_equilibrium(prob: &Problem, state: &State, tolerance: f64) -> bool {
    let du = (prob.f)(0.0, state);
    compute_max_change(&du) < tolerance
}

/// Solves until equilibrium and returns just the final state.
pub fn find_equilibrium(prob: &Problem) -> (State, bool) {
    let (_, result) = solve_until_equilibrium(
        prob,
        &methods::tsit5(),
        &Options::default_opts(),
        &EquilibriumOptions::default_opts(),
    );
    (result.state, result.reached)
}

/// Fast equilibrium detection with aggressive settings.
pub fn find_equilibrium_fast(prob: &Problem) -> (State, bool) {
    let (sol, result) = solve_until_equilibrium(
        prob,
        &methods::tsit5(),
        &Options::fast(),
        &EquilibriumOptions::fast(),
    );
    if result.reached {
        (result.state, true)
    } else {
        (
            sol.get_final_state().cloned().unwrap_or_default(),
            false,
        )
    }
}

/// Strict equilibrium detection with high confidence.
pub fn find_equilibrium_accurate(prob: &Problem) -> (State, bool) {
    let (_, result) = solve_until_equilibrium(
        prob,
        &methods::tsit5(),
        &Options::accurate(),
        &EquilibriumOptions::strict(),
    );
    (result.state, result.reached)
}

/// Combines solver and equilibrium options for specific use cases.
#[derive(Debug, Clone)]
pub struct OptionPair {
    pub solver: Options,
    pub equilibrium: EquilibriumOptions,
}

impl OptionPair {
    /// Game AI options: fast evaluation with loose equilibrium detection.
    pub fn game_ai() -> Self {
        Self {
            solver: Options::game_ai(),
            equilibrium: EquilibriumOptions {
                tolerance: 1e-3,
                consecutive_steps: 2,
                min_time: 0.01,
                check_interval: 3,
            },
        }
    }

    /// Epidemic modeling options.
    pub fn epidemic() -> Self {
        Self {
            solver: Options::epidemic(),
            equilibrium: EquilibriumOptions::default_opts(),
        }
    }

    /// Workflow/process simulation options.
    pub fn workflow() -> Self {
        Self {
            solver: Options::workflow(),
            equilibrium: EquilibriumOptions {
                tolerance: 1e-4,
                consecutive_steps: 3,
                min_time: 0.5,
                check_interval: 5,
            },
        }
    }

    /// Extended equilibrium analysis options.
    pub fn long_run() -> Self {
        Self {
            solver: Options::long_run(),
            equilibrium: EquilibriumOptions::strict(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::PetriNet;

    #[test]
    fn test_sir_equilibrium() {
        let (net, rates) = PetriNet::build().sir(999.0, 1.0, 0.0).with_rates(1.0);

        let state = net.set_state(None);
        let prob = Problem::new(net, state, [0.0, 100.0], rates);
        let (final_state, reached) = find_equilibrium(&prob);

        assert!(reached, "SIR should reach equilibrium");

        // Conservation: S + I + R = 1000
        let total = final_state["S"] + final_state["I"] + final_state["R"];
        assert!(
            (total - 1000.0).abs() < 1.0,
            "Total should be conserved: got {}",
            total
        );

        // At equilibrium, I should be near 0
        assert!(
            final_state["I"] < 1.0,
            "I should be near 0 at equilibrium: got {}",
            final_state["I"]
        );

        // R should have most of the population
        assert!(
            final_state["R"] > 900.0,
            "R should be >900 at equilibrium: got {}",
            final_state["R"]
        );
    }

    #[test]
    fn test_is_equilibrium() {
        let (net, rates) = PetriNet::build().sir(999.0, 1.0, 0.0).with_rates(1.0);

        let state = net.set_state(None);
        let prob = Problem::new(net, state, [0.0, 100.0], rates);

        // Initial state should NOT be at equilibrium
        assert!(!is_equilibrium(&prob, &prob.u0, 1e-6));

        // Find equilibrium and verify
        let (eq_state, reached) = find_equilibrium(&prob);
        if reached {
            assert!(is_equilibrium(&prob, &eq_state, 1e-4));
        }
    }
}
