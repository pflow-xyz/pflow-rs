//! ODE solver core: Problem, Solution, Options, Solve, mass-action kinetics.

use std::collections::HashMap;

use pflow_core::net::PetriNet;
use pflow_core::State;

use crate::methods::Solver;

/// A function that computes the derivative du/dt given time t and state u.
pub type ODEFunc = Box<dyn Fn(f64, &State) -> State>;

// Internal vectorized ODE function: (t, u) -> du using dense arrays.
type VecF = Box<dyn Fn(f64, &[f64]) -> Vec<f64>>;

/// An ODE initial value problem for a Petri net.
pub struct Problem {
    pub net: PetriNet,
    pub u0: State,
    pub tspan: [f64; 2],
    pub rates: HashMap<String, f64>,
    pub f: ODEFunc,
    pub state_labels: Vec<String>,
    // Vectorized internals for fast solve()
    #[allow(dead_code)]
    state_index: HashMap<String, usize>,
    vec_u0: Vec<f64>,
    vec_f: VecF,
}

impl Problem {
    /// Creates a new ODE problem from a Petri net.
    pub fn new(
        net: PetriNet,
        initial_state: State,
        tspan: [f64; 2],
        rates: HashMap<String, f64>,
    ) -> Self {
        let f = build_ode_function(&net, &rates);
        let state_labels: Vec<String> = initial_state.keys().cloned().collect();
        let state_index: HashMap<String, usize> = state_labels
            .iter()
            .enumerate()
            .map(|(i, label)| (label.clone(), i))
            .collect();
        let vec_u0: Vec<f64> = state_labels
            .iter()
            .map(|label| initial_state.get(label).copied().unwrap_or(0.0))
            .collect();
        let n_places = state_labels.len();
        let vec_f = build_vec_ode_function(&net, &rates, &state_index, n_places);
        Self {
            net,
            u0: initial_state,
            tspan,
            rates,
            f,
            state_labels,
            state_index,
            vec_u0,
            vec_f,
        }
    }
}

/// Constructs the ODE derivative function for a Petri net using mass-action kinetics.
fn build_ode_function(net: &PetriNet, rates: &HashMap<String, f64>) -> ODEFunc {
    // Pre-compute structure for the closure
    let place_labels: Vec<String> = net.places.keys().cloned().collect();
    let trans_labels: Vec<String> = net.transitions.keys().cloned().collect();
    let arcs: Vec<(String, String, f64)> = net
        .arcs
        .iter()
        .map(|a| (a.source.clone(), a.target.clone(), a.weight_sum()))
        .collect();
    let place_set: std::collections::HashSet<String> = net.places.keys().cloned().collect();
    let rates = rates.clone();

    Box::new(move |_t: f64, u: &State| -> State {
        let mut du: State = place_labels.iter().map(|l| (l.clone(), 0.0)).collect();

        for trans_label in &trans_labels {
            let rate = rates.get(trans_label).copied().unwrap_or(1.0);
            let mut flux = rate;

            // Compute flux using mass-action kinetics
            for (source, target, _weight) in &arcs {
                if target == trans_label && place_set.contains(source) {
                    let place_state = u.get(source).copied().unwrap_or(0.0);
                    if place_state <= 0.0 {
                        flux = 0.0;
                        break;
                    }
                    flux *= place_state;
                }
            }

            // Apply flux to connected places
            if flux > 0.0 {
                for (source, target, weight) in &arcs {
                    if target == trans_label && place_set.contains(source) {
                        // Input arc: consume tokens
                        if let Some(v) = du.get_mut(source) {
                            *v -= flux * weight;
                        }
                    } else if source == trans_label && place_set.contains(target) {
                        // Output arc: produce tokens
                        if let Some(v) = du.get_mut(target) {
                            *v += flux * weight;
                        }
                    }
                }
            }
        }
        du
    })
}

