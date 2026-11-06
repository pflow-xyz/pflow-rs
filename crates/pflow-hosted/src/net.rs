//! Reading the `net` argument the single-net tools share: the editor's
//! JSON-LD object, read the way pflow.xyz/mcp reads it (`netBytes`, go-pflow's
//! `parser.FromJSON`, `petri.ExpandColors`, `startMarking`, `numArg`).
//!
//! Two things the hosted server does implicitly have to be done here
//! explicitly:
//!
//!   - **Go's JSON round trip.** The server decodes the arguments into
//!     `map[string]any` (every number a `float64`) and re-marshals them before
//!     any engine sees them, so `1.0` arrives as `1`. [`go_value`] applies the
//!     same normalisation, and every tool reads the normalised document.
//!   - **Go's lenient reader.** `parser.FromJSON` never refuses an object: a
//!     field of the wrong type is skipped, not an error. [`petri_net`] reads
//!     the same way, where `pflow_core::from_json`'s typed serde reader would
//!     refuse (a numeric label, a string `x`).
//!
//! The engines (`pflow-reachability`, `pflow-verify`) run on a
//! `pflow_metamodel::Model`; [`model_of`] builds one from the color-expanded
//! net with go-pflow's firing rules: unfolded names keep the full token URL
//! (`p.https://pflow.xyz/tokens/red`), color copies no arc touches stay, an
//! output-side `inhibitTransition` is a read arc and every weight and marking
//! is truncated to an integer as Go's `int(...)` does. This is deliberately not
//! `pflow_parser::model_from_json`, which shortens color names and drops
//! untouched copies for the editor's other consumers.
//!
//! Known differences from the hosted tools, all outside a well-formed net:
//!
//!   - a negative arc weight: go-pflow fires the transition (moving tokens the
//!     other way); the metamodel firing rule refuses it, so the transition is
//!     never enabled here. `validate` reports the arc as an error either way;
//!   - the hosted transport cuts an answer over 60 KiB (`mcpResultLimit`);
//!     these functions return it whole and leave any limit to the caller.

use std::collections::BTreeMap;

use pflow_core::{ColorMap, PetriNet};
use pflow_metamodel::{Arc as MmArc, ArcType, Model, Place as MmPlace, Transition as MmTransition};
use serde_json::{Map, Number, Value};

/// The tools' state budget ceiling (`maxMCPStates`).
pub(crate) const MAX_STATES: f64 = 20000.0;

/// The `net` argument, after Go's JSON round trip (`netBytes`). Refused, as
/// hosted, when it is not an object or not in the editor's shape
/// ([`net_shape`]).
pub(crate) fn net_arg(args: &Value) -> Result<Map<String, Value>, String> {
    match args.get("net") {
        Some(Value::Object(m)) => {
            net_shape(m)?;
            match go_value(&Value::Object(m.clone())) {
                Value::Object(m) => Ok(m),
                _ => unreachable!("go_value keeps an object an object"),
            }
        }
        _ => Err("net: a JSON object (the editor's format) is required".to_string()),
    }
}

/// pflow-xyz `netShape` (pflow-xyz 2e57057): places and transitions, when
/// present, are objects keyed by id, and arcs a list of objects with a string
/// `source` and `target`. A part that is absent is left to the tool (a
/// composition has none). Messages verbatim, with the hint for the flat net
/// `pflow_bundle_flatten` returns.
pub(crate) fn net_shape(doc: &Map<String, Value>) -> Result<(), String> {
    let arcs = doc.get("arcs").and_then(Value::as_array);
    let flat = ["places", "transitions"]
        .iter()
        .any(|k| matches!(doc.get(*k), Some(Value::Array(_))))
        || arcs.is_some_and(|a| {
            a.iter().any(|x| {
                x.as_object().is_some_and(|m| {
                    m.get("from").is_some_and(|v| !v.is_null())
                        || m.get("to").is_some_and(|v| !v.is_null())
                })
            })
        });
    let hint = if flat {
        " (this looks like the flat net pflow_bundle_flatten returns, which the net tools cannot read: fire it with pflow_bundle_run)"
    } else {
        ""
    };
    for (k, id) in [("places", "place"), ("transitions", "transition")] {
        match doc.get(k) {
            None | Some(Value::Null) | Some(Value::Object(_)) => {}
            Some(_) => return Err(format!("net.{k}: an object keyed by {id} id{hint}")),
        }
    }
    match doc.get("arcs") {
        None | Some(Value::Null) => {}
        Some(Value::Array(arcs)) => {
            for (i, a) in arcs.iter().enumerate() {
                let Value::Object(m) = a else {
                    return Err(format!(
                        "net.arcs[{i}]: an object {{source, target, weight}}"
                    ));
                };
                if !matches!(m.get("source"), Some(Value::String(_))) {
                    return Err(format!(
                        "net.arcs[{i}]: source, the id of a place or transition{hint}"
                    ));
                }
                if !matches!(m.get("target"), Some(Value::String(_))) {
                    return Err(format!(
                        "net.arcs[{i}]: target, the id of a place or transition{hint}"
                    ));
                }
            }
        }
        Some(_) => return Err("net.arcs: a list of {source, target, weight}".into()),
    }
    Ok(())
}

