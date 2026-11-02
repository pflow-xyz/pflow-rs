//! `what_if`: hosted `pflow_what_if`, the editor's `sim.what_if` over
//! pflow-native's `sim.compare`.
//!
//! Runs the net as is and with `delta` more tokens in `place` on the scenario
//! SSA ([`pflow_solver::stochastic`], go-pflow's scenario engine: inhibitors,
//! read arcs and capacities respected), both arms on one shared seed so the
//! difference measures the change, not the dice. Returns each arm's summary,
//! an overlay time series (group = arm), a table of final means and each
//! arm's finals table. The answer always fits the hosted result budget whole.

use std::cell::RefCell;
use std::collections::HashMap;

use pflow_solver::stochastic;
use serde_json::{json, Map, Value};

use crate::wbnet::{
    check_horizon, check_runs, check_samples, check_tokens, fit_answer, go_str_len, last, num,
    nums, rust_float, rust_i64, sorted_nums, ArgKind, Args, Sizing, WbNet, MAX_HORIZON,
    MAX_PLACE_TOKENS, MAX_RESULT_BYTES, NUM_BYTES,
};

/// The local tool name.
pub const TOOL: &str = "what_if";

/// Sample points (native: 5000).
pub const MAX_SAMPLES: usize = 500;
const DEFAULT_SAMPLES: usize = 60;
/// realizations × samples × places, per arm.
const MAX_CELLS: usize = 4_000_000;

const ARGS: &[(&str, ArgKind)] = &[
    ("net", ArgKind::Any),
    ("place", ArgKind::Str),
    ("delta", ArgKind::Int),
    ("t_end", ArgKind::Num),
    ("runs", ArgKind::Int),
    ("seed", ArgKind::Int),
    ("samples", ArgKind::Int),
    ("places", ArgKind::StrList),
];

/// Runs `what_if` (hosted `pflow_what_if`): arguments `net` (the editor's
/// JSON-LD), `place` (required), `delta` (default 1), `t_end` (default 10),
/// `runs` (default 16), `seed` (default 7), `samples` (default 60, fewer when
/// that many would not fit) and `places`.
pub fn run(args: &Value) -> Result<Value, String> {
    let a = Args::check(TOOL, args, ARGS)?;
    if !a.has("place") {
        return Err(format!("{TOOL}: missing argument \"place\""));
    }
    let n = WbNet::from_args(&a)?;
    run_what_if(&n, &a)
}

/// One place's final value across realizations (native's `Final`).
struct Final {
    mean: f64,
    sd: Option<f64>,
    min: f64,
    max: f64,
}

/// Native's `ScenarioRun` for the SSA engine, in display names.
struct ScenarioRun {
    t: Vec<f64>,
    /// Per place.
    mean: Vec<Vec<f64>>,
    /// Per place; `None` for one realization.
    sd: Option<Vec<Vec<f64>>>,
    finals: Vec<Final>,
    metrics: Option<Value>,
    depleted: Vec<Value>,
    contended: Vec<Value>,
    waiting: Vec<Value>,
    caveats: Vec<String>,
    assumptions: Vec<String>,
}

/// The part of native's `Scenario` `sim.what_if` sets.
struct Scenario {
    name: String,
    /// Model place id → tokens.
    marking: HashMap<String, i64>,
    hours: f64,
    samples: usize,
    realizations: usize,
    seed: u64,
}