/// Constructs a vectorized ODE derivative function with pre-indexed arcs.
///
/// This replaces HashMap lookups with array indexing and pre-groups arcs
/// by transition, reducing per-call cost from O(T*A) to O(A).
fn build_vec_ode_function(
    net: &PetriNet,
    rates: &HashMap<String, f64>,
    state_index: &HashMap<String, usize>,
    n_places: usize,
) -> VecF {
    // Pre-group arcs by transition: O(A) construction
    let mut input_map: HashMap<&str, Vec<(usize, f64)>> = HashMap::new();
    let mut output_map: HashMap<&str, Vec<(usize, f64)>> = HashMap::new();

    for arc in &net.arcs {
        let w = arc.weight_sum();
        if net.transitions.contains_key(&arc.target) {
            if let Some(&idx) = state_index.get(&arc.source) {
                input_map
                    .entry(arc.target.as_str())
                    .or_default()
                    .push((idx, w));
            }
        }
        if net.transitions.contains_key(&arc.source) {
            if let Some(&idx) = state_index.get(&arc.target) {
                output_map
                    .entry(arc.source.as_str())
                    .or_default()
                    .push((idx, w));
            }
        }
    }

    // Build compact transition table: (rate, inputs, outputs)
    let transitions: Vec<(f64, Vec<(usize, f64)>, Vec<(usize, f64)>)> = net
        .transitions
        .keys()
        .map(|label| {
            let rate = rates.get(label).copied().unwrap_or(1.0);
            let inputs = input_map.remove(label.as_str()).unwrap_or_default();
            let outputs = output_map.remove(label.as_str()).unwrap_or_default();
            (rate, inputs, outputs)
        })
        .collect();

    Box::new(move |_t: f64, u: &[f64]| -> Vec<f64> {
        let mut du = vec![0.0; n_places];

        for (rate, inputs, outputs) in &transitions {
            let mut flux = *rate;

            // Mass-action kinetics: flux = rate * product(input tokens)
            for &(idx, _w) in inputs {
                let v = u[idx];
                if v <= 0.0 {
                    flux = 0.0;
                    break;
                }
                flux *= v;
            }

            if flux > 0.0 {
                for &(idx, w) in inputs {
                    du[idx] -= flux * w;
                }
                for &(idx, w) in outputs {
                    du[idx] += flux * w;
                }
            }
        }

        du
    })
}

/// The solution to an ODE problem.
pub struct Solution {
    pub t: Vec<f64>,
    pub u: Vec<State>,
    pub state_labels: Vec<String>,
}

impl Solution {
    /// Extracts the time series for a specific state variable by label.
    pub fn get_variable(&self, label: &str) -> Vec<f64> {
        self.u
            .iter()
            .map(|s| s.get(label).copied().unwrap_or(0.0))
            .collect()
    }

    /// Returns the final state of the system.
    pub fn get_final_state(&self) -> Option<&State> {
        self.u.last()
    }

    /// Returns the state at a specific time point index.
    pub fn get_state(&self, i: usize) -> Option<&State> {
        self.u.get(i)
    }
}

/// Solver configuration parameters.
#[derive(Debug, Clone)]
pub struct Options {
    pub dt: f64,
    pub dtmin: f64,
    pub dtmax: f64,
    pub abstol: f64,
    pub reltol: f64,
    pub maxiters: usize,
    pub adaptive: bool,
}

impl Options {
    /// Default solver options — balanced for most problems.
    pub fn default_opts() -> Self {
        Self {
            dt: 0.01,
            dtmin: 1e-6,
            dtmax: 0.1,
            abstol: 1e-6,
            reltol: 1e-3,
            maxiters: 100_000,
            adaptive: true,
        }
    }

    /// Options that match the pflow.xyz JavaScript solver.
    pub fn js_parity() -> Self {
        Self {
            dt: 0.01,
            dtmin: 1e-6,
            dtmax: 1.0,
            abstol: 1e-6,
            reltol: 1e-3,
            maxiters: 100_000,
            adaptive: true,
        }
    }

