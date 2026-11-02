//! `sde`: hosted `pflow_sde`, pflow-native's `sim.sde` with its artifact
//! inlined as `timeseries`.
//!
//! Chemical Langevin SDE ([`pflow_solver::ssa::sde`], Euler-Maruyama with 20
//! substeps per sample interval, byte-exact with go-pflow's portable path)
//! from the net's marking with each transition's `rate`. Refuses a net a
//! diffusion cannot express (read or inhibitor arcs, non-kinetic inputs, a
//! capacity some firing can raise, a delay), in the engine's own words. A
//! value that overflows is null, as native writes it. The answer always fits
//! the hosted result budget whole.

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use pflow_metamodel::ArcType;
use pflow_solver::ssa::{
    self, ArcKind, SdeOptions, SdeResult, SsaArc, SsaModel, SsaPlace, SsaTransition,
};
use serde_json::{json, Map, Value};

use crate::wbnet::{
    check_horizon, check_runs, check_samples, check_tokens, downsample, downsample_len, fit_answer,
    go_str_len, last, num, nums, pick, rust_float, ArgKind, Args, Sizing, WbNet, MAX_HORIZON,
    MAX_RESULT_BYTES, NUM_BYTES,
};

/// The local tool name.
pub const TOOL: &str = "sde";

/// Points returned (native: 5000).
pub const MAX_POINTS: usize = 500;
/// Euler-Maruyama grid points a `dt` may ask for (native's `MAX_SDE_STEPS`).
const MAX_SDE_STEPS: f64 = 50_000.0;
const INTERNAL_STEPS: f64 = 20.0;
const DEFAULT_POINTS: usize = 101;
/// The work budget: realizations × sample intervals × (transitions ×
/// (places + TRANSITION_COST) + 2 × places).
const MAX_WORK: usize = 100_000_000;
const TRANSITION_COST: usize = 50;
/// samples × places.
const MAX_CELLS: usize = 500_000;

const ARGS: &[(&str, ArgKind)] = &[
    ("net", ArgKind::Any),
    ("t_end", ArgKind::Num),
    ("seed", ArgKind::Int),
    ("runs", ArgKind::Int),
    ("points", ArgKind::Int),
    ("dt", ArgKind::Num),
    ("places", ArgKind::StrList),
];

/// Runs `sde` (hosted `pflow_sde`): arguments `net` (the editor's JSON-LD),
/// `t_end` (default 10), `seed` (default 1), `runs` (default 1), `points`
/// (default 101, fewer when that many would not fit), `dt` and `places`.
pub fn run(args: &Value) -> Result<Value, String> {
    let a = Args::check(TOOL, args, ARGS)?;
    let n = WbNet::from_args(&a)?;
    run_sde(&n, &a)
}