fn run_what_if(n: &WbNet, a: &Args) -> Result<Value, String> {
    // The editor checks the arguments first, then the place.
    let hours = a.num("t_end").unwrap_or(10.0);
    check_horizon("t_end", hours, MAX_HORIZON)?;
    let runs = a.uint("runs", 16)?;
    check_runs("runs", runs)?;
    let samples = a.uint("samples", DEFAULT_SAMPLES)?;
    check_samples("samples", samples, MAX_SAMPLES)?;
    let seed = a.seed("seed", 7)?;
    let place = a.str("place").unwrap_or_default();
    let doc_place = n
        .doc
        .get("places")
        .and_then(Value::as_object)
        .and_then(|p| p.get(place))
        .and_then(Value::as_object);
    let Some(pd) = doc_place else {
        let is_transition = n
            .doc
            .get("transitions")
            .and_then(Value::as_object)
            .and_then(|t| t.get(place))
            .is_some_and(|t| !t.is_null());
        if is_transition {
            return Err(format!("{place:?} is a transition, not a place"));
        }
        return Err(format!("{place:?} is not a place or transition"));
    };
    let now: f64 = match pd.get("initial") {
        Some(Value::Array(v)) => v.iter().map(|x| x.as_f64().unwrap_or(0.0)).sum(),
        Some(Value::Number(x)) => x.as_f64().unwrap_or(0.0),
        _ => 0.0,
    };
    let mut delta = a.num("delta").unwrap_or(1.0);
    if delta == 0.0 {
        delta = 0.0; // a -0 argument labels "+0", as the editor prints it
    }
    if now + delta < 0.0 {
        return Err(format!(
            "{place} has {}; cannot remove {}",
            rust_float(now),
            rust_float(-delta)
        ));
    }
    let sign = if delta >= 0.0 { "+" } else { "" };
    let arm = |name: String| Scenario {
        name,
        marking: HashMap::new(),
        hours,
        samples,
        realizations: runs,
        seed,
    };
    let mut arms = [
        arm("as is".into()),
        arm(format!("{sign}{} {place}", rust_float(delta))),
    ];
    // The second scenario's marking, read as native's parse_scenario reads it.
    let id = n
        .resolve_place(place)
        .map_err(|e| format!("scenario 2: {e}"))?;
    let to = now + delta;
    if to != to.trunc() {
        return Err(format!(
            "scenario 2: marking for {place:?} must be an integer, got {}",
            rust_float(to)
        ));
    }
    if to > MAX_PLACE_TOKENS as f64 {
        return Err(format!(
            "scenario 2: marking for {place:?} is {}, over the {MAX_PLACE_TOKENS} cap",
            rust_i64(to)
        ));
    }
    arms[1].marking.insert(id, to as i64);

    let filter = a.strings("places");
    let keep = n.series_filter(filter.as_deref())?;
    let names = [arms[0].name.clone(), arms[1].name.clone()];

    let mut build = |samples: i64| -> Result<Map<String, Value>, String> {
        let samples = samples as usize;
        for sc in arms.iter_mut() {
            sc.samples = samples;
        }
        // The engine records every realization's samples of every place.
        let cells = runs * samples * n.places.len();
        if cells > MAX_CELLS {
            return Err(format!(
                "runs × samples × places is {cells}, over this server's budget of {MAX_CELLS}; use fewer runs or fewer samples"
            ));
        }
        let mut done = vec![];
        for sc in &arms {
            done.push(run_scenario(n, sc).map_err(|e| format!("{}: {e}", sc.name))?);
        }
        let mut overlay = vec![];
        let mut x: &[f64] = &[];
        let mut summaries = vec![];
        let mut finals = vec![];
        let mut rows = vec![];
        for (sc, r) in arms.iter().zip(&done) {
            x = &r.t;
            for (j, d) in n.display.iter().enumerate() {
                let mut s = json!({"name": d, "values": nums(&r.mean[j]), "group": sc.name});
                if let Some(sd) = &r.sd {
                    s["band"] = nums(&sd[j]);
                }
                overlay.push(s);
            }
            let mut row = vec![json!(sc.name)];
            row.extend(r.finals.iter().map(|f| num(f.mean)));
            rows.push(Value::Array(row));
            summaries.push(summary(n, r, sc));
            finals.push(finals_table(n, r));
        }
        let mut columns = vec![json!("scenario")];
        columns.extend(n.display.iter().map(|d| json!(d)));
        let mut out = Map::new();
        out.insert("seed".into(), json!(seed));
        out.insert("scenarios".into(), Value::Array(summaries));
        out.insert(
            "timeseries".into(),
            json!({
                "kind": "timeseries",
                "x_label": "t",
                "x": nums(x),
                "series": n.select_series(overlay, &keep),
            }),
        );
        out.insert(
            "table".into(),
            json!({"kind": "table", "columns": columns, "rows": rows}),
        );
        out.insert("finals".into(), Value::Array(finals));
        out.insert("place".into(), json!(place));
        out.insert("from".into(), num(now));
        out.insert("to".into(), num(to));
        Ok(out)
    };
    // The answer must fit the result budget: the samples the caller asked
    // for, or as many of the default 60 as fit.
    let sizing = Sizing {
        tool: TOOL,
        what: "samples",
        def: DEFAULT_SAMPLES as i64,
        min: 2,
        explicit: a.has("samples"),
        budget: MAX_RESULT_BYTES,
        upper: Box::new(what_if_size(
            n,
            &keep,
            runs > 1,
            &names,
            place,
            NUM_BYTES,
            true,
        )),
        lower: Box::new(what_if_size(n, &keep, runs > 1, &names, place, 2, false)),
        grid: Box::new(|k| k),
        hint: "pass places to return fewer series",
    };
    fit_answer(&sizing, samples as i64, &mut build)
}

