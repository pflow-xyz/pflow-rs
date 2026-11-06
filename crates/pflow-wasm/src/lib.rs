//! pflow-rs for the browser, packaged on npm as `@pflow-xyz/pflow-wasm`.
//!
//! This crate adds no engine behaviour, only the JavaScript boundary: firing
//! is `pflow_metamodel`'s rule, the ODE is `pflow_solver::solve` with Tsit5,
//! the stochastic run is `pflow_solver::ssa::simulate` (the portable,
//! byte-exact Gillespie SSA), and the state-space search is
//! `pflow_reachability::Analyzer` (bounded BFS, the L1/L4 liveness verdicts
//! and a replayable witness, as go-pflow's `reachability`).
//!
//! The logic lives in [`Core`], plain Rust over JSON text returning
//! `Result<String, String>`, so it is tested natively (`cargo test -p
//! pflow-wasm`); the `wasm_bindgen` methods on [`Net`] only convert values.
//! `crates/pflow-wasm/npm/test.mjs` runs the same golden fixtures through the
//! built package.

use std::collections::{BTreeMap, HashMap, HashSet};

use pflow_core::PetriNet;
use pflow_metamodel::{ArcType, Marking, Model};
use pflow_reachability::{Analyzer, Verdict, WitnessKind};
use pflow_solver::ssa::{
    simulate as simulate_ssa, ArcKind, SsaArc, SsaModel, SsaOptions, SsaPlace, SsaTransition,
};
use pflow_solver::{methods, solve, stochastic, ContextualArcs, Options, Problem};
use serde::Deserialize;
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

/// Reads a model: Shape A (the pflow.xyz editor document) or Shape B
/// (`pflow_metamodel::Model`), and refuses what these engines would otherwise
/// get silently wrong. Also returns the transitions whose rate is an explicit
/// `0`: off, where an absent or `null` rate is unset (the typed model keeps
/// both as 0, which the engines read as the default rate).
fn parse_model(text: &str) -> Result<(Model, HashSet<String>), String> {
    let bytes = text.as_bytes();
    let mut off = HashSet::new();
    let model = if pflow_parser::is_pflow_json(bytes) {
        let mut model = pflow_parser::model_from_json(bytes)
            .map_err(|e| format!("Shape A parse error: {e}"))?
            .0;
        // `model_from_json` drops a transition's `rate`; the hosted tools and
        // go-pflow read it, so the local engines do too.
        if let Ok(Value::Object(doc)) = serde_json::from_str::<Value>(text) {
            if let Some(Value::Object(ts)) = doc.get("transitions") {
                for t in &mut model.transitions {
                    match ts.get(&t.id).and_then(|n| n.get("rate")).and_then(Value::as_f64) {
                        Some(r) if r == 0.0 => {
                            off.insert(t.id.clone());
                        }
                        Some(r) if r > 0.0 && r.is_finite() => t.rate = r,
                        // the array shape refuses a negative rate; the document shape must too
                        Some(r) => return Err(format!("transition {:?}: bad rate {}", t.id, r)),
                        None => {}
                    }
                }
            }
            whole_weights_a(&doc)?;
        }
        model
    } else {
        let mut v: Value = serde_json::from_str(text)
            .map_err(|e| format!("not a recognized model (Shape A or Shape B JSON): {e}"))?;
        prepare_shape_b(&mut v, &mut off)?;
        serde_json::from_value::<Model>(v)
            .map_err(|e| format!("not a recognized model (Shape A or Shape B JSON): {e}"))?
    };
    check_model(&model)?;
    Ok((model, off))
}

/// Shape B as the typed model reads it: a missing `name` is empty, a whole
/// weight written `2.0` is 2, a fractional one is refused with the arc named,
/// a `null` rate is absent and a rate of `0` is noted in `off`.
fn prepare_shape_b(v: &mut Value, off: &mut HashSet<String>) -> Result<(), String> {
    let Some(obj) = v.as_object_mut() else { return Ok(()) };
    if obj.get("name").is_none_or(Value::is_null) {
        obj.insert("name".into(), json!(""));
    }
    if let Some(Value::Array(arcs)) = obj.get_mut("arcs") {
        for (i, a) in arcs.iter_mut().enumerate() {
            let Some(a) = a.as_object_mut() else { continue };
            if let Some(w) = a.get("weight").and_then(Value::as_f64) {
                if w.fract() != 0.0 || !w.is_finite() {
                    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("?").to_string();
                    return Err(format!(
                        "arcs[{i}] {:?} -> {:?}: weight {w} is not a whole number (a token count)",
                        s("from"),
                        s("to")
                    ));
                }
                a.insert("weight".into(), json!(w as i64));
            }
        }
    }
    if let Some(Value::Array(ts)) = obj.get_mut("transitions") {
        for t in ts.iter_mut().filter_map(Value::as_object_mut) {
            if t.get("rate").is_some_and(Value::is_null) {
                t.remove("rate");
            }
            if t.get("rate").and_then(Value::as_f64) == Some(0.0) {
                if let Some(id) = t.get("id").and_then(Value::as_str) {
                    off.insert(id.to_string());
                }
            }
        }
    }
    Ok(())
}

/// A Shape A arc whose weight is not a whole number: `model_from_json`
/// truncates it with `int(...)`, which turns a half-token arc into none.
fn whole_weights_a(doc: &serde_json::Map<String, Value>) -> Result<(), String> {
    if let Some(Value::Array(arcs)) = doc.get("arcs") {
        for (i, a) in arcs.iter().enumerate() {
            let sum: f64 = match a.get("weight") {
                Some(Value::Array(w)) => w.iter().filter_map(Value::as_f64).sum(),
                Some(Value::Number(n)) => n.as_f64().unwrap_or(1.0),
                _ => 1.0,
            };
            if sum.fract() != 0.0 {
                let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("?").to_string();
                return Err(format!(
                    "arcs[{i}] {:?} -> {:?}: weight {sum} is not a whole number (a token count)",
                    s("source"),
                    s("target")
                ));
            }
        }
    }
    Ok(())
}