fn run_sde(n: &WbNet, a: &Args) -> Result<Value, String> {
    let t_end = a.num("t_end").unwrap_or(10.0);
    check_horizon("t_end", t_end, MAX_HORIZON)?;
    let runs = a.uint("runs", 1)?;
    check_runs("runs", runs)?;
    let points = a.uint("points", DEFAULT_POINTS)?;
    check_samples("points", points, MAX_POINTS)?;
    let seed = a.seed("seed", 1)?;
    // 20 Euler-Maruyama steps per sample interval: a step of dt means a grid
    // of t_end / (20 dt) intervals, thinned to points. Without dt the grid
    // is the points.
    let mut dt_samples = 0usize;
    if let Some(dt) = a.num("dt") {
        if !(dt > 0.0 && dt.is_finite()) {
            return Err("dt must be a positive number".into());
        }
        let k = (t_end / (INTERNAL_STEPS * dt)).ceil();
        if k + 1.0 > MAX_SDE_STEPS {
            return Err(format!(
                "dt {} needs {} sample intervals over t_end {}, over the {} cap; use a larger dt",
                rust_float(dt),
                k as u64,
                rust_float(t_end),
                MAX_SDE_STEPS as u64
            ));
        }
        dt_samples = (k as usize).max(1) + 1;
    }
    let samples_for = |p: usize| if dt_samples > 0 { dt_samples } else { p };
    let start = n.start();
    check_tokens(&start, false)?;
    let model = ssa_model(n, &start);
    // The refusal is the engine's: it reports a gated net as diverged with
    // no series, before running anything.
    let probe = ssa::simulate_sde(
        &model,
        &SdeOptions {
            horizon: t_end,
            samples: 2,
            realizations: 1,
            seed,
        },
    )
    .map_err(|e| format!("sde: {e}"))?;
    if probe.diverged && probe.values.is_empty() {
        return Err(format!("sde refused: {}", probe.reason));
    }
    let filter = a.strings("places");
    let keep = n.series_filter(filter.as_deref())?;

    let places = n.places.len();
    let transitions = model.transitions.len();
    let mut done: HashMap<usize, SdeResult> = HashMap::new();
    let mut build = |points: i64| -> Result<Map<String, Value>, String> {
        let points = points as usize;
        let samples = samples_for(points);
        if samples * places > MAX_CELLS {
            return Err(format!(
                "samples × places is {}, over this server's budget of {MAX_CELLS}; use a larger dt or fewer points",
                samples * places
            ));
        }
        let per_interval = transitions * (places + TRANSITION_COST) + 2 * places;
        let work = runs * (samples - 1) * per_interval;
        if work > MAX_WORK {
            return Err(format!(
                "runs × sample intervals × (transitions × (places + {TRANSITION_COST}) + 2 × places) is {work}, over this server's budget of {MAX_WORK}; use fewer runs, fewer points or a larger dt"
            ));
        }
        // With dt, fewer points thin the same run.
        let res = match done.entry(samples) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => {
                let res = ssa::simulate_sde(
                    &model,
                    &SdeOptions {
                        horizon: t_end,
                        samples,
                        realizations: runs,
                        seed,
                    },
                )
                .map_err(|e| format!("sde: {e}"))?;
                if res.diverged && res.values.is_empty() {
                    return Err(format!("sde refused: {}", res.reason));
                }
                e.insert(res)
            }
        };
        let res = &*res;
        let idx = downsample(res.times.len(), points);
        let mut finals = Map::new();
        let mut sds = Map::new();
        let mut series = vec![];
        for (id, d) in n.places.iter().zip(&n.display) {
            let p = res.places.iter().position(|q| q == id);
            let values: &[f64] = p.map(|p| res.values[p].as_slice()).unwrap_or(&[]);
            finals.insert(d.clone(), num(last(values)));
            let mut e = json!({"name": d, "values": nums(&pick_or_zero(values, &idx))});
            if runs > 1 {
                let sd: &[f64] = match (&res.stddev, p) {
                    (Some(sd), Some(p)) => &sd[p],
                    _ => &[],
                };
                sds.insert(d.clone(), num(last(sd)));
                e["band"] = nums(&pick_or_zero(sd, &idx));
            }
            series.push(e);
        }
        let mut out = Map::new();
        out.insert("engine".into(), json!("sde"));
        out.insert("runs".into(), json!(runs));
        out.insert("seed".into(), json!(seed));
        out.insert("t_end".into(), num(t_end));
        out.insert(
            "dt".into(),
            num(t_end / (samples - 1) as f64 / INTERNAL_STEPS),
        );
        out.insert("final".into(), Value::Object(finals));
        out.insert(
            "final_sd".into(),
            if runs > 1 {
                Value::Object(sds)
            } else {
                Value::Null
            },
        );
        out.insert(
            "timeseries".into(),
            json!({
                "kind": "timeseries",
                "x_label": "t",
                "x": nums(&pick(&res.times, &idx)),
                "series": n.select_series(series, &keep),
            }),
        );
        Ok(out)
    };
    // The answer must fit the result budget: the points the caller asked
    // for, or as many of the default 101 as fit.
    let grid = move |p: i64| downsample_len(samples_for(p as usize), p as usize) as i64;
    let upper = sde_size(n, &keep, runs > 1, NUM_BYTES, 900);
    let lower = sde_size(n, &keep, runs > 1, 2, 0);
    let sizing = Sizing {
        tool: TOOL,
        what: "points",
        def: DEFAULT_POINTS as i64,
        min: 2,
        explicit: a.has("points"),
        budget: MAX_RESULT_BYTES,
        upper: Box::new(move |p| upper(grid(p))),
        lower: Box::new(move |p| lower(grid(p))),
        grid: Box::new(grid),
        hint: "pass places to return fewer series",
    };
    fit_answer(&sizing, points as i64, &mut build)
}

