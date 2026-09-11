//! `Simulate`/`SimulateSchedule` and the reporting layer around the
//! discrete engine: stage folding, blocked-time contention, time-weighted
//! metrics and depletion. Ported from go-pflow's `stochastic/stochastic.go`
//! and `stochastic/schedule.go`.
//!
//! Unlike [`crate::ssa`] (the byte-exact portable Gillespie SSA held to
//! cross-language goldens), this engine has no byte-parity contract: it
//! reports the same *shape* of result go-pflow's default (non-portable)
//! path does — `Caveats`/`Assumptions` kept apart, time-weighted
//! `Metrics`, `Contended`, `Depleted` — using this crate's own portable
//! Xoshiro256 + `plog` generator for its randomness rather than
//! reproducing go-pflow's `math/rand`-backed default path number for
//! number, which go-pflow itself does not promise across runs of its own
//! two paths either.

use std::collections::HashMap;

use pflow_metamodel::{Model, RateSegment, StageExpansion};

use super::compile::{compile, token_places, StochasticError};
use super::engine::{ssa, Blockage, Pending, TimeStats};
use super::options::{sample_times, start_from, Options};
use super::result::{
    Contention, Depletion, Metrics, Series, SimResult, EXPONENTIAL_SERVICE_ASSUMPTION,
};
use super::supply::{classify_supply, SupplyKind};
use crate::ssa::Xoshiro256;

/// Cross-realization bookkeeping a run accumulates alongside its trajectory:
/// the time-weighted marking summary behind `Metrics`, and the blocked-time
/// ledger behind `Contended`. Both outlive a single segment so
/// `simulate_schedule` can merge them across the whole horizon rather than
/// averaging each segment's own (which would weight a short rush the same
/// as a long lull).
pub(crate) struct RunStats {
    pub times: TimeStats,
    pub blocked: Blockage,
    pub expansion: Option<StageExpansion>,
    /// Every realization's own final marking, in the expanded places' order.
    pub ends: Vec<Vec<i64>>,
}

/// `stochastic::Simulate`: runs Gillespie's SSA over the discrete marking.
/// Routes to [`simulate_schedule`] when the model declares stages/rate
/// schedules or the caller supplies one.
pub fn simulate(
    m: &Model,
    marking: &HashMap<String, i64>,
    opts: &Options,
) -> Result<SimResult, StochasticError> {
    if m.has_schedules() || !opts.schedule.is_empty() {
        return simulate_schedule(m, marking, opts);
    }
    let (mut res, stats, _left) = simulate_top(m, marking, opts)?;
    res.assumptions
        .extend(assumptions_for(stats.expansion.as_ref()));
    Ok(res)
}

/// Expands stage declarations, then runs [`simulate_from`] with no carried
/// state — the entry point every direct (unscheduled) caller uses.
fn simulate_top(
    m: &Model,
    marking: &HashMap<String, i64>,
    opts: &Options,
) -> Result<(SimResult, RunStats, Vec<Vec<Pending>>), StochasticError> {
    let (m2, exp) = m
        .expand_stages()
        .map_err(|e| StochasticError::Other(e.to_string()))?;
    simulate_from(m, &m2, exp.as_ref(), marking, None, None, opts)
}