/// Refuses an arc whose weight (the sum of its per-colour weights) is not a
/// whole number, naming it. go-pflow truncates it with `int(...)`, which turned
/// a half-token arc into no arc at all without a word; `load_net` refuses it
/// too, so one net is read one way everywhere. Used by the tools that run the
/// firing rule (`invariants`, `incidence`, `verify`; `check` only counts) and by `load_net`.
pub(crate) fn whole_weights(doc: &Map<String, Value>) -> Result<(), String> {
    let Some(Value::Array(arcs)) = doc.get("arcs") else {
        return Ok(());
    };
    for (i, a) in arcs.iter().enumerate() {
        let Some(m) = a.as_object() else { continue };
        let sum = match m.get("weight") {
            None | Some(Value::Null) => 1.0,
            w => float_slice(w).iter().sum(),
        };
        if sum.fract() != 0.0 {
            let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("?");
            return Err(format!(
                "net.arcs[{i}] {:?} -> {:?}: weight {} is not a whole number (a token count)",
                s("source"),
                s("target"),
                go_g(sum)
            ));
        }
    }
    Ok(())
}

/// `v` as Go's `json.Unmarshal` into `any` followed by `json.Marshal` leaves
/// it: a number with an integral value is written without a fraction.
pub(crate) fn go_value(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            Value::Object(m.iter().map(|(k, v)| (k.clone(), go_value(v))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(go_value).collect()),
        Value::Number(n) => Value::Number(go_number(n)),
        other => other.clone(),
    }
}

/// A `float64` as Go's `json.Marshal` writes it (`1`, not `1.0`).
pub(crate) fn go_f64(f: f64) -> Value {
    match Number::from_f64(f) {
        Some(n) => Value::Number(go_number(&n)),
        None => Value::Null,
    }
}

fn go_number(n: &Number) -> Number {
    if n.is_i64() || n.is_u64() {
        return n.clone();
    }
    let f = n.as_f64().unwrap_or(0.0);
    // Go prints an integral float64 below 1e21 without a fraction or exponent.
    if f.fract() == 0.0 && f.abs() < 9.2e18 {
        Number::from(f as i64)
    } else {
        n.clone()
    }
}

/// A number argument, `def` when absent: Go's `numArg`, error text included.
pub(crate) fn num_arg(args: &Value, key: &str, def: f64, lo: f64, hi: f64) -> Result<f64, String> {
    let Some(v) = args.get(key) else {
        return Ok(def);
    };
    match v.as_f64() {
        Some(f) if !f.is_nan() && f >= lo && f <= hi => Ok(f),
        _ => Err(format!("{key}: a number in [{}, {}]", go_g(lo), go_g(hi))),
    }
}

/// Go's `%g` for the bounds the tools use (integers and plain decimals).
fn go_g(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e21 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

/// go-petri's `asFloat64`, the vector reader's: a number, or a string that
/// parses as one.
fn vec_component(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

/// go-petri's `toFloatSlice` over a decoded JSON value.
fn float_slice(v: Option<&Value>) -> Vec<f64> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|x| match vec_component(x) {
                Some(f) if f.is_finite() => f,
                _ => 0.0,
            })
            .collect(),
        Some(x @ (Value::Number(_) | Value::String(_))) => match vec_component(x) {
            Some(f) if f.is_finite() => vec![f],
            _ => Vec::new(),
        },
        Some(_) => Vec::new(),
    }
}

