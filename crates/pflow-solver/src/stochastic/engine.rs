//! The discrete engine: propensities, blocked-time bookkeeping, time-
//! weighted marking summaries, and Gillespie's direct method over a
//! [`super::compile::Compiled`] model. Ported from go-pflow's
//! `stochastic/stochastic.go` (`propensitiesAt`, `blockage`, `timeStats`,
//! `ssa`, `schedulePending`).

use super::compile::{CTransition, GuardEval};

/// Upper bound on SSA steps per realization, matching the portable SSA's
/// own cap.
pub const MAX_STEPS: usize = 1_000_000;

/// `C(m, w)`, the number of distinct ways to select `w` tokens from `m`.
/// Re-exported from the portable SSA module: the same arithmetic (multiply
/// then divide, once per factor) applies here, and this engine has no
/// byte-exactness contract of its own to protect by duplicating it.
pub use crate::ssa::combinations;

/// A delayed firing that has started and not yet completed. `at` is the
/// completion time on the run's own clock; between segments of a scheduled
/// run it is re-based to the next segment's start.
#[derive(Debug, Clone, Copy)]
pub struct Pending {
    pub at: f64,
    pub tr: usize,
}

/// Inserts `p` keeping the queue sorted by completion time, after any
/// completion already due at the same instant, so two things started
/// together finish in the order they were started.
fn schedule_pending(q: &mut Vec<Pending>, p: Pending) {
    let i = q.partition_point(|x| x.at <= p.at);
    q.insert(i, p);
}

/// Accumulates, across realizations, how long each place was the sole
/// unmet input of a transition, and which transitions those were.
#[derive(Debug, Clone)]
pub struct Blockage {
    pub waited: Vec<f64>,
    pub holding: Vec<std::collections::HashSet<String>>,
    candidates: Vec<usize>,
    seen: Vec<bool>,
}

impl Blockage {
    pub fn new(n_places: usize) -> Self {
        Self {
            waited: vec![0.0; n_places],
            holding: vec![Default::default(); n_places],
            candidates: Vec::new(),
            seen: vec![false; n_places],
        }
    }

    /// Folds another ledger in, for a run assembled from segments.
    pub fn merge(&mut self, other: &Blockage) {
        for i in 0..other.waited.len() {
            self.waited[i] += other.waited[i];
            for id in &other.holding[i] {
                self.holding[i].insert(id.clone());
            }
        }
    }

    fn note(&mut self, place: usize, id: &str) {
        self.holding[place].insert(id.to_string());
        if !self.seen[place] {
            self.seen[place] = true;
            self.candidates.push(place);
        }
    }

    fn credit(&mut self, dt: f64) {
        for &p in &self.candidates {
            self.waited[p] += dt;
            self.seen[p] = false;
        }
        self.candidates.clear();
    }
}

/// The time-weighted summary of a run: how long each place spent holding
/// each token count, accumulated across realizations. This is what
/// `Metrics::mean`/`p95` are derived from, in place of averaging the
/// reported sample points (biased toward the t=0 transient at a coarse
/// grid).
#[derive(Debug, Clone)]
pub struct TimeStats {
    total: f64,
    integral: Vec<f64>,
    dwell: Vec<Vec<f64>>,
    /// When set, `hold` folds the marking through this index map (expanded
    /// places -> report places) before crediting.
    pub fold_idx: Option<Vec<usize>>,
}

impl TimeStats {
    pub fn new(n_places: usize) -> Self {
        Self {
            total: 0.0,
            integral: vec![0.0; n_places],
            dwell: vec![Vec::new(); n_places],
            fold_idx: None,
        }
    }

    pub fn hold(&mut self, marking: &[i64], dt: f64) {
        if dt <= 0.0 {
            return;
        }
        let folded;
        let marking = if let Some(fold_idx) = &self.fold_idx {
            let mut scratch = vec![0i64; self.integral.len()];
            for (i, &v) in marking.iter().enumerate() {
                scratch[fold_idx[i]] += v;
            }
            folded = scratch;
            &folded[..]
        } else {
            marking
        };
        self.total += dt;
        for (i, &v) in marking.iter().enumerate() {
            self.integral[i] += v as f64 * dt;
            let d = &mut self.dwell[i];
            if v as usize >= d.len() {
                let grown_len = 2 * (v as usize + 1);
                d.resize(grown_len, 0.0);
            }
            d[v as usize] += dt;
        }
    }

