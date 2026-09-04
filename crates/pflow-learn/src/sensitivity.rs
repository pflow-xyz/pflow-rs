//! Forward sensitivities: integrate state `x` and `S = dx/dtheta` as one augmented ODE
//! through the same adaptive stepper `pflow-solver` already uses for the plain solve.
//!
//! This mirrors go-pflow's `learn/sensitivity.go` and pflow-xyz's `petri-learn.js`: one
//! bigger ODE, not `1 + num_params` separate solves.

use std::collections::HashMap;

use pflow_core::State;
use pflow_solver::methods::Solver;
use pflow_solver::ode::{integrate_vec, Options, Solution};

use crate::error::LearnError;
use crate::gradrate::{rate_grad, SharedRateFunc};
use crate::problem::LearnableProblem;

/// One transition's pre-indexed structure: which state indices it consumes/produces
/// (by dense state index, in arc-entry order — a place can appear more than once if the
/// net has more than one arc between it and the transition) and where its own
/// parameters sit in the flat parameter vector.
pub(crate) struct TransitionIndex {
    pub rate_fn: SharedRateFunc,
    /// `[start, end)` into the flat parameter vector — this transition's own params.
    pub param_range: (usize, usize),
    pub inputs: Vec<(usize, f64)>,
    pub outputs: Vec<(usize, f64)>,
}

/// Pre-indexed RHS structure, shared (in spirit — the adjoint module isn't ported yet)
/// between the plain/sensitivity solve and any future adjoint solve, so the two modes
/// cannot drift on how flux and stoichiometry are assembled.
pub(crate) struct RhsIndex {
    pub labels: Vec<String>,
    pub params: Vec<f64>,
    // Kept for future reuse (e.g. an adjoint solve) even though this narrower cut of
    // the crate doesn't read it back yet — see the module doc.
    #[allow(dead_code)]
    pub param_index: HashMap<String, (usize, usize)>,
    pub transitions: Vec<TransitionIndex>,
}

impl RhsIndex {
    pub(crate) fn vec_to_state(&self, u: &[f64]) -> State {
        self.labels
            .iter()
            .zip(u.iter())
            .map(|(l, &x)| (l.clone(), x))
            .collect()
    }

    pub(crate) fn n(&self) -> usize {
        self.labels.len()
    }

    pub(crate) fn num_params(&self) -> usize {
        self.params.len()
    }
}

/// Builds the pre-indexed RHS structure for `prob`.
///
/// Errors: any net place referenced by an arc that has no entry in `prob`'s dense state
/// vector (i.e. missing from `u0`) — silently defaulting would drop an arc rather than
/// clamp it off, two different dynamical systems. Also errors if the problem has zero
/// learnable parameters across every installed rate function.
pub(crate) fn build_rhs_index(prob: &LearnableProblem) -> Result<RhsIndex, LearnError> {
    let labels = prob.state_labels().to_vec();
    let state_index = prob.state_index();

    // Validate every arc endpoint that names a place is present in the dense state.
    for arc in &prob.net.arcs {
        if prob.net.places.contains_key(&arc.source) && !state_index.contains_key(&arc.source) {
            return Err(LearnError::PlaceMissingFromU0(arc.source.clone()));
        }
        if prob.net.places.contains_key(&arc.target) && !state_index.contains_key(&arc.target) {
            return Err(LearnError::PlaceMissingFromU0(arc.target.clone()));
        }
    }

    let (params, param_index) = prob.get_all_params();

    let transitions: Vec<TransitionIndex> = prob
        .rate_funcs()
        .iter()
        .filter_map(|(name, rf)| {
            let range = *param_index.get(name)?;
            let inputs: Vec<(usize, f64)> = prob
                .net
                .input_arcs(name)
                .iter()
                .filter_map(|a| state_index.get(&a.source).map(|&idx| (idx, a.weight_sum())))
                .collect();
            let outputs: Vec<(usize, f64)> = prob
                .net
                .output_arcs(name)
                .iter()
                .filter_map(|a| state_index.get(&a.target).map(|&idx| (idx, a.weight_sum())))
                .collect();
            Some(TransitionIndex {
                rate_fn: rf.clone(),
                param_range: range,
                inputs,
                outputs,
            })
        })
        .collect();

    Ok(RhsIndex {
        labels,
        params,
        param_index,
        transitions,
    })
}

/// Forward sensitivities plus the underlying trajectory.
///
/// `s[k][i * num_params + p]` is `d x_i / d theta_p` at `t[k]`.
pub struct Sensitivities {
    pub t: Vec<f64>,
    pub s: Vec<Vec<f64>>,
    pub state_labels: Vec<String>,
    pub param_index: HashMap<String, (usize, usize)>,
    pub num_params: usize,
    pub sol: Solution,
}