    /// Fast options: speed over accuracy (~10x faster).
    pub fn fast() -> Self {
        Self {
            dt: 0.1,
            dtmin: 1e-4,
            dtmax: 1.0,
            abstol: 1e-2,
            reltol: 1e-2,
            maxiters: 1_000,
            adaptive: true,
        }
    }

    /// Accurate options: high precision.
    pub fn accurate() -> Self {
        Self {
            dt: 0.001,
            dtmin: 1e-8,
            dtmax: 0.1,
            abstol: 1e-9,
            reltol: 1e-6,
            maxiters: 1_000_000,
            adaptive: true,
        }
    }

    /// Options for stiff ODE systems.
    pub fn stiff() -> Self {
        Self {
            dt: 0.001,
            dtmin: 1e-10,
            dtmax: 0.01,
            abstol: 1e-8,
            reltol: 1e-5,
            maxiters: 500_000,
            adaptive: true,
        }
    }

    /// Game AI options: fast move evaluation.
    pub fn game_ai() -> Self {
        Self {
            dt: 0.1,
            dtmin: 1e-3,
            dtmax: 1.0,
            abstol: 1e-2,
            reltol: 1e-2,
            maxiters: 500,
            adaptive: true,
        }
    }

    /// Epidemic/population modeling options.
    pub fn epidemic() -> Self {
        Self {
            dt: 0.01,
            dtmin: 1e-6,
            dtmax: 0.5,
            abstol: 1e-6,
            reltol: 1e-4,
            maxiters: 200_000,
            adaptive: true,
        }
    }

    /// Workflow/process simulation options.
    pub fn workflow() -> Self {
        Self {
            dt: 0.1,
            dtmin: 1e-4,
            dtmax: 10.0,
            abstol: 1e-4,
            reltol: 1e-3,
            maxiters: 50_000,
            adaptive: true,
        }
    }

    /// Long-run simulation options.
    pub fn long_run() -> Self {
        Self {
            dt: 0.1,
            dtmin: 1e-4,
            dtmax: 10.0,
            abstol: 1e-5,
            reltol: 1e-3,
            maxiters: 500_000,
            adaptive: true,
        }
    }
}

/// Copies a state map.
pub fn copy_state(s: &State) -> State {
    s.clone()
}

/// Converts a dense vector back to a labeled State map.
fn vec_to_state(v: &[f64], labels: &[String]) -> State {
    labels
        .iter()
        .enumerate()
        .map(|(i, label)| (label.clone(), v[i]))
        .collect()
}

