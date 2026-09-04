//! Nelder-Mead simplex search and plain coordinate descent, over an arbitrary
//! `&[f64] -> f64` objective. Derivative-free — no gradient closure needed.
//!
//! Mirrors go-pflow's `learn/optimize.go`.

/// Options for [`nelder_mead`] and [`coordinate_descent`] (and their [`minimize`]
/// dispatcher). Scoped to this module for the same reason [`super::gradopt::GradOptSettings`]
/// is: `optim` stays usable with no [`crate::problem::LearnableProblem`] coupling.
#[derive(Debug, Clone, Copy)]
pub struct SimplexSettings {
    pub max_iters: usize,
    /// Converged when `worst - best < tolerance` (Nelder-Mead) or the best loss found
    /// drops below `tolerance` (coordinate descent).
    pub tolerance: f64,
    /// Initial per-coordinate step for coordinate descent.
    pub step_size: f64,
    pub verbose: bool,
}

impl Default for SimplexSettings {
    fn default() -> Self {
        Self {
            max_iters: 1000,
            tolerance: 1e-4,
            step_size: 0.01,
            verbose: false,
        }
    }
}

/// Method selector for [`minimize`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimplexMethod {
    NelderMead,
    CoordinateDescent,
}

/// Dispatches to [`nelder_mead`] or [`coordinate_descent`].
pub fn minimize(
    f: &dyn Fn(&[f64]) -> f64,
    x0: &[f64],
    method: SimplexMethod,
    settings: &SimplexSettings,
) -> super::FitResult {
    match method {
        SimplexMethod::NelderMead => nelder_mead(f, x0, settings),
        SimplexMethod::CoordinateDescent => coordinate_descent(f, x0, settings),
    }
}

/// Simple coordinate descent: try `+step`/`-step` on each coordinate in turn, keep
/// whichever improves, halve the step whenever a full sweep improves nothing.
/// Converges when the step shrinks below `1e-10` or the best loss drops below
/// `settings.tolerance`.
pub fn coordinate_descent(
    f: &dyn Fn(&[f64]) -> f64,
    x0: &[f64],
    settings: &SimplexSettings,
) -> super::FitResult {
    let mut x = x0.to_vec();
    let n = x.len();
    let mut evals = 0;

    let mut best_loss = f(&x);
    evals += 1;
    let initial_loss = best_loss;
    let mut step_size = settings.step_size;

    let mut iters = 0;
    let mut converged = false;

    for iter in 0..settings.max_iters {
        iters = iter;
        let mut improved = false;

        for i in 0..n {
            let old_val = x[i];

            x[i] = old_val + step_size;
            let pos_loss = f(&x);
            evals += 1;

            x[i] = old_val - step_size;
            let neg_loss = f(&x);
            evals += 1;

            if pos_loss < best_loss {
                x[i] = old_val + step_size;
                best_loss = pos_loss;
                improved = true;
            } else if neg_loss < best_loss {
                x[i] = old_val - step_size;
                best_loss = neg_loss;
                improved = true;
            } else {
                x[i] = old_val;
            }
        }

        if settings.verbose && iter % 100 == 0 {
            eprintln!("coordinate-descent iter {iter}: loss = {best_loss}");
        }

        if !improved {
            step_size *= 0.5;
            if step_size < 1e-10 {
                converged = true;
                break;
            }
        }

        if best_loss < settings.tolerance {
            converged = true;
            break;
        }
    }
    if !converged {
        // Mirrors go-pflow's `coordinateDescent`: an exhausted (non-converged) loop
        // reports `opts.MaxIters` itself, not the 0-indexed loop variable's last value
        // (`max_iters - 1`) — the two agree only on an early-converged return.
        iters = settings.max_iters;
    }

    super::FitResult {
        params: x,
        initial_loss,
        final_loss: best_loss,
        iters,
        evals,
        converged,
    }
}

/// Insertion sort of `simplex`/`values` by `values`, ascending — sufficient for the
/// small `n+1`-point simplices this function is ever called with.
fn sort_simplex(simplex: &mut [Vec<f64>], values: &mut [f64]) {
    let n = values.len();
    for i in 1..n {
        let val = values[i];
        let point = simplex[i].clone();
        let mut j = i;
        while j > 0 && values[j - 1] > val {
            values[j] = values[j - 1];
            simplex[j] = simplex[j - 1].clone();
            j -= 1;
        }
        values[j] = val;
        simplex[j] = point;
    }
}