/// The firing rule does not evaluate guards and the portable SSA has no stages
/// or schedules, so a net using them would run as a different net with no
/// error. Refusing keeps every answer this package gives the engine's. An arc
/// must also join a known place and a known transition: the engines assume it.
fn check_model(model: &Model) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut places = HashSet::new();
    let mut transitions = HashSet::new();
    for p in &model.places {
        if !ids.insert(p.id.as_str()) {
            return Err(format!("duplicate id {:?}", p.id));
        }
        if p.initial < 0 {
            return Err(format!("place {:?}: negative initial marking", p.id));
        }
        if p.capacity < 0 {
            return Err(format!("place {:?}: negative capacity", p.id));
        }
        places.insert(p.id.as_str());
    }
    for t in &model.transitions {
        if !ids.insert(t.id.as_str()) {
            return Err(format!("duplicate id {:?}", t.id));
        }
        if !t.guard.is_empty() {
            return Err(format!(
                "transition {:?}: guards are not supported by pflow-wasm",
                t.id
            ));
        }
        if t.stages > 1 {
            return Err(format!(
                "transition {:?}: stages are not supported by pflow-wasm",
                t.id
            ));
        }
        if !t.schedule.is_empty() {
            return Err(format!(
                "transition {:?}: rate schedules are not supported by pflow-wasm",
                t.id
            ));
        }
        if !t.rate.is_finite() || t.rate < 0.0 {
            return Err(format!("transition {:?}: bad rate {}", t.id, t.rate));
        }
        transitions.insert(t.id.as_str());
    }
    for a in &model.arcs {
        let joined = (places.contains(a.from.as_str()) && transitions.contains(a.to.as_str()))
            || (transitions.contains(a.from.as_str()) && places.contains(a.to.as_str()));
        if !joined {
            return Err(format!(
                "arc {:?} -> {:?} does not join a place and a transition",
                a.from, a.to
            ));
        }
        if a.weight < 0 {
            return Err(format!("arc {:?} -> {:?}: negative weight", a.from, a.to));
        }
    }
    Ok(())
}

/// `pflow_core::PetriNet` for the ODE, as `pflow-mcp`'s
/// `convert::model_to_petri_net` builds it (that crate is a binary with a
/// server's dependencies, so it cannot be linked here). An inhibitor stays
/// `place -> transition`, a read arc is drawn `transition -> place`.
fn petri_net(model: &Model, marking: &Marking) -> PetriNet {
    let mut net = PetriNet::new();
    let mut tokens = HashSet::new();
    for p in model.places.iter().filter(|p| p.is_token()) {
        let m = marking.get(&p.id).copied().unwrap_or(0) as f64;
        net.add_place(&p.id, vec![m], vec![], 0.0, 0.0, None);
        tokens.insert(p.id.as_str());
    }
    for t in &model.transitions {
        net.add_transition(&t.id, "", 0.0, 0.0, None);
    }
    for a in &model.arcs {
        let w = a.effective_weight() as f64;
        match a.typ {
            ArcType::Normal => {
                if tokens.contains(a.from.as_str()) || tokens.contains(a.to.as_str()) {
                    net.add_arc(&a.from, &a.to, vec![w], false);
                }
            }
            ArcType::Inhibitor if tokens.contains(a.from.as_str()) => {
                net.add_arc(&a.from, &a.to, vec![w], true);
            }
            ArcType::Read if tokens.contains(a.from.as_str()) => {
                net.add_arc(&a.to, &a.from, vec![w], true);
            }
            ArcType::Inhibitor | ArcType::Read => {}
        }
    }
    net
}

