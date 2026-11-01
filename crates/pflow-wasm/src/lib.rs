//! pflow-rs for the browser, packaged on npm as `@pflow-xyz/pflow-wasm`.
//!
//! This crate adds no engine behaviour, only the JavaScript boundary: firing
//! is `pflow_metamodel`'s rule, the ODE is `pflow_solver::solve` with Tsit5,
//! the stochastic run is `pflow_solver::ssa::simulate` (the portable,
//! byte-exact Gillespie SSA).
//!
//! The logic lives in [`Core`], plain Rust over JSON text returning
//! `Result<String, String>`, so it is tested natively (`cargo test -p
//! pflow-wasm`); the `wasm_bindgen` methods on [`Net`] only convert values.
//! `crates/pflow-wasm/npm/test.mjs` runs the same golden fixtures through the
//! built package.

use std::collections::{BTreeMap, HashMap, HashSet};

use pflow_core::PetriNet;
use pflow_metamodel::{ArcType, Marking, Model};
use pflow_solver::ssa::{
    simulate as simulate_ssa, ArcKind, SsaArc, SsaModel, SsaOptions, SsaPlace, SsaTransition,
};
use pflow_solver::{methods, solve, stochastic, ContextualArcs, Options, Problem};
use serde::Deserialize;
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

/// Reads a model: Shape A (the pflow.xyz editor document) or Shape B
/// (`pflow_metamodel::Model`), and refuses what these engines would otherwise
/// get silently wrong.
fn parse_model(text: &str) -> Result<Model, String> {
    let bytes = text.as_bytes();
    let model = if pflow_parser::is_pflow_json(bytes) {
        pflow_parser::model_from_json(bytes)
            .map_err(|e| format!("Shape A parse error: {e}"))?
            .0
    } else {
        serde_json::from_str::<Model>(text)
            .map_err(|e| format!("not a recognized model (Shape A or Shape B JSON): {e}"))?
    };
    check_model(&model)?;
    Ok(model)
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
    rates: HashMap<String, f64>,
    contextual: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct StochasticOptions {
    horizon: Option<f64>,
    samples: Option<usize>,
    realizations: Option<usize>,
    seed: Option<u64>,
    rates: HashMap<String, f64>,
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
}

impl Core {
    pub fn new(model_json: &str) -> Result<Core, String> {
        let model = parse_model(model_json)?;
        let marking = model.initial_marking();
        Ok(Core { model, marking })
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

    /// The model's own rates with `overrides` on top. A rate for a transition
    /// the net does not have is an error, not something to ignore.
    fn rates(&self, overrides: &HashMap<String, f64>) -> Result<HashMap<String, f64>, String> {
        let mut rates = stochastic::rates(&self.model);
        for (id, r) in overrides {
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
}