/// `simulateFrom`: runs the expanded net `m2`, with an optional
/// per-realization start (`starts[r]`, expanded places' order) and an
/// optional carried delayed-firing queue per realization. Every
/// realization's final marking is returned in `RunStats::ends` either way.
#[allow(clippy::too_many_arguments)]
pub(crate) fn simulate_from(
    orig: &Model,
    m2: &Model,
    exp: Option<&StageExpansion>,
    marking: &HashMap<String, i64>,
    starts: Option<&[Vec<i64>]>,
    carry: Option<&[Vec<Pending>]>,
    opts: &Options,
) -> Result<(SimResult, RunStats, Vec<Vec<Pending>>), StochasticError> {
    let translated_rates = match exp {
        Some(e) => e.translate_rates(&opts.rates),
        None => opts.rates.clone(),
    };
    let mut opts2 = opts.clone();
    opts2.rates = translated_rates;
    let opts = opts2.with_defaults(m2);

    let compiled = compile(m2, &opts.rates, opts.guard)?;
    let trs = compiled.transitions;
    let places = compiled.places;
    let caveats = compiled.caveats;

    if let Some(c) = carry {
        if c.len() != opts.realizations {
            // Mirrors go-pflow's guard: a caller passing a mismatched carry
            // count is a bug in the scheduler, not a model defect.
            return Err(StochasticError::Other(format!(
                "{} carried realizations for a run of {}",
                c.len(),
                opts.realizations
            )));
        }
    }
    let mut left: Vec<Vec<Pending>> = vec![Vec::new(); opts.realizations];
    let mut inflight = vec![0.0f64; trs.len()];

    // The reporting vocabulary: every expanded place folds to itself except
    // stage places, which fold to their carrier.
    let mut report: Vec<String> = Vec::with_capacity(places.len());
    let mut report_idx: HashMap<&str, usize> = HashMap::new();
    for p in &places {
        if exp.map(|e| e.is_stage_place(p)).unwrap_or(false) {
            continue;
        }
        report_idx.insert(p.as_str(), report.len());
        report.push(p.clone());
    }
    let fold_idx: Vec<usize> = places
        .iter()
        .map(|p| {
            let carrier = exp.and_then(|e| e.carrier_of.get(p));
            let key = carrier.map(|c| c.as_str()).unwrap_or(p.as_str());
            report_idx[key]
        })
        .collect();

    let times = sample_times(&opts);
    let mut firings = vec![0.0f64; trs.len()];

    let mut sums: Vec<Vec<f64>> = vec![vec![0.0; times.len()]; report.len()];
    let mut sum_squares: Vec<Vec<f64>> = vec![vec![0.0; times.len()]; report.len()];

    let seed = if opts.seed == 0 { 1 } else { opts.seed };
    let initial = start_from(m2, marking);

    let mut acc = RunStats {
        times: TimeStats::new(report.len()),
        blocked: Blockage::new(places.len()),
        expansion: exp.cloned(),
        ends: vec![Vec::new(); opts.realizations],
    };
    if exp.is_some() {
        acc.times.fold_idx = Some(fold_idx.clone());
    }

    let mut folded = vec![0.0f64; times.len()];
    // `r` seeds the RNG, indexes `starts`/`carry`/`left`/`acc.ends` and is
    // passed to `on_fire`, so this is not a single-vec indexing loop an
    // iterator adapter would simplify.
    #[allow(clippy::needless_range_loop)]
    for r in 0..opts.realizations {
        let mut start: Vec<i64> = if let Some(s) = starts.and_then(|s| s.get(r)) {
            s.clone()
        } else {
            places
                .iter()
                .map(|p| *initial.get(p).unwrap_or(&0))
                .collect()
        };
        let mut counts = vec![0i64; trs.len()];
        let mut rng = Xoshiro256::new(seed.wrapping_add(r as u64));
        let carried: &[Pending] = carry
            .and_then(|c| c.get(r))
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        let (traj, rest) = ssa(
            &trs,
            &places,
            &mut start,
            &times,
            &mut rng,
            &mut counts,
            &mut acc.blocked,
            &mut acc.times,
            opts.guard,
            r,
            opts.on_fire,
            carried,
        );
        acc.ends[r] = start;
        for p in &rest {
            inflight[p.tr] += 1.0;
        }
        left[r] = rest;
        for (i, &c) in counts.iter().enumerate() {
            firings[i] += c as f64;
        }

        for ri in 0..report.len() {
            for v in folded.iter_mut() {
                *v = 0.0;
            }
            for (p, &fi) in fold_idx.iter().enumerate() {
                if fi != ri {
                    continue;
                }
                for (j, &v) in traj[p].iter().enumerate() {
                    folded[j] += v;
                }
            }
            for (j, &v) in folded.iter().enumerate() {
                sums[ri][j] += v;
                sum_squares[ri][j] += v * v;
            }
        }
    }

    let n = opts.realizations as f64;
    let mut res = SimResult {
        method: "ssa".to_string(),
        times: times.clone(),
        ..Default::default()
    };
    for (i, p) in report.iter().enumerate() {
        let mut mean = vec![0.0; times.len()];
        let mut sd = if opts.realizations > 1 {
            Some(vec![0.0; times.len()])
        } else {
            None
        };
        for j in 0..times.len() {
            mean[j] = sums[i][j] / n;
            if let Some(sd) = sd.as_mut() {
                let variance = (sum_squares[i][j] / n - mean[j] * mean[j]).max(0.0);
                sd[j] = variance.sqrt();
            }
        }
        res.final_.insert(p.clone(), *mean.last().unwrap_or(&0.0));
        res.series.push(Series {
            place: p.clone(),
            values: mean,
            std_dev: sd,
        });
    }
    res.depleted = depletions(orig, &res);
    res.contended = drop_stage_contentions(
        exp,
        contentions(m2, &places, &acc.blocked, opts.horizon * n),
    );
    res.caveats = caveats;
    let mut metrics = metrics_of(
        &fold_throughput(exp, &trs, &firings),
        &report,
        &acc.times,
        n,
    );
    for (i, t) in trs.iter().enumerate() {
        if t.delay > 0.0 {
            metrics.in_flight.insert(t.id.clone(), inflight[i] / n);
        }
    }
    res.metrics = Some(metrics);

    Ok((res, acc, left))
}