    pub fn merge(&mut self, other: &TimeStats) {
        self.total += other.total;
        for i in 0..other.integral.len() {
            self.integral[i] += other.integral[i];
            if other.dwell[i].len() > self.dwell[i].len() {
                self.dwell[i].resize(other.dwell[i].len(), 0.0);
            }
            for (v, &w) in other.dwell[i].iter().enumerate() {
                self.dwell[i][v] += w;
            }
        }
    }

    pub fn mean(&self, place: usize) -> f64 {
        if self.total <= 0.0 {
            return 0.0;
        }
        self.integral[place] / self.total
    }

    /// The token count at or below which the place spent `q` of its time —
    /// the time-weighted analogue of a nearest-rank percentile.
    pub fn percentile(&self, place: usize, q: f64) -> f64 {
        if self.total <= 0.0 {
            return 0.0;
        }
        let target = q * self.total;
        let mut acc = 0.0;
        for (v, &w) in self.dwell[place].iter().enumerate() {
            acc += w;
            if acc >= target {
                return v as f64;
            }
        }
        0.0
    }
}

/// Fills `out` with the SSA propensity of every transition at `marking` and
/// returns their total, summed strictly left to right. When `blk` is set,
/// each blocked transition's sole short input is noted.
pub(crate) fn propensities_at(
    trs: &[CTransition],
    marking: &[i64],
    places: &[String],
    guard: Option<GuardEval>,
    out: &mut Vec<f64>,
    mut blk: Option<&mut Blockage>,
) -> f64 {
    out.clear();
    let mut total = 0.0;
    for t in trs {
        let mut a = t.rate;
        for input in &t.inputs {
            let m = marking[input.place];
            if m < input.weight {
                a = 0.0;
                break;
            }
            if input.kinetic {
                a *= combinations(m, input.weight);
            }
        }
        if a > 0.0 && (!t.gated(marking) || !t.allows(places, marking, guard)) {
            a = 0.0;
        }
        out.push(a);
        total += a;

        if a == 0.0 {
            if let Some(short) = t.sole_short_input(marking) {
                if t.gated(marking) && t.allows(places, marking, guard) {
                    if let Some(blk) = blk.as_deref_mut() {
                        blk.note(short, &t.id);
                    }
                }
            }
        }
    }
    total
}