/// The parser's `asFloat64` for coordinates: a number only.
fn coord(m: &Map<String, Value>, key: &str) -> f64 {
    m.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn label(m: &Map<String, Value>) -> Option<String> {
    m.get("label").and_then(Value::as_str).map(String::from)
}

/// go-pflow's `parser.FromJSON` over the (already object) document.
pub(crate) fn petri_net(doc: &Map<String, Value>) -> PetriNet {
    let mut net = PetriNet::new();
    if let Some(Value::Array(tokens)) = doc.get("token") {
        net.token = tokens
            .iter()
            .filter_map(|t| t.as_str().map(String::from))
            .collect();
    }
    if let Some(Value::Object(places)) = doc.get("places") {
        for (id, pd) in places {
            let empty = Map::new();
            let p = pd.as_object().unwrap_or(&empty);
            net.add_place(
                id.clone(),
                float_slice(p.get("initial")),
                float_slice(p.get("capacity")),
                coord(p, "x"),
                coord(p, "y"),
                label(p),
            );
        }
    }
    if let Some(Value::Object(transitions)) = doc.get("transitions") {
        for (id, td) in transitions {
            let empty = Map::new();
            let t = td.as_object().unwrap_or(&empty);
            let role = t.get("role").and_then(Value::as_str).unwrap_or("default");
            net.add_transition(
                id.clone(),
                role.to_string(),
                coord(t, "x"),
                coord(t, "y"),
                label(t),
            );
        }
    }
    if let Some(Value::Array(arcs)) = doc.get("arcs") {
        for a in arcs.iter().filter_map(Value::as_object) {
            let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let weight = match a.get("weight") {
                None | Some(Value::Null) => vec![1.0],
                w => float_slice(w),
            };
            let inhibit = a
                .get("inhibitTransition")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            net.add_arc(s("source"), s("target"), weight, inhibit);
        }
    }
    net
}

/// The net's ids in Go's `sort.Strings` order (byte order, as `String`'s `Ord`).
pub(crate) fn sorted<'a, I: IntoIterator<Item = &'a String>>(ids: I) -> Vec<String> {
    let mut v: Vec<String> = ids.into_iter().cloned().collect();
    v.sort();
    v
}

/// The color-expanded net (`petri.ExpandColors`): the net itself when it has
/// one color.
pub(crate) fn expand(net: &PetriNet) -> (PetriNet, Option<ColorMap>) {
    net.expand_colors()
}

/// `int(p.GetTokenCount())`.
pub(crate) fn tokens(p: &pflow_core::Place) -> i64 {
    p.token_count() as i64
}

/// `startMarking`: the initial marking in the analyzers' (unfolded) names.
pub(crate) fn start_marking(expanded: &PetriNet) -> BTreeMap<String, i64> {
    expanded
        .places
        .iter()
        .map(|(id, p)| (id.clone(), tokens(p)))
        .collect()
}

/// The engines' view of an expanded net, under go-pflow's firing rules (see
/// the module doc). An arc whose truncated weight is zero moves and gates
/// nothing in Go and is left out (the metamodel reads weight 0 as 1), as is
/// an arc that does not join a place and a transition.
pub(crate) fn model_of(expanded: &PetriNet) -> Model {
    let mut model = Model::default();
    for id in sorted(expanded.places.keys()) {
        let p = &expanded.places[&id];
        let capacity: i64 = p.capacity.iter().map(|&c| c as i64).sum();
        model.places.push(MmPlace {
            id,
            initial: tokens(p),
            capacity,
            ..Default::default()
        });
    }
    for id in sorted(expanded.transitions.keys()) {
        model.transitions.push(MmTransition {
            id,
            ..Default::default()
        });
    }
    for a in &expanded.arcs {
        let weight = a.weight_sum() as i64;
        if weight == 0 {
            continue;
        }
        let source_is_place = expanded.places.contains_key(&a.source);
        // go-pflow's firing rule reads only place -> transition and
        // transition -> place arcs; a dangling one gates and moves nothing.
        let joins = if source_is_place {
            expanded.transitions.contains_key(&a.target)
        } else {
            expanded.transitions.contains_key(&a.source) && expanded.places.contains_key(&a.target)
        };
        if !joins {
            continue;
        }
        let (from, to, typ) = match (a.inhibit_transition, source_is_place) {
            (false, _) => (a.source.clone(), a.target.clone(), ArcType::Normal),
            (true, true) => (a.source.clone(), a.target.clone(), ArcType::Inhibitor),
            (true, false) => (a.target.clone(), a.source.clone(), ArcType::Read),
        };
        model.arcs.push(MmArc {
            from,
            to,
            weight,
            typ,
            ..Default::default()
        });
    }
    model
}

/// The net's color count as `colorCount` reports it: the declared token
/// list's length, or 1.
pub(crate) fn color_count(doc: &Map<String, Value>) -> usize {
    match doc.get("token") {
        Some(Value::Array(t)) if !t.is_empty() => t.len(),
        _ => 1,
    }
}

/// A JSON object of integers, keys sorted as Go's `json.Marshal` writes a map.
pub(crate) fn int_map<'a, I: IntoIterator<Item = (&'a String, &'a i64)>>(m: I) -> Value {
    Value::Object(
        m.into_iter()
            .map(|(k, v)| (k.clone(), Value::from(*v)))
            .collect(),
    )
}
