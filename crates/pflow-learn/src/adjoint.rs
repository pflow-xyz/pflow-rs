//! Adjoint (reverse-mode) sensitivities: the gradient of a trajectory loss in one
//! backward solve, however many parameters the rates carry.
//!
//! Forward mode ([`crate::sensitivity::solve_with_sensitivities`]) integrates `n*P`
//! extra states — the right tool for a small net with a handful of rates. A
//! many-parameter rate (`MLPRateFunc`, dozens of learnable weights) makes that
//! augmentation the whole cost; the adjoint integrates one extra system of width
//! `n + P` instead, backward through the same pre-indexed mass-action RHS forward mode
//! uses, accumulating `dLoss/dtheta` as it goes.
//!
//! Ported from go-pflow's `learn/adjoint.go` (branch `tsit5-error-estimate`).
//! **Correction to an earlier note here:** pflow-xyz's `petri-learn.js` *does* carry an
//! adjoint port (`solveAdjoint`/`mseLossAdjoint`, added 2026-08-26,
//! `public/petri-learn.js`), and `parity/learn/goldens.json`'s
//! `point1Adjoint`/`point2Adjoint` fields are Go-adjoint fixtures the JS side already
//! replays — so there IS a cross-language (Go/JS) golden for this module, copied into
//! `../parity/goldens.json` and replayed against this crate by
//! `../tests/parity_goldens.rs` (see that file's module doc for the exact-vs-tolerance
//! rule it applies). `tests/adjoint.rs`'s own check — [`solve_adjoint`]'s gradient
//! against [`crate::lossgrad::mse_loss_grad`] on the same problem — remains a second,
//! complementary gate (two independently-derived methods computing the same quantity),
//! not a substitute for the cross-language one.
//!
//! This is the continuous adjoint: `lambda_dot = -J^T(x(t))*lambda` between observation
//! times, a jump `lambda += dl/dx` at each observed point, and
//! `G = integral(lambda^T * df/dtheta) dt` read off alongside. Unlike go-pflow's design
//! note about needing a reversed integrator, this crate's `integrate_vec` stays
//! forward-only by construction (see its doc comment in `pflow-solver`): each backward
//! segment `[lo, hi]` is solved as an ordinary *forward* integration in
//! `tau = hi - t` (so `tau` runs `0..hi-lo` while `t` runs `hi..lo`), exactly the
//! reformulation go-pflow's own `integrateSeg` performs via `solver.NewVectorProblem`
//! with `tspan = [0, hi-lo]` and `t := hi - tau` inside the RHS closure — this module
//! follows that shape directly rather than adding a generic reversal wrapper.
//!
//! The backward pass evaluates the Jacobian on the piecewise-linear reconstruction of
//! the stored forward trajectory (the same linear interpolation the losses use), and the
//! jumps do not differentiate through the interpolation weights — so gradient error
//! tracks the density of the forward grid. The clamp conventions are the forward mode's,
//! including the deliberate one-sided derivative at flux == 0.

use std::collections::HashMap;

use pflow_solver::methods::Solver;
use pflow_solver::ode::{integrate_vec, Options, Solution};

use crate::dataset::Dataset;
use crate::error::LearnError;
use crate::gradrate::rate_grad;
use crate::problem::LearnableProblem;
use crate::sensitivity::{build_rhs_index, RhsIndex};

/// Evaluates one observed sample — the loss contribution of a single
/// `(place, observation-time)` pair and its derivative with respect to the simulated
/// value. [`solve_adjoint`] sums contributions raw: any normalization (`1/numPoints` for
/// MSE, per-place scaling) belongs inside the closure.
pub type PointLossGrad<'a> = dyn Fn(&str, f64, f64) -> (f64, f64) + 'a;

/// The outcome of one reverse-mode gradient evaluation.
pub struct AdjointResult {
    /// The forward trajectory (a plain solve) — feed straight into the plain losses to
    /// cross-check `loss`.
    pub sol: Solution,
    /// Sum of pointwise loss terms.
    pub loss: f64,
    /// `dLoss/dtheta`, in [`LearnableProblem::get_all_params`]'s packing.
    pub grad: Vec<f64>,
    pub param_index: HashMap<String, (usize, usize)>,
    pub num_params: usize,
    /// Accepted steps summed over every backward segment.
    pub backward_steps: usize,
}