/// Maps stage-transition firing counts back to the original vocabulary: the
/// final stage's firings are the original transition's completions,
/// intermediate stages are internal, unstaged transitions pass through.
fn fold_throughput(
    exp: Option<&StageExpansion>,
    trs: &[super::compile::CTransition],
    firings: &[f64],
) -> HashMap<String, f64> {
    let mut out: HashMap<String, f64> = HashMap::new();
    let mut intermediate: std::collections::HashSet<&str> = std::collections::HashSet::new();
    if let Some(exp) = exp {
        for ids in exp.stage_ids.values() {
            for id in &ids[..ids.len().saturating_sub(1)] {
                intermediate.insert(id.as_str());
            }
        }
    }
    for (t, &f) in trs.iter().zip(firings.iter()) {
        if intermediate.contains(t.id.as_str()) {
            continue;
        }
        let id = exp
            .and_then(|e| e.final_stage.get(&t.id))
            .cloned()
            .unwrap_or_else(|| t.id.clone());
        *out.entry(id).or_insert(0.0) += f;
    }
    out
}

/// Removes internal stage places from the contention report and folds
/// stage-transition names in the surviving entries' blocking lists back to
/// the original id.
fn drop_stage_contentions(exp: Option<&StageExpansion>, in_: Vec<Contention>) -> Vec<Contention> {
    let Some(exp) = exp else { return in_ };
    let mut original: HashMap<&str, &str> = HashMap::new();
    for (id, stages) in &exp.stage_ids {
        for sid in stages {
            original.insert(sid.as_str(), id.as_str());
        }
    }
    let mut out = Vec::with_capacity(in_.len());
    for mut c in in_ {
        if exp.is_stage_place(&c.place) {
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        let mut blocking = Vec::new();
        for id in &c.blocking {
            let folded = original.get(id.as_str()).copied().unwrap_or(id.as_str());
            if seen.insert(folded.to_string()) {
                blocking.push(folded.to_string());
            }
        }
        c.blocking = blocking;
        out.push(c);
    }
    out
}

/// The engine's assumption list, adjusted for a stage expansion: staged
/// transitions have declared their way out of the exponential worst case.
fn assumptions_for(exp: Option<&StageExpansion>) -> Vec<String> {
    let Some(exp) = exp else {
        return vec![EXPONENTIAL_SERVICE_ASSUMPTION.to_string()];
    };
    let mut ids: Vec<String> = exp
        .stages
        .iter()
        .map(|(id, k)| format!("{id} (Erlang-{k})"))
        .collect();
    ids.sort();
    vec![format!(
        "unstaged transitions draw exponential durations \u{2014} the most erratic a step can be for a given average. \
         Staged transitions are the exception: {} draw phase-type durations with the declared lower spread, so their \
         waiting reflects the declaration rather than the worst case.",
        ids.join(", ")
    )]
}

/// A contention below this fraction of the horizon is dropped: every place
/// in a net is momentarily short of something, and reporting an interval a
/// second and a half long over an eight-hour run is not an answer.
const MIN_CONTENTION: f64 = 0.01;

fn contentions(m: &Model, places: &[String], blk: &Blockage, total_time: f64) -> Vec<Contention> {
    if total_time <= 0.0 {
        return Vec::new();
    }
    let kinds = classify_supply(m);
    let mut out = Vec::new();
    for (i, p) in places.iter().enumerate() {
        let f = blk.waited[i] / total_time;
        if f < MIN_CONTENTION {
            continue;
        }
        let mut held: Vec<String> = blk.holding[i].iter().cloned().collect();
        held.sort();
        let kind = kinds.get(p).copied().unwrap_or(SupplyKind::Queue);
        out.push(Contention {
            place: p.clone(),
            fraction: f,
            kind,
            blocking: held,
        });
    }
    sort_contentions(&mut out);
    out
}

/// Capacity constraints first, then the longest wait first within each
/// group: a queue never outranks something you could buy more of, however
/// long the wait on it was.
fn sort_contentions(out: &mut [Contention]) {
    out.sort_by(|a, b| {
        let (ca, cb) = (a.kind.is_capacity(), b.kind.is_capacity());
        if ca != cb {
            return cb.cmp(&ca);
        }
        match b.fraction.partial_cmp(&a.fraction) {
            Some(std::cmp::Ordering::Equal) | None => a.place.cmp(&b.place),
            Some(ord) => ord,
        }
    });
}

fn metrics_of(
    throughput: &HashMap<String, f64>,
    places: &[String],
    ts: &TimeStats,
    n: f64,
) -> Metrics {
    let mut mt = Metrics {
        throughput: throughput.iter().map(|(k, v)| (k.clone(), v / n)).collect(),
        mean: HashMap::with_capacity(places.len()),
        p95: HashMap::with_capacity(places.len()),
        utilization: HashMap::new(),
        in_flight: HashMap::new(),
    };
    for (i, p) in places.iter().enumerate() {
        mt.mean.insert(p.clone(), ts.mean(i));
        mt.p95.insert(p.clone(), ts.percentile(i, 0.95));
    }
    mt.utilization = utilization(places, &mt.mean);
    mt
}

/// Pairs up the "available"/"busy" (or "in_use") places a resource pool
/// exposes and reports the busy fraction. Matched by suffix so it works
/// both inside a single net (`"available"`) and across a composed one
/// (`"staff/available"`), and only when both halves are present.
fn utilization(places: &[String], means: &HashMap<String, f64>) -> HashMap<String, f64> {
    let mut pools: HashMap<String, [f64; 2]> = HashMap::new();
    let mut seen: HashMap<String, [bool; 2]> = HashMap::new();
    for p in places {
        let (pool, role) = if p == "available" || p.ends_with("/available") {
            (
                p.trim_end_matches("available")
                    .trim_end_matches('/')
                    .to_string(),
                0,
            )
        } else if p == "busy" || p.ends_with("/busy") {
            (
                p.trim_end_matches("busy").trim_end_matches('/').to_string(),
                1,
            )
        } else if p == "in_use" || p.ends_with("/in_use") {
            (
                p.trim_end_matches("in_use")
                    .trim_end_matches('/')
                    .to_string(),
                1,
            )
        } else {
            continue;
        };
        let v = pools.entry(pool.clone()).or_insert([0.0, 0.0]);
        let s = seen.entry(pool).or_insert([false, false]);
        v[role] = *means.get(p).unwrap_or(&0.0);
        s[role] = true;
    }
    let mut out = HashMap::new();
    for (pool, v) in pools {
        let s = seen[&pool];
        if !s[0] || !s[1] {
            continue;
        }
        let total = v[0] + v[1];
        if total <= 0.0 {
            continue;
        }
        let name = if pool.is_empty() {
            "pool".to_string()
        } else {
            pool
        };
        out.insert(name, v[1] / total);
    }
    out
}

/// Reports the first sample time at which each place can no longer supply
/// anything that draws on it (below the smallest weight any transition
/// takes from it, not literally zero), earliest first. A place that starts
/// empty is not "depleted".
pub(crate) fn depletions(m: &Model, res: &SimResult) -> Vec<Depletion> {
    let mut floor: HashMap<String, i64> = HashMap::new();
    for t in &m.transitions {
        for input in m.inputs(&t.id) {
            floor
                .entry(input.place.clone())
                .and_modify(|w| {
                    if input.weight < *w {
                        *w = input.weight;
                    }
                })
                .or_insert(input.weight);
        }
    }
    let mut out = Vec::new();
    for s in &res.series {
        if s.values.is_empty() {
            continue;
        }
        let threshold = *floor.get(s.place.as_str()).unwrap_or(&0) as f64;
        if s.values[0] < threshold || s.values[0] <= 0.0 {
            continue;
        }
        for (i, &v) in s.values.iter().enumerate() {
            if v < threshold || v <= 0.0 {
                let last = *s.values.last().unwrap();
                out.push(Depletion {
                    place: s.place.clone(),
                    at: res.times[i],
                    recovered: last >= threshold && last > 0.0,
                });
                break;
            }
        }
    }
    out.sort_by(|a, b| {
        a.at.partial_cmp(&b.at)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.place.cmp(&b.place))
    });
    out
}