/// Native's `run_scenario` on the SSA engine: pflow-solver's scenario SSA
/// from the net's marking with the scenario's overrides and the document's
/// rates.
fn run_scenario(n: &WbNet, sc: &Scenario) -> Result<ScenarioRun, String> {
    let mut start = n.start();
    for (k, v) in &sc.marking {
        start.insert(k.clone(), *v);
    }
    check_tokens(&start, true)?;
    let (order, _) = stochastic::token_places(&n.model).map_err(|e| format!("scenario: {e}"))?;
    let init: Vec<i64> = order
        .iter()
        .map(|p| start.get(p).copied().unwrap_or(0))
        .collect();
    // Every realization's last marking, from the engine's on-fire hook.
    let ends: RefCell<Vec<Vec<i64>>> = RefCell::new(vec![init; sc.realizations]);
    let hook = |r: usize, _t: f64, _id: &str, mk: &[i64]| {
        let mut ends = ends.borrow_mut();
        if let Some(e) = ends.get_mut(r) {
            if mk.len() == e.len() {
                e.copy_from_slice(mk);
            }
        }
    };
    let opts = stochastic::Options {
        horizon: sc.hours,
        samples: sc.samples,
        rates: n.rates().clone(),
        seed: sc.seed,
        realizations: sc.realizations,
        guard: None,
        schedule: HashMap::new(),
        on_fire: Some(&hook),
    };
    let res =
        stochastic::simulate(&n.model, &start, &opts).map_err(|e| format!("scenario: {e}"))?;
    let ends = ends.into_inner();

    let ns = res.times.len();
    let mut run = ScenarioRun {
        t: res.times.clone(),
        mean: vec![],
        sd: (sc.realizations > 1).then(Vec::new),
        finals: vec![],
        metrics: None,
        depleted: vec![],
        contended: vec![],
        waiting: vec![],
        caveats: res.caveats.clone(),
        assumptions: res.assumptions.clone(),
    };
    for id in &n.places {
        let s = res.series.iter().find(|s| &s.place == id);
        let values = s.map(|s| s.values.clone()).unwrap_or_else(|| vec![0.0; ns]);
        let spread = s.and_then(|s| s.std_dev.clone());
        let mean = res.final_.get(id).copied().unwrap_or_else(|| last(&values));
        let (mut lo, mut hi) = (mean, mean);
        if let Some(ti) = order.iter().position(|p| p == id) {
            if !ends.is_empty() {
                lo = f64::INFINITY;
                hi = f64::NEG_INFINITY;
                for e in &ends {
                    let x = e[ti] as f64;
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
        }
        run.finals.push(Final {
            mean,
            sd: spread.as_deref().map(last),
            min: lo,
            max: hi,
        });
        if let Some(sd) = run.sd.as_mut() {
            sd.push(spread.unwrap_or_else(|| vec![0.0; ns]));
        }
        run.mean.push(values);
    }
    let disp = |id: &str| n.display_of(id);
    if let Some(m) = &res.metrics {
        run.metrics = Some(json!({
            "throughput": sorted_nums(m.throughput.iter().map(|(k, v)| (k.clone(), *v))),
            "mean": sorted_nums(m.mean.iter().map(|(k, v)| (disp(k), *v))),
            "p95": sorted_nums(m.p95.iter().map(|(k, v)| (disp(k), *v))),
            "utilization": sorted_nums(m.utilization.iter().map(|(k, v)| (k.clone(), *v))),
            "inFlight": sorted_nums(m.in_flight.iter().map(|(k, v)| (k.clone(), *v))),
        }));
    }
    run.depleted = res
        .depleted
        .iter()
        .map(|d| json!({"place": disp(&d.place), "at": num(d.at), "recovered": d.recovered}))
        .collect();
    for c in &res.contended {
        let v = json!({
            "place": disp(&c.place),
            "fraction": num(c.fraction),
            "kind": format!("{:?}", c.kind).to_lowercase(),
            "blocking": c.blocking,
        });
        if c.kind.is_capacity() {
            run.contended.push(v);
        } else {
            run.waiting.push(v);
        }
    }
    Ok(run)
}

/// Native's `ScenarioRun::summary`.
fn summary(n: &WbNet, run: &ScenarioRun, sc: &Scenario) -> Value {
    let mut finals = Map::new();
    let mut sds = Map::new();
    for (d, f) in n.display.iter().zip(&run.finals) {
        finals.insert(d.clone(), num(f.mean));
        if let Some(sd) = f.sd {
            sds.insert(d.clone(), num(sd));
        }
    }
    let mut out = json!({
        "name": sc.name,
        "engine": "ssa",
        "hours": num(sc.hours),
        "samples": sc.samples,
        "realizations": sc.realizations,
        "seed": sc.seed,
        "final": finals,
    });
    if !sds.is_empty() {
        out["finalStdDev"] = Value::Object(sds);
    }
    if let Some(m) = &run.metrics {
        out["metrics"] = m.clone();
    }
    out["depleted"] = json!(run.depleted);
    out["contended"] = json!(run.contended);
    out["waitingForWork"] = json!(run.waiting);
    out["caveats"] = json!(run.caveats);
    out["assumptions"] = json!(run.assumptions);
    out
}

/// Native's `finals_table` (what `sim.scenario` stores).
fn finals_table(n: &WbNet, run: &ScenarioRun) -> Value {
    let metric = |k: &str, p: &str| -> Value {
        run.metrics
            .as_ref()
            .and_then(|m| m.get(k))
            .and_then(|m| m.get(p))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let rows: Vec<Value> = n
        .display
        .iter()
        .zip(&run.finals)
        .map(|(p, f)| {
            let depleted = run
                .depleted
                .iter()
                .find(|d| d["place"] == json!(p))
                .map(|d| d["at"].clone())
                .unwrap_or(Value::Null);
            json!([
                p,
                num(f.mean),
                f.sd.map_or(Value::Null, num),
                num(f.min),
                num(f.max),
                metric("mean", p),
                metric("p95", p),
                depleted,
            ])
        })
        .collect();
    json!({
        "kind": "table",
        "columns": ["place", "final_mean", "final_sd", "final_min", "final_max", "time_mean", "p95", "depleted_at"],
        "rows": rows,
    })
}

/// Bounds the answer in bytes as a function of the samples, with every
/// number taking `num` bytes. With `lists` (the upper bound) it also allows
/// for the most the engine can report per place or transition (depletion,
/// contention, utilization, in-flight firings, caveats) and for the keys and
/// a note; without (the lower bound) it counts only what every answer
/// carries.
fn what_if_size(
    n: &WbNet,
    keep: &[bool],
    band: bool,
    arms: &[String],
    place: &str,
    num: i64,
    lists: bool,
) -> impl Fn(i64) -> i64 {
    let b = if band { 2 } else { 1 };
    let places = n.places.len() as i64;
    let (mut names, mut keyed) = (0i64, 0i64); // Σ name, Σ "name":number,
    for d in &n.display {
        names += go_str_len(d);
        keyed += go_str_len(d) + 1 + num;
    }
    let (mut trans_keyed, mut blocking, mut text) = (0i64, 0i64, 1500i64);
    for t in &n.model.transitions {
        let id = go_str_len(&t.id);
        trans_keyed += id + 1 + num;
        // A contention's blocking list names transitions that draw on the
        // place: at most one entry per input or test arc.
        blocking += (n.model.inputs(&t.id).len() + n.model.tests(&t.id).len()) as i64 * (id + 1);
        // Caveats and the service assumption name delayed, staged and
        // guarded transitions.
        if t.delay > 0.0 || t.stages > 1 || !t.guard.is_empty() {
            text += 600 + 2 * id;
        }
    }
    let mut fixed = go_str_len(place) + 3 * num + names; // seed, place, from, to; the table's columns
    let mut per_point = num; // x
    for arm in arms {
        let a = go_str_len(arm);
        fixed += a + 4 * num; // a summary's name, hours, samples, realizations, seed
        fixed += keyed * b; // final, finalStdDev
        fixed += 2 * keyed + trans_keyed; // metrics: mean, p95, throughput
        fixed += names + places * (7 * num); // the finals table
        fixed += a + places * num; // the scenario × place table's row
        if lists {
            fixed += 320 + 140 + 8; // the keys of a summary, a finals table, a row
            fixed += keyed + trans_keyed; // metrics: utilization, inFlight
            fixed += names + places * (40 + num); // depleted
            fixed += names + places * (70 + num) + blocking; // contended, waitingForWork
            fixed += text + places * 4; // caveats, assumptions; a finals row's brackets
        }
        for (i, d) in n.display.iter().enumerate() {
            if keep[i] {
                fixed += go_str_len(d) + a; // a series' name and group
                if lists {
                    fixed += 42; // {"name":…,"values":[],"group":…,"band":[]},
                }
                per_point += num * b;
            }
        }
    }
    if lists {
        fixed += 900 + places; // keys, a note; the table's column separators
    }
    move |k| fixed + per_point * k
}