/// Finds the bracketing index for linear interpolation on a sorted, ascending time
/// grid: `t` at or before the first node reads the first value, `t` at or past the last
/// reads the last. Returns `(k, alpha)` such that
/// `x(t) = X[k]*(1-alpha) + X[k+1]*alpha`, with `k+1` valid whenever `alpha > 0`.
///
/// Mirrors go-pflow's `locateT` exactly (binary search via `partition_point`, the Rust
/// analog of `sort.SearchFloat64s`).
fn locate_t(times: &[f64], t: f64) -> (usize, f64) {
    let last = times.len().saturating_sub(1);
    if last == 0 || t <= times[0] {
        return (0, 0.0);
    }
    if t >= times[last] {
        return (last, 0.0);
    }
    // First index with times[k] > t; the bracket is [k-1, k].
    let mut k = times.partition_point(|&x| x < t);
    if times[k] == t {
        return (k, 0.0);
    }
    k -= 1;
    let dt = times[k + 1] - times[k];
    if dt == 0.0 {
        return (k, 0.0);
    }
    (k, (t - times[k]) / dt)
}

/// Interpolates the dense trajectory `x_traj` (rows aligned with `t_traj`) at `t`, into
/// `out` (length `x_traj[0].len()`).
fn interpolate_row(t_traj: &[f64], x_traj: &[Vec<f64>], t: f64, out: &mut [f64]) {
    let (k, alpha) = locate_t(t_traj, t);
    if alpha > 0.0 {
        for i in 0..out.len() {
            out[i] = x_traj[k][i] * (1.0 - alpha) + x_traj[k + 1][i] * alpha;
        }
    } else {
        out.copy_from_slice(&x_traj[k]);
    }
}

/// Integrates one backward segment `t in [lo, hi]` (via the forward-in-`tau` substitution
/// `tau = hi - t`) and updates `lam`/`g` in place with the result at `t = lo`. Returns the
/// number of accepted steps the segment took.
///
/// A no-op (`hi <= lo`, e.g. a jump landing exactly at `t0`) is not an error — it simply
/// contributes no gradient, the reverse-mode image of forward mode's `S(0) = 0`.
#[allow(clippy::too_many_arguments)]
fn integrate_segment(
    idx: &RhsIndex,
    t_traj: &[f64],
    x_traj: &[Vec<f64>],
    state_index: &HashMap<String, usize>,
    lo: f64,
    hi: f64,
    lam: &mut [f64],
    g: &mut [f64],
    solver: &Solver,
    opts: &Options,
) -> Result<usize, LearnError> {
    if !(hi > lo) {
        return Ok(0);
    }

    let n = idx.n();
    let p = idx.num_params();
    let m = n + p;

    let mut y0 = vec![0.0; m];
    y0[..n].copy_from_slice(lam);
    y0[n..].copy_from_slice(g);

    // Backward state y = [lambda(n); G(P)] in reversed time tau = hi - t:
    // dlambda/dtau = +J^T*lambda, dG/dtau = +lambda^T . df/dtheta.
    let rhs = |tau: f64, y: &[f64]| -> Vec<f64> {
        let mut dy = vec![0.0; m];
        let t = hi - tau;

        let mut xbuf = vec![0.0; n];
        interpolate_row(t_traj, x_traj, t, &mut xbuf);
        let state = idx.vec_to_state(&xbuf);

        for tr in &idx.transitions {
            // Input clamp: the transition contributes nothing — flux 0, subgradient 0,
            // exactly as forward mode.
            let clamped = tr.inputs.iter().any(|&(i, _)| xbuf[i] <= 0.0);
            if clamped {
                continue;
            }

            let (k_rate, dk_dtheta, dk_dstate) = rate_grad(&tr.rate_fn, &state, t);

            // g = product over inputs; pp[q] = product over entries != q, via
            // prefix/suffix (no division; every factor > 0 here).
            let xs: Vec<f64> = tr.inputs.iter().map(|&(i, _)| xbuf[i]).collect();
            let nin = xs.len();
            let mut prefix = vec![1.0; nin + 1];
            for i in 0..nin {
                prefix[i + 1] = prefix[i] * xs[i];
            }
            let mut suffix = vec![1.0; nin + 1];
            for i in (0..nin).rev() {
                suffix[i] = suffix[i + 1] * xs[i];
            }
            let g_val = prefix[nin];

            // Flux clamp: negative flux is the genuine ReLU-off region — skip. Flux
            // exactly 0 with all inputs > 0 KEEPS its derivative terms (the one-sided
            // right derivative as k -> 0+), so forward and reverse mode agree there.
            let flux = k_rate * g_val;
            if flux < 0.0 {
                continue;
            }

            // a = lambda^T . (stoichiometry column of this transition): inputs
            // subtract (their weight leaves the place), outputs add.
            let mut a = 0.0;
            for &(idxi, w) in &tr.inputs {
                a -= w * y[idxi];
            }
            for &(idxi, w) in &tr.outputs {
                a += w * y[idxi];
            }

            // b[j] = d(flux)/d(x_j), sparse over inputs union the rate function's own
            // state-dependencies — exactly as forward mode builds it.
            let mut b: HashMap<usize, f64> = HashMap::with_capacity(nin + dk_dstate.len());
            for i in 0..nin {
                let dg_dxi = prefix[i] * suffix[i + 1];
                *b.entry(tr.inputs[i].0).or_insert(0.0) += k_rate * dg_dxi;
            }
            for (place, &d) in &dk_dstate {
                if let Some(&si) = state_index.get(place) {
                    *b.entry(si).or_insert(0.0) += g_val * d;
                }
            }
            for (&j, &bv) in &b {
                dy[j] += a * bv;
            }
            for pi in tr.param_range.0..tr.param_range.1 {
                dy[n + pi] += a * g_val * dk_dtheta[pi - tr.param_range.0];
            }
        }

        dy
    };

    let (t_seg, y_seg) = integrate_vec(&rhs, &y0, [0.0, hi - lo], solver, opts);

    // integrate_vec runs `while tcur < tf && nsteps < maxiters` with no separate
    // truncation flag (unlike go-pflow's Solution.Truncated) — a maxiters cutoff shows
    // up as the last accepted time falling short of the segment's target span. Treat
    // that the same way go-pflow treats a truncated backward solve: a hard error,
    // because the gradient it would report is silently wrong.
    let target = hi - lo;
    let last_t = *t_seg
        .last()
        .expect("integrate_vec always returns at least t0");
    if last_t + 1e-9 < target {
        return Err(LearnError::AdjointTruncated);
    }

    let final_y = y_seg
        .last()
        .expect("integrate_vec always returns at least the initial condition");
    lam.copy_from_slice(&final_y[..n]);
    g.copy_from_slice(&final_y[n..]);

    Ok(t_seg.len().saturating_sub(1))
}