/// Nelder-Mead simplex search. alpha=1.0 (reflect), gamma=2.0 (expand), rho=0.5
/// (contract), sigma=0.5 (shrink). Initial simplex perturbs each coordinate by
/// `0.05 * (1 + |x0[i]|)`. Converges when `values[n] - values[0] < settings.tolerance`.
pub fn nelder_mead(
    f: &dyn Fn(&[f64]) -> f64,
    x0: &[f64],
    settings: &SimplexSettings,
) -> super::FitResult {
    const ALPHA: f64 = 1.0;
    const GAMMA: f64 = 2.0;
    const RHO: f64 = 0.5;
    const SIGMA: f64 = 0.5;

    let n = x0.len();
    let mut evals = 0;

    let mut simplex: Vec<Vec<f64>> = Vec::with_capacity(n + 1);
    let mut values: Vec<f64> = Vec::with_capacity(n + 1);

    simplex.push(x0.to_vec());
    values.push(f(x0));
    evals += 1;
    let initial_loss = values[0];

    for i in 0..n {
        let mut p = x0.to_vec();
        p[i] += 0.05 * (1.0 + x0[i].abs());
        values.push(f(&p));
        evals += 1;
        simplex.push(p);
    }

    let mut iters = 0;
    let mut converged = false;

    for iter in 0..settings.max_iters {
        iters = iter;
        sort_simplex(&mut simplex, &mut values);

        if settings.verbose && iter % 100 == 0 {
            eprintln!(
                "nelder-mead iter {iter}: best = {}, worst = {}",
                values[0], values[n]
            );
        }

        if values[n] - values[0] < settings.tolerance {
            converged = true;
            break;
        }

        let mut centroid = vec![0.0; n];
        for i in 0..n {
            let mut sum = 0.0;
            for j in 0..n {
                sum += simplex[j][i];
            }
            centroid[i] = sum / n as f64;
        }

        let mut reflected = vec![0.0; n];
        for i in 0..n {
            reflected[i] = centroid[i] + ALPHA * (centroid[i] - simplex[n][i]);
        }
        let reflected_val = f(&reflected);
        evals += 1;

        if values[0] <= reflected_val && reflected_val < values[n - 1] {
            simplex[n] = reflected;
            values[n] = reflected_val;
            continue;
        }

        if reflected_val < values[0] {
            let mut expanded = vec![0.0; n];
            for i in 0..n {
                expanded[i] = centroid[i] + GAMMA * (reflected[i] - centroid[i]);
            }
            let expanded_val = f(&expanded);
            evals += 1;

            if expanded_val < reflected_val {
                simplex[n] = expanded;
                values[n] = expanded_val;
            } else {
                simplex[n] = reflected;
                values[n] = reflected_val;
            }
            continue;
        }

        let mut contracted = vec![0.0; n];
        if reflected_val < values[n] {
            for i in 0..n {
                contracted[i] = centroid[i] + RHO * (reflected[i] - centroid[i]);
            }
        } else {
            for i in 0..n {
                contracted[i] = centroid[i] + RHO * (simplex[n][i] - centroid[i]);
            }
        }
        let contracted_val = f(&contracted);
        evals += 1;

        if contracted_val < reflected_val.min(values[n]) {
            simplex[n] = contracted;
            values[n] = contracted_val;
            continue;
        }

        // Shrink toward the best point.
        for i in 1..=n {
            for j in 0..n {
                simplex[i][j] = simplex[0][j] + SIGMA * (simplex[i][j] - simplex[0][j]);
            }
            values[i] = f(&simplex[i]);
            evals += 1;
        }
    }

    sort_simplex(&mut simplex, &mut values);
    if !converged {
        // See the identical note in coordinate_descent above.
        iters = settings.max_iters;
    }

    super::FitResult {
        params: simplex[0].clone(),
        initial_loss,
        final_loss: values[0],
        iters,
        evals,
        converged,
    }
}