// --- schedules -------------------------------------------------------

/// `SimulateSchedule`: runs the horizon in pieces, one per schedule
/// boundary, carrying each realization's own marking across. SSA draws a
/// waiting time from the current total propensity, so a rate that changes
/// mid-draw would mean sampling from a distribution that no longer applies
/// — restarting at each boundary keeps every draw consistent with the
/// rates in force when it was made.
pub fn simulate_schedule(
    m: &Model,
    marking: &HashMap<String, i64>,
    opts: &Options,
) -> Result<SimResult, StochasticError> {
    let user_rates = opts.rates.clone();
    let user_schedule = opts.schedule.clone();
    let (m2, exp) = m
        .expand_stages()
        .map_err(|e| StochasticError::Other(e.to_string()))?;
    let opts_defaulted = opts.clone().with_defaults(m);
    let (expanded_places, _) = token_places(&m2)?;
    let (report, _) = token_places(m)?;
    let bounds = run_boundaries(m, &user_schedule, opts_defaulted.horizon);

    let mut starts: Option<Vec<Vec<i64>>> = None;
    let mut carry: Option<Vec<Vec<Pending>>> = None;
    let mut combined = SimResult {
        method: "ssa".to_string(),
        ..Default::default()
    };
    let mut series: HashMap<String, Vec<f64>> = HashMap::new();
    let mut throughput: HashMap<String, f64> = HashMap::new();
    let mut in_flight: HashMap<String, f64> = HashMap::new();

    let mut stats = RunStats {
        times: TimeStats::new(report.len()),
        blocked: Blockage::new(expanded_places.len()),
        expansion: exp.clone(),
        ends: Vec::new(),
    };
    let mut caveats: Vec<String> = Vec::new();
    let mut from = 0.0f64;
    for &to in &bounds {
        let span = to - from;
        if span <= 0.0 {
            continue;
        }
        let mut samples = (opts_defaulted.samples as f64 * span / opts_defaulted.horizon) as usize;
        if samples < 2 {
            samples = 2;
        }
        let seg_rates = rates_at(m, &user_rates, &user_schedule, from);
        let segment = Options {
            horizon: span,
            samples,
            rates: seg_rates,
            realizations: opts_defaulted.realizations,
            seed: opts_defaulted.seed,
            guard: opts_defaulted.guard,
            schedule: HashMap::new(),
            on_fire: opts_defaulted.on_fire,
        };
        let (res, seg_stats, seg_carry) = simulate_from(
            m,
            &m2,
            exp.as_ref(),
            marking,
            starts.as_deref(),
            carry.as_deref(),
            &segment,
        )?;
        stats.times.merge(&seg_stats.times);
        stats.blocked.merge(&seg_stats.blocked);
        starts = Some(seg_stats.ends);
        carry = Some(seg_carry);

        for t in &res.times {
            combined.times.push(from + t);
        }
        for sr in &res.series {
            series
                .entry(sr.place.clone())
                .or_default()
                .extend(sr.values.iter().copied());
        }
        if let Some(metrics) = &res.metrics {
            for (id, n) in &metrics.throughput {
                *throughput.entry(id.clone()).or_insert(0.0) += n;
            }
            in_flight = metrics.in_flight.clone();
        }
        if caveats.is_empty() {
            caveats = res.caveats.clone();
        }

        from = to;
    }

    let mut keys: Vec<&String> = series.keys().collect();
    keys.sort();
    for p in keys {
        let vals = &series[p];
        combined.series.push(Series {
            place: p.clone(),
            values: vals.clone(),
            std_dev: None,
        });
        combined
            .final_
            .insert(p.clone(), *vals.last().unwrap_or(&0.0));
    }
    combined.depleted = depletions(m, &combined);
    combined.contended = drop_stage_contentions(
        exp.as_ref(),
        contentions(
            &m2,
            &expanded_places,
            &stats.blocked,
            opts_defaulted.horizon * opts_defaulted.realizations as f64,
        ),
    );
    combined.caveats = caveats;
    combined.assumptions.extend(assumptions_for(exp.as_ref()));

    let mut mt = Metrics {
        throughput,
        in_flight,
        mean: HashMap::new(),
        p95: HashMap::new(),
        utilization: HashMap::new(),
    };
    for (i, p) in report.iter().enumerate() {
        mt.mean.insert(p.clone(), stats.times.mean(i));
        mt.p95.insert(p.clone(), stats.times.percentile(i, 0.95));
    }
    mt.utilization = utilization(&report, &mt.mean);
    combined.metrics = Some(mt);

    Ok(combined)
}