/// Computes a trajectory loss and its full parameter gradient in one forward solve plus
/// one backward (adjoint) solve — the cost no longer scales with the parameter count.
///
/// `pl` of `None` selects squared-error terms matching [`crate::lossgrad::mse_loss`]
/// exactly (each observation contributes `diff^2/N` with
/// `N = data.times.len() * data.places.len()`, so `loss` equals `mse_loss` of the same
/// trajectory). Observation times beyond `prob.tspan` clamp: a point past `tf` jumps at
/// the start of the backward pass, a point at or before `t0` contributes its loss but no
/// gradient (the costate has nowhere left to propagate — the reverse-mode image of
/// `S(0) = 0`).
///
/// Errors mirror [`crate::sensitivity::solve_with_sensitivities`]: a net place missing
/// from `u0` ([`LearnError::PlaceMissingFromU0`]), or a problem with no learnable
/// parameters ([`LearnError::ZeroParams`]). A truncated backward segment is
/// [`LearnError::AdjointTruncated`] — never silently returned, since the gradient it
/// would report is wrong.
pub fn solve_adjoint(
    prob: &LearnableProblem,
    data: &Dataset,
    pl: Option<&PointLossGrad>,
    solver: &Solver,
    opts: &Options,
) -> Result<AdjointResult, LearnError> {
    let idx = build_rhs_index(prob)?;
    let n = idx.n();
    let p = idx.num_params();
    if p == 0 {
        return Err(LearnError::ZeroParams);
    }

    let u0 = prob.dense_u0()?;
    let state_index = prob.state_index();

    // Forward: a plain solve. The stored dense trajectory is all the backward pass
    // reads (no second, augmented forward integration).
    let f = |t: f64, u: &[f64]| -> Vec<f64> {
        let mut du = vec![0.0; n];
        let state = idx.vec_to_state(u);
        for tr in &idx.transitions {
            if !tr.inputs.iter().all(|&(i, _)| u[i] > 0.0) {
                continue;
            }
            let k = tr.rate_fn.borrow().eval(&state, t);
            let g: f64 = tr.inputs.iter().map(|&(i, _)| u[i]).product();
            let flux = k * g;
            if flux < 0.0 {
                continue;
            }
            for &(i, w) in &tr.inputs {
                du[i] -= flux * w;
            }
            for &(i, w) in &tr.outputs {
                du[i] += flux * w;
            }
        }
        du
    };
    let (t_out, u_out) = integrate_vec(&f, &u0, prob.tspan, solver, opts);

    let sol = Solution {
        t: t_out.clone(),
        u: u_out.iter().map(|v| idx.vec_to_state(v)).collect(),
        state_labels: idx.labels.clone(),
    };

    let (t0, tf) = (prob.tspan[0], prob.tspan[1]);

    let n_norm = (data.times.len() * data.places.len()).max(1) as f64;
    let eval_pl = |place: &str, sim: f64, obs: f64| -> (f64, f64) {
        if let Some(f) = pl {
            f(place, sim, obs)
        } else {
            let diff = sim - obs;
            (diff * diff / n_norm, 2.0 * diff / n_norm)
        }
    };

    // Jump pass: evaluate every observed point once, accumulating the loss and the
    // per-row costate jumps keyed by clamped observation time. No extra solve happens
    // here — `loss` equals the pointwise sum exactly.
    let mut total_loss = 0.0;
    let mut jump_times: Vec<f64> = Vec::new();
    let mut jump_deltas: Vec<HashMap<usize, f64>> = Vec::new();

    for place in &data.places {
        let row = match state_index.get(place) {
            Some(&i) => i,
            // No color model in pflow-rs (unlike go-pflow's colorMap.Lookup expansion):
            // a place not present in the state simply contributes nothing, same effect
            // as an expansion that resolves to no rows.
            None => continue,
        };
        let obs_values = &data.observations[place];
        for (j, &t_obs) in data.times.iter().enumerate() {
            let (k, alpha) = locate_t(&t_out, t_obs);
            let mut sim = u_out[k][row];
            if alpha > 0.0 {
                sim = sim * (1.0 - alpha) + u_out[k + 1][row] * alpha;
            }
            let (l, d) = eval_pl(place, sim, obs_values[j]);
            total_loss += l;

            let tc = t_obs.clamp(t0, tf);
            let pos = match jump_times.iter().position(|&x| x.to_bits() == tc.to_bits()) {
                Some(pos) => pos,
                None => {
                    jump_times.push(tc);
                    jump_deltas.push(HashMap::new());
                    jump_times.len() - 1
                }
            };
            *jump_deltas[pos].entry(row).or_insert(0.0) += d;
        }
    }

    // Walk jump times from tf down, integrating the segment above each jump before
    // applying it. A jump exactly at t0 lands after the last segment and therefore
    // contributes no gradient.
    let mut order: Vec<usize> = (0..jump_times.len()).collect();
    order.sort_by(|&a, &b| jump_times[b].partial_cmp(&jump_times[a]).unwrap());

    let mut lam = vec![0.0; n];
    let mut g = vec![0.0; p];
    let mut back_steps = 0usize;
    let mut t_hi = tf;

    for &oi in &order {
        let jt = jump_times[oi];
        back_steps += integrate_segment(
            &idx,
            &t_out,
            &u_out,
            state_index,
            jt,
            t_hi,
            &mut lam,
            &mut g,
            solver,
            opts,
        )?;
        if jt < t_hi {
            t_hi = jt;
        }
        for (&row, &d) in &jump_deltas[oi] {
            lam[row] += d;
        }
    }
    back_steps += integrate_segment(
        &idx,
        &t_out,
        &u_out,
        state_index,
        t0,
        t_hi,
        &mut lam,
        &mut g,
        solver,
        opts,
    )?;

    Ok(AdjointResult {
        sol,
        loss: total_loss,
        grad: g,
        param_index: idx.param_index.clone(),
        num_params: p,
        backward_steps: back_steps,
    })
}