/// One realization of Gillespie's direct method, run over `[times[0],
/// times[last]]`. Returns `traj[place][sample]` in `places` order and the
/// queue of delayed firings still pending at the horizon (re-based to the
/// next segment's clock).
#[allow(clippy::too_many_arguments)]
pub(crate) fn ssa(
    trs: &[CTransition],
    places: &[String],
    marking: &mut [i64],
    times: &[f64],
    rng: &mut crate::ssa::Xoshiro256,
    fired: &mut [i64],
    blk: &mut Blockage,
    ts: &mut TimeStats,
    guard: Option<GuardEval>,
    realization: usize,
    on_fire: Option<super::options::OnFireFn>,
    carried: &[Pending],
) -> (Vec<Vec<f64>>, Vec<Pending>) {
    let n_places = marking.len();
    let mut queue: Vec<Pending> = carried.to_vec();
    let timed = trs.iter().any(|t| t.delay > 0.0);

    let start = |t: f64, marking: &mut [i64], queue: &mut Vec<Pending>| {
        if !timed {
            return;
        }
        loop {
            let mut again = false;
            for (i, tr) in trs.iter().enumerate() {
                if tr.delay > 0.0 && tr.enabled(places, marking, guard) {
                    for a in &tr.inputs {
                        marking[a.place] -= a.weight;
                    }
                    schedule_pending(
                        queue,
                        Pending {
                            at: t + tr.delay,
                            tr: i,
                        },
                    );
                    again = true;
                }
            }
            if !again {
                break;
            }
        }
    };

    blk.credit(0.0); // a step cut short by MAX_STEPS leaves scratch behind
    let mut traj: Vec<Vec<f64>> = vec![vec![0.0; times.len()]; n_places];

    let mut t = 0.0f64;
    let mut next = 0usize;
    let record = |t: f64, marking: &[i64], next: &mut usize, traj: &mut Vec<Vec<f64>>| {
        while *next < times.len() && times[*next] <= t {
            for p in 0..n_places {
                traj[p][*next] = marking[p] as f64;
            }
            *next += 1;
        }
    };
    record(t, marking, &mut next, &mut traj);

    let t_end = *times.last().expect("times is non-empty");
    let mut propensities: Vec<f64> = Vec::with_capacity(trs.len());

    let mut step = 0usize;
    while step < MAX_STEPS && t < t_end {
        step += 1;
        start(t, marking, &mut queue);
        let total = propensities_at(
            trs,
            marking,
            places,
            guard,
            &mut propensities,
            Some(&mut *blk),
        );
        if total <= 0.0 && queue.is_empty() {
            // Dead marking: nothing can fire, and no amount of time changes
            // that.
            blk.credit(t_end - t);
            ts.hold(marking, t_end - t);
            break;
        }

        let mut dt = f64::INFINITY;
        if total > 0.0 {
            // Portable draw shape (matches `ssa::mod`'s `wait()`, and go-pflow's
            // `portableSampler.wait()`): `u = 1.0 - x1` is exact for every `x1` in
            // `[0, 1)` and never zero, so there is no clamp and no redraw. This used
            // to draw `-plog(u)` from the raw uniform with a `u <= 0.0` clamp instead
            // — go-pflow's non-portable `stdSampler.wait()` shape, paired here with a
            // portable RNG stream it was never meant to receive. That mismatch was the
            // root cause of the scheduled/staged SSA byte-parity gap against
            // go-pflow's portable-path goldens (see `tests/scheduled_parity.rs`).
            let u = 1.0 - rng.uniform();
            dt = -crate::ssa::plog(u) / total;
        }

        if !queue.is_empty() && queue[0].at <= t + dt {
            let due = queue[0];
            let held = (due.at - t).min(t_end - t);
            blk.credit(held);
            ts.hold(marking, held);
            t = due.at;
            record(t, marking, &mut next, &mut traj);
            if t > t_end {
                // Clamped only conceptually: the loop exits here and `t_end`
                // is what every use after the loop reads.
                break;
            }
            queue.remove(0);
            for a in &trs[due.tr].outputs {
                marking[a.place] += a.weight;
            }
            fired[due.tr] += 1;
            if let Some(f) = on_fire {
                f(realization, t, &trs[due.tr].id, marking);
            }
            continue;
        }

        let held = dt.min(t_end - t);
        blk.credit(held);
        ts.hold(marking, held);
        t += dt;
        record(t, marking, &mut next, &mut traj);
        if t > t_end {
            break;
        }

        let r = rng.uniform() * total;
        let mut chosen = trs.len() - 1;
        let mut acc = 0.0;
        for (i, &a) in propensities.iter().enumerate() {
            acc += a;
            if r <= acc {
                chosen = i;
                break;
            }
        }
        for a in &trs[chosen].inputs {
            marking[a.place] -= a.weight;
        }
        for a in &trs[chosen].outputs {
            marking[a.place] += a.weight;
        }
        fired[chosen] += 1;
        if let Some(f) = on_fire {
            f(realization, t, &trs[chosen].id, marking);
        }
    }

    // Hold the final marking through any remaining samples.
    while next < times.len() {
        for p in 0..n_places {
            traj[p][next] = marking[p] as f64;
        }
        next += 1;
    }
    // What is still in flight leaves on the next segment's clock.
    for p in &mut queue {
        p.at -= t_end;
    }
    (traj, queue)
}