/// The portable SSA's ordered model. Declaration order is normative, so this
/// goes straight from `Model`'s arrays rather than through `PetriNet`'s maps.
fn ssa_model(model: &Model, marking: &Marking, rates: &HashMap<String, f64>) -> SsaModel {
    let tokens: HashSet<&str> = model
        .places
        .iter()
        .filter(|p| p.is_token())
        .map(|p| p.id.as_str())
        .collect();
    // The portable SSA reads rate 0 as the default 1, so a transition that is
    // off (rate 0) is left out with its arcs: it can never fire, which is what
    // a zero propensity is, and no random number is spent on it either way.
    let off: HashSet<&str> = model
        .transitions
        .iter()
        .filter(|t| t.delay <= 0.0 && rates.get(&t.id) == Some(&0.0))
        .map(|t| t.id.as_str())
        .collect();
    SsaModel {
        places: model
            .places
            .iter()
            .filter(|p| p.is_token())
            .map(|p| SsaPlace {
                id: p.id.clone(),
                initial: marking.get(&p.id).copied().unwrap_or(0),
                capacity: p.capacity,
            })
            .collect(),
        transitions: model
            .transitions
            .iter()
            .filter(|t| !off.contains(t.id.as_str()))
            .map(|t| SsaTransition {
                id: t.id.clone(),
                rate: rates.get(&t.id).copied().unwrap_or(t.rate),
                delay: t.delay,
            })
            .collect(),
        arcs: model
            .arcs
            .iter()
            .filter(|a| tokens.contains(a.from.as_str()) || tokens.contains(a.to.as_str()))
            .filter(|a| !off.contains(a.from.as_str()) && !off.contains(a.to.as_str()))
            .map(|a| SsaArc {
                from: a.from.clone(),
                to: a.to.clone(),
                weight: a.weight,
                kind: match a.typ {
                    ArcType::Normal => ArcKind::Flow,
                    ArcType::Inhibitor => ArcKind::Inhibitor,
                    ArcType::Read => ArcKind::Read,
                },
                kinetic: a.is_kinetic(),
            })
            .collect(),
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct SimulateOptions {
    t_end: Option<f64>,
    max_points: Option<usize>,
    rates: HashMap<String, Option<f64>>,
    contextual: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct StochasticOptions {
    horizon: Option<f64>,
    samples: Option<usize>,
    realizations: Option<usize>,
    seed: Option<u64>,
    rates: HashMap<String, Option<f64>>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct ReachabilityOptions {
    max_states: Option<u64>,
    max_tokens: Option<i64>,
    from: Option<String>,
}

/// pflow.xyz/mcp's `pflow_reachability` budget: default 2000, at most 20000.
pub const REACH_DEFAULT_STATES: u64 = 2_000;
pub const REACH_MAX_STATES: u64 = 20_000;
/// go-pflow's per-place token bound, past which a net is called unbounded.
pub const REACH_DEFAULT_TOKENS: i64 = 1_000;

fn verdict_value(v: Verdict) -> Value {
    match v {
        Verdict::True => json!(true),
        Verdict::False => json!(false),
        _ => json!("unknown"),
    }
}

fn sorted_marking(m: &Marking) -> BTreeMap<&str, i64> {
    m.iter().map(|(k, v)| (k.as_str(), *v)).collect()
}

fn parse_options<T: for<'a> Deserialize<'a> + Default>(
    text: &str,
    what: &str,
) -> Result<T, String> {
    if text.trim().is_empty() {
        return Ok(T::default());
    }
    serde_json::from_str(text).map_err(|e| format!("{what}: bad options: {e}"))
}

/// The native core of [`Net`]: a model and its current marking.
pub struct Core {
    model: Model,
    marking: Marking,
    /// Transitions whose rate the model sets to an explicit 0: off.
    off: HashSet<String>,
}

impl Core {
    pub fn new(model_json: &str) -> Result<Core, String> {
        let (model, off) = parse_model(model_json)?;
        let marking = model.initial_marking();
        Ok(Core { model, marking, off })
    }

    fn marking_map(&self) -> BTreeMap<&str, i64> {
        self.model
            .places
            .iter()
            .filter(|p| p.is_token())
            .map(|p| (p.id.as_str(), self.marking.get(&p.id).copied().unwrap_or(0)))
            .collect()
    }

    pub fn places_json(&self) -> String {
        let ids: Vec<&str> = self
            .model
            .places
            .iter()
            .filter(|p| p.is_token())
            .map(|p| p.id.as_str())
            .collect();
        json!(ids).to_string()
    }

    pub fn transitions_json(&self) -> String {
        let ids: Vec<&str> = self
            .model
            .transitions
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        json!(ids).to_string()
    }

    pub fn marking_json(&self) -> String {
        json!(self.marking_map()).to_string()
    }

    pub fn enabled_json(&self) -> String {
        json!(self.model.enabled_transitions(&self.marking)).to_string()
    }

    /// Fires `transition` at the current marking and returns the new marking.
    /// A refusal, with the firing rule's reason, leaves the marking unchanged.
    pub fn fire_json(&mut self, transition: &str) -> Result<String, String> {
        self.model
            .enabled_why_not(transition, &self.marking)
            .map_err(|e| e.to_string())?;
        self.marking = self.model.fire(transition, &self.marking);
        Ok(self.marking_json())
    }

    pub fn reset(&mut self) {
        self.marking = self.model.initial_marking();
    }

    /// Bounded reachability: the state space from the initial marking (or,
    /// with `from: "current"`, the current one), and go-pflow's three
    /// liveness verdicts over it. Rates play no part. The response is
    /// pflow.xyz/mcp `pflow_reachability`'s, key for key: each verdict is
    /// `true`, `false` or `"unknown"`; nothing is `true` unless `complete`;
    /// `witness` is a firing sequence that replays from the start marking to a
    /// dead marking or into a closed region `never_fires` names.
    pub fn reachability_json(&self, options_json: &str) -> Result<String, String> {
        let o: ReachabilityOptions = parse_options(options_json, "reachability")?;
        let max_states = o.max_states.unwrap_or(REACH_DEFAULT_STATES);
        if !(1..=REACH_MAX_STATES).contains(&max_states) {
            return Err(format!(
                "reachability: maxStates must be between 1 and {REACH_MAX_STATES}"
            ));
        }
        let max_tokens = o.max_tokens.unwrap_or(REACH_DEFAULT_TOKENS);
        if max_tokens < 1 {
            return Err("reachability: maxTokens must be at least 1".into());
        }
        let start = match o.from.as_deref() {
            None | Some("initial") => self.model.initial_marking(),
            Some("current") => self.marking.clone(),
            Some(other) => {
                return Err(format!(
                    "reachability: from must be \"initial\" or \"current\", not {other:?}"
                ))
            }
        };
        let r = Analyzer::new(&self.model)
            .with_initial_marking(start.clone())
            .with_max_states(max_states as usize)
            .with_max_tokens(max_tokens)
            .analyze();
        let lv = &r.liveness;
        let witness = lv.witness.as_ref().map(|w| {
            json!({
                "kind": match w.kind {
                    WitnessKind::Deadlock => "deadlock",
                    WitnessKind::Livelock => "livelock",
                },
                "trace": w.trace,
                "marking": sorted_marking(&w.marking),
                "region_size": w.region_size,
                "never_fires": w.never_fires,
            })
        });
        let mut out = json!({
            "engine": "pflow-rs pflow-reachability (wasm)",
            "from": sorted_marking(&start),
            "states": r.state_count,
            "edges": r.edge_count,
            "complete": r.is_complete,
            "bounded": r.bounded,
            "max_tokens": r.max_tokens.iter().map(|(k, v)| (k.as_str(), *v)).collect::<BTreeMap<_, _>>(),
            "deadlocks": r.dead_markings.len(),
            "deadlock_free": verdict_value(lv.deadlock_free),
            "live": verdict_value(lv.live),
            "quasi_live": verdict_value(lv.quasi_live),
            "witness": witness,
            "not_live": lv.not_live,
            "dead_transitions": r.dead_trans,
            "note": "untimed: rates play no part; a claim about every state holds only when complete is true",
        });
        if r.truncated {
            out["truncated"] = json!(r.truncate_msg);
            out["bound"] = json!({
                "max_states": r.exploration_stats.states_limit,
                "max_tokens": r.exploration_stats.tokens_limit,
            });
        }
        if !r.is_complete {
            // The analyzer's `bounded` covers only the states it saw.
            out["bounded"] = json!("unknown: the search stopped at max_states");
        }
        if let Some(first) = r
            .dead_markings
            .first()
            .and_then(|k| r.graph.get_state_by_key(k))
        {
            out["first_deadlock"] = json!(sorted_marking(&first.marking));
        }
        Ok(out.to_string())
    }

    /// The model's own rates with `overrides` on top. A rate for a transition
    /// the net does not have is an error, not something to ignore.
    ///
    /// A rate of 0 is off: the transition never fires in `simulate` and
    /// `stochastic`. `null` (or no entry) is unset: the model's own rate, else
    /// 1. (The portable SSA reads 0 as "default", so `ssa_model` drops an off
    /// transition instead of passing its 0 on.)
    fn rates(&self, overrides: &HashMap<String, Option<f64>>) -> Result<HashMap<String, f64>, String> {
        let mut rates = stochastic::rates(&self.model);
        for id in &self.off {
            rates.insert(id.clone(), 0.0);
        }
        for (id, r) in overrides {
            let Some(r) = r else {
                // unset: back to the model's own
                if self.off.contains(id) {
                    rates.insert(id.clone(), 0.0);
                } else if self.model.transition_by_id(id).is_none() {
                    return Err(format!("rates: no transition {id:?}"));
                }
                continue;
            };
            if self.model.transition_by_id(id).is_none() {
                return Err(format!("rates: no transition {id:?}"));
            }
            if !r.is_finite() || *r < 0.0 {
                return Err(format!("rates: bad rate {r} for {id:?}"));
            }
            rates.insert(id.clone(), *r);
        }
        Ok(rates)
    }

    /// Mass-action ODE (Tsit5) from the current marking.
    pub fn simulate_json(&self, options_json: &str) -> Result<String, String> {
        let o: SimulateOptions = parse_options(options_json, "simulate")?;
        let t_end = o.t_end.unwrap_or(100.0);
        if !t_end.is_finite() || t_end <= 0.0 {
            return Err("simulate: tEnd must be positive and finite".into());
        }
        let max_points = o.max_points.unwrap_or(200);
        if max_points < 2 {
            return Err("simulate: maxPoints must be >= 2".into());
        }
        let contextual: ContextualArcs = match &o.contextual {
            None => ContextualArcs::Refuse,
            Some(s) => s.parse().map_err(|e| format!("simulate: {e}"))?,
        };
        let rates = self.rates(&o.rates)?;

        let net = petri_net(&self.model, &self.marking);
        let state = net.set_state(None);
        let prob = Problem::with_contextual(net, state, [0.0, t_end], rates, contextual)
            .map_err(|e| e.to_string())?;
        let sol = solve(&prob, &methods::tsit5(), &Options::default_opts());

        let total = sol.t.len();
        let step = (total / max_points).max(1);
        let mut picks: Vec<usize> = (0..total).step_by(step).collect();
        if total > 0 && picks.last() != Some(&(total - 1)) {
            picks.push(total - 1);
        }
        let series: BTreeMap<&str, Vec<f64>> = sol
            .state_labels
            .iter()
            .map(|label| {
                let values = picks
                    .iter()
                    .map(|&i| sol.u[i].get(label).copied().unwrap_or(0.0))
                    .collect();
                (label.as_str(), values)
            })
            .collect();
        let time: Vec<f64> = picks.iter().map(|&i| sol.t[i]).collect();
        let final_state: BTreeMap<&str, f64> = sol
            .state_labels
            .iter()
            .map(|label| {
                let v = sol
                    .get_final_state()
                    .and_then(|s| s.get(label))
                    .copied()
                    .unwrap_or(0.0);
                (label.as_str(), v)
            })
            .collect();
        Ok(json!({
            "method": "tsit5",
            "places": sol.state_labels,
            "times": time,
            "series": series,
            "final": final_state,
            // What the continuous solution could not honour (capacity, delay).
            "gating": self.model.gating(),
        })
        .to_string())
    }

    /// The portable Gillespie SSA from the current marking: seeded, and
    /// byte-exact with go-pflow, pflow-xyz and pflow-jl at the same seed.
    pub fn stochastic_json(&self, options_json: &str) -> Result<String, String> {
        let o: StochasticOptions = parse_options(options_json, "stochastic")?;
        let defaults = SsaOptions::default();
        let opts = SsaOptions {
            horizon: o.horizon.unwrap_or(defaults.horizon),
            samples: o.samples.unwrap_or(defaults.samples),
            realizations: o.realizations.unwrap_or(defaults.realizations),
            seed: o.seed.unwrap_or(defaults.seed),
        };
        let rates = self.rates(&o.rates)?;
        let model = ssa_model(&self.model, &self.marking, &rates);
        let res = simulate_ssa(&model, &opts).map_err(|e| e.to_string())?;

        let mut series = serde_json::Map::new();
        let mut final_state = serde_json::Map::new();
        for (p, id) in res.places.iter().enumerate() {
            let mut entry = json!({ "values": res.values[p] });
            if let Some(stddev) = &res.stddev {
                entry["stddev"] = json!(stddev[p]);
            }
            series.insert(id.clone(), entry);
            final_state.insert(id.clone(), json!(res.final_[p]));
        }
        Ok(json!({
            "method": "ssa-portable",
            "seed": opts.seed,
            "places": res.places,
            "times": res.times,
            "series": Value::Object(series),
            "final": Value::Object(final_state),
        })
        .to_string())
    }
}

#[wasm_bindgen(typescript_custom_section)]
const TS_TYPES: &'static str = r#"
/** A marking: token count per place. */
export type Marking = Record<string, number>;
/** A net as JSON text or as the object itself: Shape A (pflow.xyz) or Shape B. */
export type ModelInput = string | object;
export interface SimulateOptions {
  /** End time; default 100. */
  tEnd?: number;
  /** Most points returned (the last is always kept); default 200. */
  maxPoints?: number;
  /** Rate overrides by transition id. */
  rates?: Record<string, number>;
  /** Read and inhibitor arcs: "refuse" (default), "selfloop" or "drop". */
  contextual?: "refuse" | "selfloop" | "drop";
}
export interface SimulateResult {
  method: "tsit5";
  places: string[];
  times: number[];
  series: Record<string, number[]>;
  final: Record<string, number>;
  /** Constraints the continuous solution could not honour. */
  gating: string[];
}
export interface StochasticOptions {
  /** End time; default 10. */
  horizon?: number;
  /** Grid points, at least 2; default 101. */
  samples?: number;
  /** Ensemble size; default 1. */
  realizations?: number;
  /** Seed; default 1. Same seed, same bytes as go-pflow. */
  seed?: number;
  /** Rate overrides by transition id. */
  rates?: Record<string, number>;
}
export interface ReachabilityOptions {
  /** State budget; default 2000, at most 20000. */
  maxStates?: number;
  /** Tokens in one place past which the net is called unbounded; default 1000. */
  maxTokens?: number;
  /** Explore from the "initial" marking (default) or the "current" one. */
  from?: "initial" | "current";
}
/** true, false, or "unknown" when the search stopped before seeing every state. */
export type Verdict = boolean | "unknown";
export interface ReachabilityWitness {
  kind: "deadlock" | "livelock";
  /** Fire these in order from the start marking to reach `marking`. */
  trace: string[];
  marking: Marking;
  /** Markings in the closed region; 1 for a dead marking. */
  region_size: number;
  /** Transitions that can never fire again once `marking` is reached. */
  never_fires: string[];
}
export interface ReachabilityResult {
  engine: string;
  from: Marking;
  states: number;
  edges: number;
  complete: boolean;
  bounded: boolean | string;
  max_tokens: Record<string, number>;
  deadlocks: number;
  deadlock_free: Verdict;
  /** L4: from every reachable marking, every transition can still fire. */
  live: Verdict;
  /** L1: every transition fires in some reachable marking. */
  quasi_live: Verdict;
  witness: ReachabilityWitness | null;
  not_live: string[];
  dead_transitions: string[];
  note: string;
  truncated?: string;
  bound?: { max_states: number; max_tokens: number };
  first_deadlock?: Marking;
}
export interface StochasticResult {
  method: "ssa-portable";
  seed: number;
  places: string[];
  times: number[];
  series: Record<string, { values: number[]; stddev?: number[] }>;
  final: Record<string, number>;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = JSON, js_name = parse, catch)]
    fn json_parse(text: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = JSON, js_name = stringify, catch)]
    fn json_stringify(value: &JsValue) -> Result<JsValue, JsValue>;
}

/// A JS argument as JSON text: a string as it is, anything else stringified.
/// `undefined` comes back empty.
fn to_text(value: &JsValue) -> Result<String, JsError> {
    if let Some(s) = value.as_string() {
        return Ok(s);
    }
    if value.is_undefined() || value.is_null() {
        return Ok(String::new());
    }
    json_stringify(value)
        .map_err(|_| JsError::new("argument cannot be converted to JSON"))
        .map(|s| s.as_string().unwrap_or_default())
}

fn to_js(text: &str) -> Result<JsValue, JsError> {
    json_parse(text).map_err(|_| JsError::new("internal: result is not JSON"))
}

/// A Petri net and its current marking.
#[wasm_bindgen]
pub struct Net {
    core: Core,
}

#[wasm_bindgen]
impl Net {
    /// Loads a net from Shape A (pflow.xyz) or Shape B JSON, as text or as an
    /// object. Throws on a malformed net, or on one using guards, stages or
    /// rate schedules, which this build does not run.
    #[wasm_bindgen(constructor)]
    pub fn new(
        #[wasm_bindgen(unchecked_param_type = "ModelInput")] model: JsValue,
    ) -> Result<Net, JsError> {
        let text = to_text(&model)?;
        let core = Core::new(&text).map_err(|e| JsError::new(&e))?;
        Ok(Net { core })
    }

    /// Token place ids, in declaration order.
    #[wasm_bindgen(unchecked_return_type = "string[]")]
    pub fn places(&self) -> Result<JsValue, JsError> {
        to_js(&self.core.places_json())
    }

    /// Transition ids, in declaration order.
    #[wasm_bindgen(unchecked_return_type = "string[]")]
    pub fn transitions(&self) -> Result<JsValue, JsError> {
        to_js(&self.core.transitions_json())
    }

    /// The current marking.
    #[wasm_bindgen(unchecked_return_type = "Marking")]
    pub fn marking(&self) -> Result<JsValue, JsError> {
        to_js(&self.core.marking_json())
    }

    /// The transitions that can fire at the current marking.
    #[wasm_bindgen(unchecked_return_type = "string[]")]
    pub fn enabled(&self) -> Result<JsValue, JsError> {
        to_js(&self.core.enabled_json())
    }

    /// Fires one transition and returns the new marking. Throws, with the
    /// reason, if it is not enabled; the marking is then unchanged.
    #[wasm_bindgen(unchecked_return_type = "Marking")]
    pub fn fire(&mut self, transition: &str) -> Result<JsValue, JsError> {
        let text = self
            .core
            .fire_json(transition)
            .map_err(|e| JsError::new(&e))?;
        to_js(&text)
    }

    /// Back to the initial marking.
    pub fn reset(&mut self) {
        self.core.reset();
    }

    /// Mass-action ODE (Tsit5) from the current marking. Does not move it.
    #[wasm_bindgen(unchecked_return_type = "SimulateResult")]
    pub fn simulate(
        &self,
        #[wasm_bindgen(unchecked_param_type = "SimulateOptions | undefined")] options: JsValue,
    ) -> Result<JsValue, JsError> {
        let text = self
            .core
            .simulate_json(&to_text(&options)?)
            .map_err(|e| JsError::new(&e))?;
        to_js(&text)
    }

    /// Bounded reachability with the deadlock-free, L1 and L4 verdicts and a
    /// replayable witness. Does not move the marking.
    #[wasm_bindgen(unchecked_return_type = "ReachabilityResult")]
    pub fn reachability(
        &self,
        #[wasm_bindgen(unchecked_param_type = "ReachabilityOptions | undefined")] options: JsValue,
    ) -> Result<JsValue, JsError> {
        let text = self
            .core
            .reachability_json(&to_text(&options)?)
            .map_err(|e| JsError::new(&e))?;
        to_js(&text)
    }

    /// Portable Gillespie SSA from the current marking. Does not move it.
    #[wasm_bindgen(unchecked_return_type = "StochasticResult")]
    pub fn stochastic(
        &self,
        #[wasm_bindgen(unchecked_param_type = "StochasticOptions | undefined")] options: JsValue,
    ) -> Result<JsValue, JsError> {
        let text = self
            .core
            .stochastic_json(&to_text(&options)?)
            .map_err(|e| JsError::new(&e))?;
        to_js(&text)
    }
}

/// The package's version, which is the pflow-rs workspace's.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Runs one pflow.xyz/mcp tool locally (`pflow_hosted::call`): `tool` is the
/// hosted name without `pflow_`, `args` the tool's arguments as JSON text or an
/// object. Returns the answer as an object; throws the tool's refusal. Only in
/// the `hosted` build, which @pflow-xyz/pflow-mcp ships.
#[cfg(feature = "hosted")]
#[wasm_bindgen]
pub fn hosted(tool: &str, args: JsValue) -> Result<JsValue, JsError> {
    let text = to_text(&args)?;
    let args: serde_json::Value = if text.is_empty() {
        serde_json::Value::Object(Default::default())
    } else {
        serde_json::from_str(&text).map_err(|e| JsError::new(&format!("arguments: {e}")))?
    };
    let out = pflow_hosted::call(tool, &args).map_err(|e| JsError::new(&e))?;
    to_js(&out.to_string())
}

/// The tools `hosted` answers.
#[cfg(feature = "hosted")]
#[wasm_bindgen(js_name = hostedTools, unchecked_return_type = "string[]")]
pub fn hosted_tools() -> Result<JsValue, JsError> {
    to_js(&serde_json::to_string(pflow_hosted::TOOLS).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAIN: &str = r#"{"name":"chain",
        "places":[{"id":"a","initial":3},{"id":"b"}],
        "transitions":[{"id":"t","rate":0.5}],
        "arcs":[{"from":"a","to":"t"},{"from":"t","to":"b"}]}"#;

    fn parsed(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn fire_moves_tokens_and_refuses_with_the_reason() {
        let mut c = Core::new(CHAIN).unwrap();
        assert_eq!(parsed(&c.places_json()), json!(["a", "b"]));
        assert_eq!(parsed(&c.transitions_json()), json!(["t"]));
        assert_eq!(parsed(&c.enabled_json()), json!(["t"]));
        for _ in 0..3 {
            c.fire_json("t").unwrap();
        }
        assert_eq!(parsed(&c.marking_json()), json!({"a": 0, "b": 3}));
        assert_eq!(parsed(&c.enabled_json()), json!([]));

        let refused = c.fire_json("t").unwrap_err();
        assert!(refused.contains('a'), "names the short place: {refused}");
        assert_eq!(parsed(&c.marking_json()), json!({"a": 0, "b": 3}));
        assert!(c.fire_json("nope").is_err());

        c.reset();
        assert_eq!(parsed(&c.marking_json()), json!({"a": 3, "b": 0}));
    }

    #[test]
    fn shape_a_loads_and_fires() {
        let shape_a = r#"{"@type":"PetriNet",
            "places":{"a":{"initial":[2],"x":0,"y":0},"b":{"initial":[0],"x":0,"y":0}},
            "transitions":{"t":{"x":0,"y":0}},
            "arcs":[{"source":"a","target":"t","weight":[1]},
                    {"source":"t","target":"b","weight":[1]}]}"#;
        let mut c = Core::new(shape_a).unwrap();
        c.fire_json("t").unwrap();
        assert_eq!(parsed(&c.marking_json()), json!({"a": 1, "b": 1}));
    }

    #[test]
    fn ode_conserves_and_starts_from_the_current_marking() {
        let mut c = Core::new(CHAIN).unwrap();
        let r = parsed(&c.simulate_json(r#"{"tEnd":50,"maxPoints":10}"#).unwrap());
        assert!(r["times"].as_array().unwrap().len() <= 12);
        assert_eq!(r["times"][0], 0.0);
        let (a, b) = (
            r["final"]["a"].as_f64().unwrap(),
            r["final"]["b"].as_f64().unwrap(),
        );
        assert!((a + b - 3.0).abs() < 1e-6, "{a} + {b}");
        assert!(a < 1e-3, "drained: {a}");

        c.fire_json("t").unwrap();
        let r = parsed(&c.simulate_json("").unwrap());
        assert_eq!(r["series"]["a"][0], 2.0);
        // A run does not move the marking.
        assert_eq!(parsed(&c.marking_json()), json!({"a": 2, "b": 1}));
    }

    #[test]
    fn ode_refuses_contextual_arcs_unless_told() {
        let gated = r#"{"name":"n","places":[{"id":"a","initial":1},{"id":"g","initial":1},{"id":"b"}],
            "transitions":[{"id":"t"}],
            "arcs":[{"from":"a","to":"t"},{"from":"t","to":"b"},
                    {"from":"g","to":"t","type":"read"}]}"#;
        let c = Core::new(gated).unwrap();
        assert!(c.simulate_json("").is_err());
        assert!(c.simulate_json(r#"{"contextual":"selfloop"}"#).is_ok());
        assert!(c.simulate_json(r#"{"contextual":"nonsense"}"#).is_err());
    }

    /// `==` on every double, whichever way JSON spelled it (`0` or `0.0`).
    fn same_doubles(got: &Value, want: &Value, name: &str) {
        match (got, want) {
            (Value::Number(g), Value::Number(w)) => {
                assert_eq!(g.as_f64().unwrap(), w.as_f64().unwrap(), "{name}");
            }
            (Value::Array(g), Value::Array(w)) => {
                assert_eq!(g.len(), w.len(), "{name}");
                for (g, w) in g.iter().zip(w) {
                    same_doubles(g, w, name);
                }
            }
            (Value::Object(g), Value::Object(w)) => {
                assert_eq!(g.len(), w.len(), "{name}: {g:?} / {w:?}");
                for (k, w) in w {
                    same_doubles(&g[k], w, name);
                }
            }
            _ => panic!("{name}: shapes differ: {got} / {want}"),
        }
    }

    /// Every portable golden through this crate's own path: Shape B text in,
    /// `==` on every double out. The same fixtures run through the built
    /// package in `npm/test.mjs`.
    #[test]
    fn stochastic_is_byte_exact_with_the_goldens() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../pflow-solver/tests/fixtures/ssa"
        );
        for name in ["chain", "sir", "dimer", "coffeeshop", "gates", "timed"] {
            let text = std::fs::read_to_string(format!("{dir}/{name}.json")).unwrap();
            let golden = parsed(&text);
            let c = Core::new(&golden["model"].to_string()).unwrap();
            let got = parsed(&c.stochastic_json(&golden["options"].to_string()).unwrap());
            let want = &golden["expected"];
            same_doubles(&got["times"], &want["times"], name);
            same_doubles(&got["final"], &want["final"], name);
            same_doubles(&got["series"], &want["series"], name);
        }
    }

    #[test]
    fn refuses_what_the_engines_would_get_wrong() {
        let cases = [
            (
                "guard",
                r#"{"name":"n","places":[{"id":"a"}],"transitions":[{"id":"t","guard":"a > 1"}],"arcs":[]}"#,
            ),
            (
                "stages",
                r#"{"name":"n","places":[{"id":"a"}],"transitions":[{"id":"t","stages":3}],"arcs":[]}"#,
            ),
            (
                "join",
                r#"{"name":"n","places":[{"id":"a"}],"transitions":[{"id":"t"}],"arcs":[{"from":"a","to":"zz"}]}"#,
            ),
            (
                "join",
                r#"{"name":"n","places":[{"id":"a"},{"id":"b"}],"transitions":[],"arcs":[{"from":"a","to":"b"}]}"#,
            ),
            (
                "duplicate",
                r#"{"name":"n","places":[{"id":"a"},{"id":"a"}],"transitions":[],"arcs":[]}"#,
            ),
            (
                "negative",
                r#"{"name":"n","places":[{"id":"a","initial":-1}],"transitions":[],"arcs":[]}"#,
            ),
            (
                "negative",
                r#"{"name":"n","places":[{"id":"a"}],"transitions":[{"id":"t"}],"arcs":[{"from":"a","to":"t","weight":-1}]}"#,
            ),
        ];
        for (needle, model) in cases {
            let err = Core::new(model)
                .err()
                .unwrap_or_else(|| panic!("accepted {model}"));
            assert!(err.contains(needle), "{err} should mention {needle}");
        }
        assert!(Core::new("not json").is_err());
    }

    #[test]
    fn refuses_bad_options_rather_than_ignoring_them() {
        let c = Core::new(CHAIN).unwrap();
        assert!(c.simulate_json(r#"{"tEndd":5}"#).is_err());
        assert!(c.simulate_json(r#"{"tEnd":-1}"#).is_err());
        assert!(c.simulate_json(r#"{"maxPoints":1}"#).is_err());
        assert!(c.simulate_json(r#"{"rates":{"nope":1}}"#).is_err());
        assert!(c.simulate_json(r#"{"rates":{"t":-1}}"#).is_err());
        assert!(c.stochastic_json(r#"{"samples":1}"#).is_err());
        assert!(c.stochastic_json(r#"{"seed":-3}"#).is_err());
        assert!(c.stochastic_json(r#"{"horizon":0}"#).is_err());
    }

    #[test]
    fn rate_overrides_change_the_answer() {
        let c = Core::new(CHAIN).unwrap();
        let slow = parsed(&c.simulate_json(r#"{"tEnd":1}"#).unwrap());
        let fast = parsed(&c.simulate_json(r#"{"tEnd":1,"rates":{"t":5}}"#).unwrap());
        assert!(fast["final"]["b"].as_f64() > slow["final"]["b"].as_f64());
    }

    const TWO: &str = r#"{"places":[{"id":"a","initial":5},{"id":"b"}],
      "transitions":[{"id":"d","rate":0},{"id":"e","rate":0.3}],
      "arcs":[{"from":"a","to":"d"},{"from":"d","to":"b"},{"from":"a","to":"e"},{"from":"e","to":"b"}]}"#;

    fn finals(json: &str) -> (f64, f64) {
        let v = parsed(json);
        (v["final"]["a"].as_f64().unwrap(), v["final"]["b"].as_f64().unwrap())
    }

    /// B4: a rate of 0 is off, null is unset, in both engines, from an override
    /// and from the model; B7: the model needs no `name`.
    #[test]
    fn rate_zero_is_off_and_null_is_unset() {
        let c = Core::new(TWO).unwrap(); // no name, d has "rate": 0
        // the model's own zero: d is off, e (0.3) drains a
        let (_, b) = finals(&c.stochastic_json(r#"{"horizon":200,"samples":2}"#).unwrap());
        assert_eq!(b, 5.0);
        let (a, _) = finals(&c.simulate_json(r#"{"tEnd":200,"maxPoints":2,"rates":{"e":0}}"#).unwrap());
        assert_eq!(a, 5.0, "both off: nothing moves in the ODE");
        let (a, b) = finals(&c.stochastic_json(r#"{"horizon":200,"samples":2,"rates":{"e":0}}"#).unwrap());
        assert_eq!((a, b), (5.0, 0.0), "both off: nothing fires in the SSA");
        // null is unset: e back to its own 0.3, d stays off
        let (a, _) = finals(&c.stochastic_json(r#"{"horizon":200,"samples":2,"rates":{"e":null}}"#).unwrap());
        assert_eq!(a, 0.0);
        // an override of a model-off transition turns it on
        let c2 = Core::new(r#"{"places":[{"id":"a","initial":5},{"id":"b"}],"transitions":[{"id":"d","rate":0}],"arcs":[{"from":"a","to":"d"},{"from":"d","to":"b"}]}"#).unwrap();
        let (_, b) = finals(&c2.stochastic_json(r#"{"horizon":200,"samples":2,"rates":{"d":2}}"#).unwrap());
        assert_eq!(b, 5.0);
        let (a, _) = finals(&c2.stochastic_json(r#"{"horizon":200,"samples":2}"#).unwrap());
        assert_eq!(a, 5.0);
        // absent rate is the default 1
        let c3 = Core::new(r#"{"places":[{"id":"a","initial":5},{"id":"b"}],"transitions":[{"id":"d"},{"id":"x","rate":null}],"arcs":[{"from":"a","to":"d"},{"from":"d","to":"b"}]}"#).unwrap();
        let (a, _) = finals(&c3.stochastic_json(r#"{"horizon":200,"samples":2}"#).unwrap());
        assert_eq!(a, 0.0);
    }

    /// A pflow.xyz net's transition `rate` is read, 0 included, as the hosted
    /// tools read it.
    #[test]
    fn shape_a_rates_are_read() {
        let doc = r#"{"@context":"https://pflow.xyz/schema","places":{"a":{"initial":[5]},"b":{"initial":[0]}},
          "transitions":{"d":{"rate":0},"e":{"rate":0.3}},
          "arcs":[{"source":"a","target":"d"},{"source":"d","target":"b"},{"source":"a","target":"e"},{"source":"e","target":"b"}]}"#;
        let c = Core::new(doc).unwrap();
        let (a, b) = finals(&c.stochastic_json(r#"{"horizon":200,"samples":2,"rates":{"e":0}}"#).unwrap());
        assert_eq!((a, b), (5.0, 0.0));
        let (a, _) = finals(&c.simulate_json(r#"{"tEnd":1,"maxPoints":2}"#).unwrap());
        assert!(a > 3.0, "e at 0.3 only: {a}");
    }

    /// A negative rate is refused in both shapes (the document shape used to treat it as unset).
    #[test]
    fn a_negative_rate_is_refused_in_both_shapes() {
        let e = Core::new(r#"{"places":[{"id":"a"}],"transitions":[{"id":"t","rate":-1}],"arcs":[{"from":"a","to":"t"}]}"#).err().unwrap();
        assert!(e.contains("bad rate -1"), "{e}");
        let e = Core::new(r#"{"@context":"https://pflow.xyz/schema","places":{"a":{}},"transitions":{"t":{"rate":-1}},"arcs":[{"source":"a","target":"t"}]}"#).err().unwrap();
        assert!(e.contains("bad rate -1"), "{e}");
    }

    /// B7: a fractional weight is refused with the arc named, in either shape.
    #[test]
    fn fractional_weights_are_refused_by_arc() {
        let e = Core::new(r#"{"places":[{"id":"a"}],"transitions":[{"id":"t"}],"arcs":[{"from":"a","to":"t"},{"from":"a","to":"t","weight":0.5}]}"#).err().unwrap();
        assert!(e.contains("arcs[1]") && e.contains("0.5"), "{e}");
        let e = Core::new(r#"{"@context":"https://pflow.xyz/schema","places":{"a":{}},"transitions":{"t":{}},"arcs":[{"source":"a","target":"t","weight":[0.5]}]}"#).err().unwrap();
        assert!(e.contains("arcs[0]") && e.contains("0.5"), "{e}");
        // 2.0 is a whole 2
        Core::new(r#"{"places":[{"id":"a"}],"transitions":[{"id":"t"}],"arcs":[{"from":"a","to":"t","weight":2.0}]}"#).unwrap();
    }

    // ---- reachability ----------------------------------------------------

    /// Fires `trace` from `from` with the firing rule and returns where it ends:
    /// a witness must replay.
    fn replay(net: &str, from: &Value, trace: &Value) -> Value {
        let mut c = Core::new(net).unwrap();
        for (k, v) in from.as_object().unwrap() {
            c.marking.insert(k.clone(), v.as_i64().unwrap());
        }
        for t in trace.as_array().unwrap() {
            c.fire_json(t.as_str().unwrap()).unwrap();
        }
        parsed(&c.marking_json())
    }

    /// p -> t -> q -> u -> p: live, bounded, no deadlock.
    const CYCLE: &str = r#"{"name":"cycle","places":[{"id":"p","initial":1},{"id":"q"}],
        "transitions":[{"id":"t"},{"id":"u"}],
        "arcs":[{"from":"p","to":"t"},{"from":"t","to":"q"},{"from":"q","to":"u"},{"from":"u","to":"p"}]}"#;

    /// a choice between a loop (t, u) and a one-way exit into a loop (x, then v, w)
    /// that shuts t and u out: quasi-live and deadlock-free, but not L4-live.
    const LIVELOCK: &str = r#"{"name":"livelock",
        "places":[{"id":"p","initial":1},{"id":"q"},{"id":"r"},{"id":"s"}],
        "transitions":[{"id":"t"},{"id":"u"},{"id":"x"},{"id":"v"},{"id":"w"}],
        "arcs":[{"from":"p","to":"t"},{"from":"t","to":"q"},{"from":"q","to":"u"},{"from":"u","to":"p"},
                {"from":"p","to":"x"},{"from":"x","to":"r"},
                {"from":"r","to":"v"},{"from":"v","to":"s"},{"from":"s","to":"w"},{"from":"w","to":"r"}]}"#;

    /// t makes a token from nothing: unbounded, so the search stops at its budget.
    const SOURCE: &str = r#"{"name":"source","places":[{"id":"p"}],
        "transitions":[{"id":"t"}],"arcs":[{"from":"t","to":"p"}]}"#;

    #[test]
    fn reachability_deadlock_has_a_replayable_witness() {
        let c = Core::new(CHAIN).unwrap();
        let r = parsed(&c.reachability_json("").unwrap());
        assert_eq!(r["states"], 4);
        assert_eq!(r["edges"], 3);
        assert_eq!(r["complete"], true);
        assert_eq!(r["bounded"], true);
        assert_eq!(r["deadlocks"], 1);
        assert_eq!(r["deadlock_free"], false);
        assert_eq!(r["live"], false);
        assert_eq!(r["quasi_live"], true);
        assert_eq!(r["first_deadlock"], json!({"a": 0, "b": 3}));
        let w = &r["witness"];
        assert_eq!(w["kind"], "deadlock");
        assert_eq!(w["trace"], json!(["t", "t", "t"]));
        assert_eq!(w["never_fires"], json!(["t"]));
        assert_eq!(replay(CHAIN, &r["from"], &w["trace"]), w["marking"]);
    }

    #[test]
    fn reachability_live_cycle_proves_everything() {
        let r = parsed(&Core::new(CYCLE).unwrap().reachability_json("").unwrap());
        assert_eq!(r["states"], 2);
        assert_eq!(r["deadlock_free"], true);
        assert_eq!(r["live"], true);
        assert_eq!(r["quasi_live"], true);
        assert_eq!(r["witness"], Value::Null);
        assert_eq!(r["not_live"], json!([]));
        assert!(r.get("truncated").is_none() && r.get("first_deadlock").is_none());
    }

    #[test]
    fn reachability_livelock_is_l1_not_l4() {
        let r = parsed(&Core::new(LIVELOCK).unwrap().reachability_json("").unwrap());
        assert_eq!(r["complete"], true);
        assert_eq!(r["deadlock_free"], true);
        assert_eq!(r["quasi_live"], true);
        assert_eq!(r["live"], false);
        let w = &r["witness"];
        assert_eq!(w["kind"], "livelock");
        assert_eq!(w["region_size"], 2);
        assert_eq!(w["never_fires"], json!(["t", "u", "x"]));
        assert_eq!(r["not_live"], json!(["t", "u", "x"]));
        assert_eq!(replay(LIVELOCK, &r["from"], &w["trace"]), w["marking"]);
    }

    #[test]
    fn reachability_truncates_and_claims_nothing_true() {
        let r = parsed(
            &Core::new(SOURCE)
                .unwrap()
                .reachability_json(r#"{"maxStates":50}"#)
                .unwrap(),
        );
        assert_eq!(r["complete"], false);
        assert!(r["bounded"].as_str().unwrap().starts_with("unknown"));
        assert!(r["truncated"].is_string());
        assert_eq!(r["bound"]["max_states"], 50);
        for k in ["deadlock_free", "live", "quasi_live"] {
            assert_ne!(r[k], true, "{k} must not be true on a truncated search");
        }
    }

    #[test]
    fn reachability_from_the_current_marking_and_bad_options() {
        let mut c = Core::new(CHAIN).unwrap();
        c.fire_json("t").unwrap();
        let initial = parsed(&c.reachability_json("").unwrap());
        assert_eq!(initial["states"], 4);
        let current = parsed(&c.reachability_json(r#"{"from":"current"}"#).unwrap());
        assert_eq!(current["states"], 3);
        assert_eq!(current["from"], json!({"a": 2, "b": 1}));
        // The search does not move the marking.
        assert_eq!(parsed(&c.marking_json()), json!({"a": 2, "b": 1}));
        for bad in [
            r#"{"maxStates":0}"#,
            r#"{"maxStates":20001}"#,
            r#"{"maxTokens":0}"#,
            r#"{"from":"later"}"#,
            r#"{"max_states":10}"#,
        ] {
            assert!(c.reachability_json(bad).is_err(), "{bad}");
        }
    }
}