/// Integrates the ODE problem using the given solver and options.
///
/// Internally uses vectorized (dense array) state representation for performance.
pub fn solve(prob: &Problem, solver: &Solver, opts: &Options) -> Solution {
    let dt = opts.dt;
    let dtmin = opts.dtmin;
    let dtmax = opts.dtmax;
    let abstol = opts.abstol;
    let reltol = opts.reltol;
    let maxiters = opts.maxiters;
    let adaptive = opts.adaptive;

    let t0 = prob.tspan[0];
    let tf = prob.tspan[1];
    let f = &prob.vec_f;
    let n = prob.vec_u0.len();

    let mut t_out = vec![t0];
    let mut u_out: Vec<Vec<f64>> = vec![prob.vec_u0.clone()];
    let mut tcur = t0;
    let mut ucur = prob.vec_u0.clone();
    let mut dtcur = dt;
    let mut nsteps = 0usize;

    while tcur < tf && nsteps < maxiters {
        // Don't overshoot
        if tcur + dtcur > tf {
            dtcur = tf - tcur;
        }

        // Compute Runge-Kutta stages
        let num_stages = solver.c.len();
        let mut k: Vec<Vec<f64>> = Vec::with_capacity(num_stages);
        k.push(f(tcur, &ucur));

        for stage in 1..num_stages {
            let tstage = tcur + solver.c[stage] * dtcur;
            let mut ustage = ucur.clone();
            for j in 0..stage {
                let aj = if stage < solver.a.len() && j < solver.a[stage].len() {
                    solver.a[stage][j]
                } else {
                    0.0
                };
                if aj != 0.0 {
                    let scale = dtcur * aj;
                    for i in 0..n {
                        ustage[i] += scale * k[j][i];
                    }
                }
            }
            k.push(f(tstage, &ustage));
        }

        // Compute solution at next step
        let mut unext = ucur.clone();
        for j in 0..solver.b.len() {
            if solver.b[j] != 0.0 {
                let scale = dtcur * solver.b[j];
                for i in 0..n {
                    unext[i] += scale * k[j][i];
                }
            }
        }

        // Compute error estimate
        let mut err = 0.0;
        if adaptive {
            for i in 0..n {
                let mut errest = 0.0;
                for j in 0..solver.b_hat.len() {
                    errest += dtcur * solver.b_hat[j] * k[j][i];
                }
                let uc = ucur[i];
                let un = unext[i];
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

        // Accept or reject step
        if !adaptive || err <= 1.0 || dtcur <= dtmin {
            tcur += dtcur;
            ucur = unext;
            t_out.push(tcur);
            u_out.push(ucur.clone());
            nsteps += 1;

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

    // Convert dense trajectory to State maps for backward compatibility
    let state_u: Vec<State> = u_out
        .iter()
        .map(|v| vec_to_state(v, &prob.state_labels))
        .collect();

    Solution {
        t: t_out,
        u: state_u,
        state_labels: prob.state_labels.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods;

    #[test]
    fn test_simple_decay() {
        // A -> t1 -> B, should transfer tokens from A to B
        let net = PetriNet::build()
            .place("A", 10.0)
            .place("B", 0.0)
            .transition("t1")
            .arc("A", "t1", 1.0)
            .arc("t1", "B", 1.0)
            .done();

        let state = net.set_state(None);
        let rates = net.set_rates(None);
        let prob = Problem::new(net, state, [0.0, 10.0], rates);
        let sol = solve(&prob, &methods::tsit5(), &Options::default_opts());

        let final_state = sol.get_final_state().unwrap();
        let total = final_state["A"] + final_state["B"];
        // Conservation: A + B should be approximately 10
        assert!((total - 10.0).abs() < 0.1);
    }

    /// With a 5th-order error estimate, tightening reltol by a factor of 100
    /// should cost roughly 100^(1/5) ~ 2.5x the steps. The old +1/66 weight
    /// made the estimate first order, so the same tightening cost ~100x.
    #[test]
    fn tsit5_step_count_scales_as_fifth_root_of_reltol() {
        let build = || {
            PetriNet::build()
                .place("S", 990.0)
                .place("I", 10.0)
                .place("R", 0.0)
                .transition("infect")
                .transition("recover")
                .arc("S", "infect", 1.0)
                .arc("I", "infect", 1.0)
                .arc("infect", "I", 2.0)
                .arc("I", "recover", 1.0)
                .arc("recover", "R", 1.0)
                .done()
        };
        let steps = |reltol: f64| {
            let net = build();
            let state = net.set_state(None);
            let mut rates = net.set_rates(None);
            rates.insert("infect".into(), 0.0005);
            rates.insert("recover".into(), 0.1);
            let prob = Problem::new(net, state, [0.0, 100.0], rates);
            let opts = Options {
                dt: 0.01,
                dtmin: 1e-10,
                dtmax: 100.0,
                abstol: 1e-12,
                reltol,
                maxiters: 1_000_000,
                adaptive: true,
            };
            solve(&prob, &methods::tsit5(), &opts).t.len() - 1
        };
        let coarse = steps(1e-3);
        let fine = steps(1e-5);
        let ratio = fine as f64 / coarse as f64;
        assert!(
            (1.5..8.0).contains(&ratio),
            "steps at 1e-3: {coarse}, at 1e-5: {fine}, ratio {ratio} (expected ~2.5)"
        );
    }
}