/// Every segment end inside the horizon — the caller's schedule and the
/// model's own declared day shape — plus the horizon itself.
fn run_boundaries(
    m: &Model,
    schedule: &HashMap<String, Vec<RateSegment>>,
    horizon: f64,
) -> Vec<f64> {
    let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut note = |until: f64| {
        if until > 0.0 && until < horizon && seen.insert(until.to_bits()) {
            out.push(until);
        }
    };
    for segs in schedule.values() {
        for seg in segs {
            note(seg.until);
        }
    }
    for t in &m.transitions {
        for seg in &t.schedule {
            note(seg.until);
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out.push(horizon);
    out
}

/// The rate table in force at time `t`: the model's rates, then its own
/// declared day shape for every transition the caller left alone, then the
/// caller's constant overrides, then the caller's schedule segment.
fn rates_at(
    m: &Model,
    user_rates: &HashMap<String, f64>,
    user_schedule: &HashMap<String, Vec<RateSegment>>,
    t: f64,
) -> HashMap<String, f64> {
    let mut rates = super::options::rates(m);
    for tr in &m.transitions {
        if user_rates.contains_key(&tr.id) || user_schedule.contains_key(&tr.id) {
            continue;
        }
        if let Some(v) = tr.scheduled_rate(t) {
            rates.insert(tr.id.clone(), v);
        }
    }
    schedule_rates(&rates, user_schedule, t, &[user_rates])
}

/// The rate table in force at time `t`: `base` with the schedule's segment
/// for `t` overlaid, `overrides` applied in order first.
fn schedule_rates(
    base: &HashMap<String, f64>,
    schedule: &HashMap<String, Vec<RateSegment>>,
    t: f64,
    overrides: &[&HashMap<String, f64>],
) -> HashMap<String, f64> {
    let mut rates = base.clone();
    for ov in overrides {
        for (id, r) in ov.iter() {
            rates.insert(id.clone(), *r);
        }
    }
    for (id, segs) in schedule {
        // The last segment extends past its own `until`, so a schedule
        // that stops short of the horizon holds its final rate.
        let mut value = segs.last().map(|s| s.value).unwrap_or(0.0);
        for seg in segs {
            if t < seg.until {
                value = seg.value;
                break;
            }
        }
        rates.insert(id.clone(), value);
    }
    rates
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_metamodel::{Arc, ArcType, Marking, Place, Transition};

    /// M/M/1-shaped queue: `arrive` is a source (no input), `serve` drains
    /// `queue` into `done` with a declared capacity on `queue` so the run
    /// has something to be short of.
    fn mm1(capacity: i64) -> Model {
        Model {
            name: "mm1".into(),
            places: vec![
                Place {
                    id: "queue".into(),
                    capacity,
                    ..Default::default()
                },
                Place {
                    id: "done".into(),
                    ..Default::default()
                },
            ],
            transitions: vec![
                Transition {
                    id: "arrive".into(),
                    rate: 5.0,
                    ..Default::default()
                },
                Transition {
                    id: "serve".into(),
                    rate: 1.0,
                    ..Default::default()
                },
            ],
            arcs: vec![
                Arc {
                    from: "arrive".into(),
                    to: "queue".into(),
                    ..Default::default()
                },
                Arc {
                    from: "queue".into(),
                    to: "serve".into(),
                    ..Default::default()
                },
                Arc {
                    from: "serve".into(),
                    to: "done".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn simulate_reports_metrics_and_keeps_caveats_apart_from_assumptions() {
        let m = mm1(0);
        let opts = Options {
            horizon: 5.0,
            samples: 20,
            realizations: 3,
            seed: 7,
            ..Default::default()
        };
        let res = simulate(&m, &HashMap::new(), &opts).expect("simulate");
        assert_eq!(res.method, "ssa");
        let metrics = res.metrics.expect("metrics populated");
        assert!(metrics.throughput.contains_key("arrive"));
        assert!(metrics.mean.contains_key("queue"));
        assert!(metrics.p95.contains_key("queue"));
        // No guard, no capacity breach possible above a limit that's never
        // set (capacity 0 = unbounded): no caveats, but the SSA's exponential-
        // service note is always an assumption, never a caveat.
        assert!(res.caveats.is_empty());
        assert_eq!(res.assumptions.len(), 1);
        assert!(res.assumptions[0].contains("exponentially distributed"));
    }

    #[test]
    fn bounded_queue_depletes_and_is_uncontended_when_never_empty() {
        // A tiny capacity with a much faster arrival than service rate: the
        // queue should fill (not "deplete" - depletion is running OUT, and a
        // full queue never empties in this test). Use the opposite: a queue
        // that starts stocked and only drains, to exercise `depletions`.
        let m = Model {
            name: "pantry".into(),
            places: vec![
                Place {
                    id: "beans".into(),
                    initial: 3,
                    ..Default::default()
                },
                Place {
                    id: "brewed".into(),
                    ..Default::default()
                },
            ],
            transitions: vec![Transition {
                id: "brew".into(),
                rate: 5.0,
                ..Default::default()
            }],
            arcs: vec![
                Arc {
                    from: "beans".into(),
                    to: "brew".into(),
                    ..Default::default()
                },
                Arc {
                    from: "brew".into(),
                    to: "brewed".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let opts = Options {
            horizon: 10.0,
            samples: 30,
            realizations: 1,
            seed: 1,
            ..Default::default()
        };
        let res = simulate(&m, &HashMap::new(), &opts).expect("simulate");
        assert_eq!(res.depleted.len(), 1);
        assert_eq!(res.depleted[0].place, "beans");
        assert!(!res.depleted[0].recovered, "beans never refill");
    }

    #[test]
    fn guard_caveat_when_no_evaluator_supplied() {
        let mut m = mm1(0);
        m.transitions[1].guard = "tokens(\"queue\") >= 1".into();
        let opts = Options {
            horizon: 2.0,
            samples: 10,
            realizations: 1,
            seed: 1,
            ..Default::default()
        };
        let res = simulate(&m, &HashMap::new(), &opts).expect("simulate");
        assert_eq!(res.caveats.len(), 1);
        assert!(res.caveats[0].contains("serve"));
    }

    #[test]
    fn decidable_guard_is_enforced_not_caveated() {
        let mut m = mm1(0);
        m.transitions[1].guard = "tokens(\"queue\") >= 1".into();
        let eval = |expr: &str, mk: &Marking| -> Result<bool, String> {
            if expr == "tokens(\"queue\") >= 1" {
                Ok(*mk.get("queue").unwrap_or(&0) >= 1)
            } else {
                Err(format!("cannot evaluate {expr:?}"))
            }
        };
        let opts = Options {
            horizon: 2.0,
            samples: 10,
            realizations: 1,
            seed: 1,
            guard: Some(&eval),
            ..Default::default()
        };
        let res = simulate(&m, &HashMap::new(), &opts).expect("simulate");
        assert!(res.caveats.is_empty());
    }

    #[test]
    fn undecidable_guard_is_caveated_not_enforced() {
        let mut m = mm1(0);
        m.transitions[1].guard = "amount > 0".into(); // references an action param
        let eval = |expr: &str, mk: &Marking| -> Result<bool, String> {
            if expr == "tokens(\"queue\") >= 1" {
                Ok(*mk.get("queue").unwrap_or(&0) >= 1)
            } else {
                Err(format!("cannot evaluate {expr:?}"))
            }
        };
        let opts = Options {
            horizon: 2.0,
            samples: 10,
            realizations: 1,
            seed: 1,
            guard: Some(&eval),
            ..Default::default()
        };
        let res = simulate(&m, &HashMap::new(), &opts).expect("simulate");
        assert_eq!(res.caveats.len(), 1);
        assert!(res.caveats[0].contains("action parameters"));
    }

    /// A closed system (no source, no sink: tokens only move between two
    /// places) run under a schedule with a mid-horizon boundary. If the
    /// carried marking were a *rounded mean* across realizations (the
    /// pre-v0.28.1 go-pflow bug this phase fixes) rather than each
    /// realization's own integer ending marking, the second segment would
    /// start every realization from the same value and the total would
    /// still happen to add up — so the real regression this guards is
    /// `simulate_schedule` running at all across >1 realizations without
    /// panicking on a marking-shape mismatch, and every place total
    /// conserved (the invariant `a + b == 5` this net guarantees) at every
    /// sampled time on both sides of the boundary.
    #[test]
    fn schedule_carries_each_realization_own_marking_across_boundaries() {
        let m = Model {
            name: "closed".into(),
            places: vec![
                Place {
                    id: "a".into(),
                    initial: 5,
                    ..Default::default()
                },
                Place {
                    id: "b".into(),
                    ..Default::default()
                },
            ],
            transitions: vec![
                Transition {
                    id: "fwd".into(),
                    rate: 3.0,
                    ..Default::default()
                },
                Transition {
                    id: "back".into(),
                    rate: 3.0,
                    ..Default::default()
                },
            ],
            arcs: vec![
                Arc {
                    from: "a".into(),
                    to: "fwd".into(),
                    ..Default::default()
                },
                Arc {
                    from: "fwd".into(),
                    to: "b".into(),
                    ..Default::default()
                },
                Arc {
                    from: "b".into(),
                    to: "back".into(),
                    ..Default::default()
                },
                Arc {
                    from: "back".into(),
                    to: "a".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut schedule = HashMap::new();
        schedule.insert(
            "fwd".to_string(),
            vec![
                RateSegment {
                    until: 4.0,
                    value: 1.0,
                },
                RateSegment {
                    until: 8.0,
                    value: 6.0,
                },
            ],
        );
        let opts = Options {
            horizon: 8.0,
            samples: 40,
            realizations: 6,
            seed: 11,
            schedule,
            ..Default::default()
        };
        let res = simulate_schedule(&m, &HashMap::new(), &opts).expect("simulate_schedule");
        let a = res.series.iter().find(|s| s.place == "a").unwrap();
        let b = res.series.iter().find(|s| s.place == "b").unwrap();
        for i in 0..a.values.len() {
            assert!(
                (a.values[i] + b.values[i] - 5.0).abs() < 1e-6,
                "a+b should stay 5 at every sample (index {i}): a={}, b={}",
                a.values[i],
                b.values[i]
            );
        }
        assert!(res.metrics.is_some());
    }

    #[test]
    fn staged_transition_reduces_variance_and_notes_erlang_in_assumptions() {
        let staged = Model {
            name: "wash".into(),
            places: vec![
                Place {
                    id: "queue".into(),
                    initial: 20,
                    ..Default::default()
                },
                Place {
                    id: "washing".into(),
                    ..Default::default()
                },
                Place {
                    id: "bay_free".into(),
                    initial: 1,
                    ..Default::default()
                },
                Place {
                    id: "done".into(),
                    ..Default::default()
                },
            ],
            transitions: vec![
                Transition {
                    id: "start".into(),
                    rate: 720.0,
                    ..Default::default()
                },
                Transition {
                    id: "finish".into(),
                    rate: 4.0,
                    stages: 4,
                    ..Default::default()
                },
            ],
            arcs: vec![
                Arc {
                    from: "queue".into(),
                    to: "start".into(),
                    ..Default::default()
                },
                Arc {
                    from: "bay_free".into(),
                    to: "start".into(),
                    ..Default::default()
                },
                Arc {
                    from: "start".into(),
                    to: "washing".into(),
                    ..Default::default()
                },
                Arc {
                    from: "washing".into(),
                    to: "finish".into(),
                    ..Default::default()
                },
                Arc {
                    from: "finish".into(),
                    to: "done".into(),
                    ..Default::default()
                },
                Arc {
                    from: "finish".into(),
                    to: "bay_free".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let opts = Options {
            horizon: 4.0,
            samples: 20,
            realizations: 1,
            seed: 3,
            ..Default::default()
        };
        let res = simulate(&staged, &HashMap::new(), &opts).expect("simulate");
        assert_eq!(res.assumptions.len(), 1);
        assert!(res.assumptions[0].contains("Erlang-4"));
        // No stage place ("finish@stageN") leaks into the reported series.
        assert!(!res.series.iter().any(|s| s.place.contains("@stage")));
        assert!(res.final_.contains_key("washing"));
        assert!(!res.final_.contains_key("finish@stage1"));
    }

    #[test]
    fn arc_type_normal_is_read_by_default_for_unrecognized() {
        // Guard against a stray typo in compile.rs's match on ArcType: only
        // Inhibitor arcs go to `inhibits`, everything read-only that isn't
        // Inhibitor (i.e. Read) goes to `reads`.
        let mut m = mm1(5);
        m.arcs.push(Arc {
            from: "done".into(),
            to: "serve".into(),
            typ: ArcType::Read,
            weight: 1,
            ..Default::default()
        });
        // done starts at 0, so serve should never fire while the read arc
        // is unmet.
        let opts = Options {
            horizon: 3.0,
            samples: 10,
            realizations: 1,
            seed: 1,
            ..Default::default()
        };
        let res = simulate(&m, &HashMap::new(), &opts).expect("simulate");
        let done = res.final_["done"];
        assert_eq!(
            done, 0.0,
            "serve should never fire: its read arc on done is never met"
        );
    }
}