fn pick_or_zero(v: &[f64], idx: &[usize]) -> Vec<f64> {
    if v.is_empty() {
        vec![0.0; idx.len()]
    } else {
        pick(v, idx)
    }
}

/// Bounds the answer in bytes as a function of the points it returns, with
/// every number taking `num` bytes and `slack` bytes for the keys and a note.
fn sde_size(n: &WbNet, keep: &[bool], band: bool, num: i64, slack: i64) -> impl Fn(i64) -> i64 {
    let b = if band { 2 } else { 1 };
    let mut fixed = slack + 4 * num; // engine, runs, seed, t_end, dt
    let mut per_point = num; // x
    for (i, d) in n.display.iter().enumerate() {
        fixed += (go_str_len(d) + 1 + num) * b; // final, final_sd
        if keep[i] {
            fixed += go_str_len(d); // a series' name
            if slack > 0 {
                fixed += 32; // {"name":…,"values":[],"band":[]},
            }
            per_point += num * b;
        }
    }
    move |k| fixed + per_point * k
}

/// Native's `Net::ssa_model`: the portable engines' ordered model, places in
/// the net's order, starting at `start`. A transition at rate 0 is left out
/// with its arcs (0 disables it, where the engine would read 0 as its
/// default rate 1).
fn ssa_model(n: &WbNet, start: &HashMap<String, i64>) -> SsaModel {
    let places = n
        .places
        .iter()
        .map(|id| SsaPlace {
            id: id.clone(),
            initial: start.get(id).copied().unwrap_or(0),
            capacity: n
                .model
                .places
                .iter()
                .find(|p| &p.id == id)
                .map(|p| p.capacity.max(0))
                .unwrap_or(0),
        })
        .collect();
    let mut transitions = vec![];
    let mut arcs = vec![];
    let w = |w: i64| w.max(1);
    for t in &n.model.transitions {
        let rate = n.rate(&t.id);
        if rate <= 0.0 {
            continue;
        }
        transitions.push(SsaTransition {
            id: t.id.clone(),
            rate,
            delay: t.delay.max(0.0),
        });
        for a in n.model.inputs(&t.id) {
            arcs.push(SsaArc {
                from: a.place.clone(),
                to: t.id.clone(),
                weight: w(a.weight),
                kind: ArcKind::Flow,
                kinetic: a.kinetic,
            });
        }
        for a in n.model.outputs(&t.id) {
            arcs.push(SsaArc {
                from: t.id.clone(),
                to: a.place.clone(),
                weight: w(a.weight),
                kind: ArcKind::Flow,
                kinetic: true,
            });
        }
        for a in n.model.tests(&t.id) {
            arcs.push(SsaArc {
                from: a.place.clone(),
                to: t.id.clone(),
                weight: w(a.weight),
                kind: if a.typ == ArcType::Inhibitor {
                    ArcKind::Inhibitor
                } else {
                    ArcKind::Read
                },
                kinetic: false,
            });
        }
    }
    SsaModel {
        places,
        transitions,
        arcs,
    }
}