/// The reverse-mode counterpart of [`crate::lossgrad::mse_loss_grad`]: the same MSE
/// objective (`loss` equals `mse_loss` of the same trajectory), with the gradient from
/// one backward solve instead of `n*P` forward sensitivity states.
pub fn mse_loss_adjoint(
    prob: &LearnableProblem,
    data: &Dataset,
    solver: &Solver,
    opts: &Options,
) -> Result<AdjointResult, LearnError> {
    solve_adjoint(prob, data, None, solver, opts)
}

/// The reverse-mode counterpart of [`crate::lossgrad::relative_mse_loss_grad`]: per-place
/// `1/meanObs^2` weighting (`meanObs == 0` falls back to `1.0`), identical loss value,
/// gradient from one backward solve.
///
/// There is deliberately no RMSE adjoint (matching go-pflow): `sqrt` after the sum does
/// not decompose into pointwise terms, and minimizing MSE minimizes RMSE — fit on MSE
/// and report the root if a root is wanted.
pub fn relative_mse_loss_adjoint(
    prob: &LearnableProblem,
    data: &Dataset,
    solver: &Solver,
    opts: &Options,
) -> Result<AdjointResult, LearnError> {
    let mut means: HashMap<String, f64> = HashMap::with_capacity(data.places.len());
    for place in &data.places {
        let obs = data
            .observations
            .get(place)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let mean = if obs.is_empty() {
            0.0
        } else {
            obs.iter().sum::<f64>() / obs.len() as f64
        };
        means.insert(place.clone(), if mean == 0.0 { 1.0 } else { mean });
    }
    let n_norm = (data.times.len() * data.places.len()).max(1) as f64;
    let pl = move |place: &str, sim: f64, obs: f64| -> (f64, f64) {
        let mo = means[place];
        let diff = (sim - obs) / mo;
        (diff * diff / n_norm, 2.0 * diff / (mo * n_norm))
    };
    solve_adjoint(prob, data, Some(&pl), solver, opts)
}