impl Sensitivities {
    /// `d state[place] / d params[param]` at time index `k`.
    pub fn at(&self, k: usize, place: &str, param: usize) -> Option<f64> {
        let i = self.state_labels.iter().position(|l| l == place)?;
        self.s.get(k)?.get(i * self.num_params + param).copied()
    }
}

/// Solves `prob`'s ODE together with its forward sensitivities `S = dx/dtheta`, as one
/// augmented ODE of length `n * (1 + num_params)` through `solver`.
pub fn solve_with_sensitivities(
    prob: &LearnableProblem,
    solver: &Solver,
    opts: &Options,
) -> Result<Sensitivities, LearnError> {
    let index = build_rhs_index(prob)?;
    let n = index.n();
    let p = index.num_params();
    if p == 0 {
        return Err(LearnError::ZeroParams);
    }

    let x0 = prob.dense_u0()?;
    let mut y0 = vec![0.0; n * (1 + p)];
    y0[..n].copy_from_slice(&x0);
    // S(0) = 0 — the sensitivity block is already zero-initialized above.

    let f = move |t: f64, y: &[f64]| -> Vec<f64> {
        let x = &y[..n];
        let s_block = &y[n..];
        let state = index.vec_to_state(x);

        let mut dy = vec![0.0; n * (1 + p)];

        for tr in &index.transitions {
            // Input clamp: any input <= 0 => flux 0, all derivative terms 0.
            let inputs_ok = tr.inputs.iter().all(|&(idx, _)| x[idx] > 0.0);
            if !inputs_ok {
                continue;
            }

            let (k, dk_dparams_local, dk_dstate) = rate_grad(&tr.rate_fn, &state, t);

            // g = product of input concentrations; dg/dx via prefix/suffix products so
            // repeated arcs to the same place (d(x^2)/dx = 2x) are handled without division.
            let xs: Vec<f64> = tr.inputs.iter().map(|&(idx, _)| x[idx]).collect();
            let m = xs.len();
            let mut prefix = vec![1.0; m + 1];
            for i in 0..m {
                prefix[i + 1] = prefix[i] * xs[i];
            }
            let mut suffix = vec![1.0; m + 1];
            for i in (0..m).rev() {
                suffix[i] = suffix[i + 1] * xs[i];
            }
            let g = prefix[m]; // product of all inputs (1.0 if none)

            let raw_flux = k * g;
            if raw_flux < 0.0 {
                // Negative-flux clamp: flux 0, all derivative terms 0.
                continue;
            }

            // Base state equation.
            for &(idx, w) in &tr.inputs {
                dy[idx] -= raw_flux * w;
            }
            for &(idx, w) in &tr.outputs {
                dy[idx] += raw_flux * w;
            }

            // b[j] = d(flux)/d(x_j), combining the two routes: k * dg/dx_j (through the
            // inputs this transition itself reads) and g * dk/dx_j (through any place the
            // rate function itself depends on, e.g. a state-dependent LinearRateFunc).
            let mut b: HashMap<usize, f64> = HashMap::new();
            for i in 0..m {
                let dg_dxi = prefix[i] * suffix[i + 1];
                *b.entry(tr.inputs[i].0).or_insert(0.0) += k * dg_dxi;
            }
            for (place, &dk) in &dk_dstate {
                if let Some(&idx) = prob.state_index().get(place) {
                    *b.entry(idx).or_insert(0.0) += g * dk;
                }
            }

            for pi in 0..p {
                let mut c = 0.0;
                for (&j, &bj) in &b {
                    c += bj * s_block[j * p + pi];
                }
                if pi >= tr.param_range.0 && pi < tr.param_range.1 {
                    c += g * dk_dparams_local[pi - tr.param_range.0];
                }
                if c != 0.0 {
                    for &(idx, w) in &tr.inputs {
                        dy[n + idx * p + pi] -= w * c;
                    }
                    for &(idx, w) in &tr.outputs {
                        dy[n + idx * p + pi] += w * c;
                    }
                }
            }
        }

        dy
    };

    let (t_out, y_out) = integrate_vec(&f, &y0, prob.tspan, solver, opts);

    let state_labels = prob.state_labels().to_vec();
    let sol_u = y_out
        .iter()
        .map(|y| {
            state_labels
                .iter()
                .zip(y[..n].iter())
                .map(|(l, &v)| (l.clone(), v))
                .collect()
        })
        .collect();
    let s = y_out.iter().map(|y| y[n..].to_vec()).collect();

    let param_index_rebuilt = prob.get_all_params().1;
    Ok(Sensitivities {
        t: t_out.clone(),
        s,
        state_labels: state_labels.clone(),
        param_index: param_index_rebuilt,
        num_params: p,
        sol: Solution {
            t: t_out,
            u: sol_u,
            state_labels,
        },
    })
}
