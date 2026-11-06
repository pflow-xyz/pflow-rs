//! `net_to_jsonld`: any net `pflow-mcp`'s `load_net` accepts, as the pflow.xyz
//! JSON-LD document every hosted-name tool reads (bug B3 of the 0.4.0 test
//! report: a net loaded in the array shape could be simulated but not
//! checked, proved, exported or sealed).
//!
//! Not one of pflow.xyz's tools, so it is not in [`crate::TOOLS`]: the server
//! calls it from `load_net` through `hosted("net_to_jsonld", …)`.
//!
//! Two shapes come in, the two `pflow_wasm::Net::new` reads:
//!
//!   - **the editor's JSON-LD** (`places` an object keyed by id): kept as
//!     written. Missing `@context`, `@type`, `@version` and `token` are added
//!     (the hosted tools refuse a document without them); nothing else changes,
//!     so its CID is the one pflow.xyz gives it (`layout` says `given`, or
//!     `none` when no node has a coordinate).
//!   - **the array shape** (`pflow_metamodel::Model`: `places` a list of
//!     `{id, initial, capacity}`, `arcs` a list of `{from, to, weight, type}`,
//!     the shape of every sim.pflow.xyz model): written as a Shape A document.
//!
//! # Arcs
//!
//! go-pflow and `pflow_parser::model_from_json` read an arc's
//! `inhibitTransition` flag by direction (`ModelFromJSON`, "An input-side
//! inhibitTransition (place -> transition) is an inhibitor arc. An output-side
//! one (transition -> place) is the editor's read arc"), so the array shape's
//! arcs are written back the same way:
//!
//! | array shape                         | JSON-LD arc                                   |
//! |-------------------------------------|-----------------------------------------------|
//! | normal `place -> transition`, `transition -> place` | as is, `inhibitTransition: false` |
//! | `inhibitor`, `from` place, `to` transition | `source` place, `target` transition, `inhibitTransition: true` |
//! | `read`, `from` place, `to` transition      | `source` the **transition**, `target` the **place**, `inhibitTransition: true` |
//!
//! An inhibitor or read arc that does not run place to transition cannot be
//! written either way, and is refused with the arc named. (The JS helper
//! `toJsonLd` of the test corpus swaps the ends of a read arc and writes it as
//! an inhibitor: not this.)
//!
//! An array-shape arc's weight 0 means 1 (`Arc::effective_weight`), as the
//! firing rule reads it.
//!
//! # What is not written, and what is refused
//!
//! A net the JSON-LD document cannot say faithfully is **not converted**: the
//! answer carries `unsupported` (the reason) and no `net`, so a hosted-name
//! tool on it says why instead of judging a different net. That is a model
//! with data places, constraints, parameters or a solver rate override, a
//! transition with a timed delay, or an arc that is not kinetic or carries
//! keys. Display-only fields (views, presentation, events, tags, units, place
//! types) are dropped. A fractional arc weight, in either shape, is a refusal
//! (`Err`) naming the arc: hosted `check` and `invariants` would otherwise
//! truncate it to 0 and `load_net` refuse it, for the same net.
//!
//! # Layout, and what the CID names
//!
//! An array-shape net has no coordinates (all `0`, or absent). When **no**
//! node has one, a layout is synthesized, deterministically: nodes are
//! layered by breadth-first distance from the sources (a place without an
//! input arc, then a transition without one, in declaration order; a cycle
//! with no source starts from its first node), column `x = 100 + 140 *
//! layer`, row `y = 80 + 100 * (position in the layer)`.
//!
//! The CID is of **the document**, as pflow.xyz's is (bug B6): it changes with
//! the layout, the arc order and the net's name, so the same array-shape net
//! written with a different layout has a different CID. The answer says so in
//! `cid_note` and says `layout: "synthesized"` when the layout is ours.
//!
//! # Cost
//!
//! The CID is the single-threaded URDNA2015 canonicalisation, about cubic in
//! the net's size (53 s at 200 places, bug B5). So it is computed here only
//! for a net of at most [`EAGER_CID_MAX_PLACES`] places and [`EAGER_CID_MAX_NODES`]
//! nodes-plus-arcs (or when `cid: true`); the cost grows with every node and arc,
//! not with places alone (10 places and 200 transitions took 56 s);
//! above that `cid` is `null` with `cid_skipped` saying why, and the `cid`
//! tool computes it on demand.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use pflow_metamodel::{ArcType, Model, StateKind};
use serde_json::{json, Map, Value};

use crate::cid::seal;
use crate::net::net_shape;

/// A net with more places than this has no CID computed unless asked.
pub const EAGER_CID_MAX_PLACES: usize = 50;

/// A net whose places + transitions + arcs exceed this has no CID computed
/// unless asked: canonicalisation cost follows the whole graph, not the place
/// count alone.
pub const EAGER_CID_MAX_NODES: usize = 250;

const CONTEXT: &str = "https://pflow.xyz/schema";
const BLACK: &str = "https://pflow.xyz/tokens/black";

/// `args` is `{model, name?, cid?}`: `model` an object or JSON text, `name`
/// the name the net is kept under (used as the document's `name` when the
/// model has none), `cid` `true` to compute the CID whatever the size,
/// `false` never, absent for up to [`EAGER_CID_MAX_PLACES`] places.
pub(crate) fn run(args: &Value) -> Result<Value, String> {
    let model = match args.get("model") {
        Some(Value::String(s)) => {
            serde_json::from_str::<Value>(s).map_err(|e| format!("model: not JSON: {e}"))?
        }
        Some(v @ Value::Object(_)) => v.clone(),
        _ => return Err("model: a net as a JSON object or JSON text".into()),
    };
    let name = args.get("name").and_then(Value::as_str);
    let want_cid = match args.get("cid") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err("cid: true or false".into()),
    };
    let (mut doc, layout, shape) = match model.get("places") {
        Some(Value::Object(_)) => {
            let (d, l) = from_editor(&model)?;
            (d, l, "jsonld")
        }
        Some(Value::Array(_)) => match from_array(&model, name)? {
            Converted::Doc(d, l) => (d, l, "array"),
            Converted::Unsupported(why) => {
                return Ok(json!({"shape": "array", "net": null, "unsupported": why,
                    "cid": null, "cid_skipped": "the net is not expressible as a pflow.xyz document"}))
            }
        },
        _ => {
            return Err("model: places must be an object keyed by id (pflow.xyz JSON-LD) or a list of {id, initial} (array shape)".into())
        }
    };

    let places = doc["places"].as_object().map_or(0, Map::len);
    let size = places
        + doc.get("transitions").and_then(Value::as_object).map_or(0, Map::len)
        + doc.get("arcs").and_then(Value::as_array).map_or(0, Vec::len);
    let compute = want_cid.unwrap_or(places <= EAGER_CID_MAX_PLACES && size <= EAGER_CID_MAX_NODES);
    let mut out = Map::new();
    out.insert("shape".into(), json!(shape));
    out.insert("layout".into(), json!(layout));
    out.insert("places".into(), json!(places));
    if compute {
        match seal(&doc) {
            Ok(cid) => {
                doc.insert("@id".into(), json!(cid));
                out.insert("cid".into(), json!(cid));
                out.insert(
                    "cid_note".into(),
                    json!(cid_note(layout)),
                );
            }
            Err(e) => {
                out.insert("cid".into(), Value::Null);
                out.insert("cid_skipped".into(), json!(e));
            }
        }
    } else {
        out.insert("cid".into(), Value::Null);
        let why = if want_cid == Some(false) {
            "cid: false".to_string()
        } else {
            format!(
                "{places} places and {size} places+transitions+arcs is over the limit ({EAGER_CID_MAX_PLACES} places, {EAGER_CID_MAX_NODES} in all): sealing is about cubic in net size (53 s at 200 places), so it is left for the cid tool"
            )
        };
        out.insert("cid_skipped".into(), json!(why));
    }
    out.insert("net".into(), Value::Object(doc));
    Ok(Value::Object(out))
}

fn cid_note(layout: &str) -> String {
    let own = if layout == "synthesized" {
        " The layout (x, y) of this net was synthesized by pflow-mcp, deterministically, so another tool writing the same net gets another CID."
    } else {
        ""
    };
    format!("The CID names this document, as pflow.xyz's does: it includes the layout (x, y), the order of the arcs and the net's name, so two copies of one net that differ in those have different CIDs.{own}")
}

enum Converted {
    Doc(Map<String, Value>, &'static str),
    Unsupported(String),
}

// ---- the editor's own shape ------------------------------------------------

fn from_editor(model: &Value) -> Result<(Map<String, Value>, &'static str), String> {
    let Value::Object(mut doc) = model.clone() else {
        unreachable!("the caller matched an object");
    };
    net_shape(&doc)?;
    whole_weights_of_editor_doc(&doc)?;
    for (k, def) in [
        ("@context", json!(CONTEXT)),
        ("@type", json!("PetriNet")),
        ("@version", json!("1.1")),
        ("token", json!([BLACK])),
    ] {
        if doc.get(k).is_none_or(Value::is_null) {
            doc.insert(k.into(), def);
        }
    }
    for (k, def) in [("transitions", json!({})), ("arcs", json!([]))] {
        if doc.get(k).is_none_or(Value::is_null) {
            doc.insert(k.into(), def);
        }
    }
    let has_layout = ["places", "transitions"].iter().any(|k| {
        doc[*k].as_object().is_some_and(|m| {
            m.values().any(|n| {
                ["x", "y"]
                    .iter()
                    .any(|c| n.get(*c).and_then(Value::as_f64).is_some_and(|v| v != 0.0))
            })
        })
    });
    // An editor document is the author's: its layout (or lack of one) is left
    // alone, so its CID stays the one pflow.xyz gives it.
    Ok((doc, if has_layout { "given" } else { "none" }))
}

/// Refuses an arc whose weight is not a whole number, naming it. The weight
/// of an editor arc is a list of per-colour weights; the net reads their sum.
fn whole_weights_of_editor_doc(doc: &Map<String, Value>) -> Result<(), String> {
    crate::net::whole_weights(doc)
}

// ---- the array shape -------------------------------------------------------

fn from_array(model: &Value, name: Option<&str>) -> Result<Converted, String> {
    let Value::Object(raw) = model else {
        unreachable!("the caller matched an object");
    };
    // Raw checks first, so the refusal names the arc and says what is wrong,
    // where serde would say "invalid type: floating point".
    let mut raw = raw.clone();
    if let Some(Value::Array(arcs)) = raw.get_mut("arcs") {
        for (i, a) in arcs.iter_mut().enumerate() {
            let Some(a) = a.as_object_mut() else { continue };
            let label = format!(
                "arcs[{i}] {:?} -> {:?}",
                a.get("from").and_then(Value::as_str).unwrap_or("?"),
                a.get("to").and_then(Value::as_str).unwrap_or("?")
            );
            if let Some(w) = a.get("weight").and_then(Value::as_f64) {
                if w.fract() != 0.0 || !w.is_finite() {
                    return Err(format!("{label}: weight {w} is not a whole number"));
                }
                a.insert("weight".into(), json!(w as i64));
            }
        }
    }
    if raw.get("name").is_none_or(Value::is_null) {
        raw.insert("name".into(), json!(""));
    }
    // A rate present in the text as a number, 0 included, is the net's: `0`
    // means off, absent or null means unset (the typed model cannot tell).
    let mut rates: HashMap<String, f64> = HashMap::new();
    if let Some(Value::Array(ts)) = raw.get_mut("transitions") {
        for t in ts.iter_mut().filter_map(Value::as_object_mut) {
            if t.get("rate").is_some_and(Value::is_null) {
                t.remove("rate");
            }
            if let (Some(id), Some(r)) = (
                t.get("id").and_then(Value::as_str),
                t.get("rate").and_then(Value::as_f64),
            ) {
                rates.insert(id.to_string(), r);
            }
        }
    }
    let m: Model = serde_json::from_value(Value::Object(raw))
        .map_err(|e| format!("not a recognized model (array shape): {e}"))?;

    if let Some(why) = unsupported(&m) {
        return Ok(Converted::Unsupported(why));
    }
    check_array(&m)?;

    let place_ids: Vec<String> = m.places.iter().map(|p| p.id.clone()).collect();
    let transition_ids: Vec<String> = m.transitions.iter().map(|t| t.id.clone()).collect();
    let placed = m.places.iter().any(|p| p.x != 0 || p.y != 0)
        || m.transitions.iter().any(|t| t.x != 0 || t.y != 0);
    let (layout, pos) = if placed {
        ("given", HashMap::new())
    } else {
        let places: HashSet<&str> = place_ids.iter().map(String::as_str).collect();
        let edges: Vec<(String, String)> = m
            .arcs
            .iter()
            .map(|a| {
                // layout direction: flow arcs as they run, tests place -> transition
                if a.typ != ArcType::Normal && !places.contains(a.from.as_str()) {
                    (a.to.clone(), a.from.clone())
                } else {
                    (a.from.clone(), a.to.clone())
                }
            })
            .collect();
        (
            "synthesized",
            synthesize_layout(&place_ids, &transition_ids, &edges),
        )
    };
    let at = |id: &str, x: i64, y: i64| -> (Value, Value) {
        match pos.get(id) {
            Some((px, py)) => (json!(px), json!(py)),
            None => (json!(x), json!(y)),
        }
    };

    let mut place_nodes = Map::new();
    for p in &m.places {
        let (x, y) = at(&p.id, p.x, p.y);
        let mut n = Map::new();
        n.insert("@type".into(), json!("Place"));
        n.insert("capacity".into(), json!([p.capacity]));
        n.insert("initial".into(), json!([p.initial]));
        n.insert("x".into(), x);
        n.insert("y".into(), y);
        if !p.description.is_empty() {
            n.insert("label".into(), json!(p.description));
        }
        place_nodes.insert(p.id.clone(), Value::Object(n));
    }
    let mut transition_nodes = Map::new();
    for t in &m.transitions {
        let (x, y) = at(&t.id, t.x, t.y);
        let mut n = Map::new();
        n.insert("@type".into(), json!("Transition"));
        n.insert("role".into(), json!("default"));
        n.insert("x".into(), x);
        n.insert("y".into(), y);
        if !t.description.is_empty() {
            n.insert("label".into(), json!(t.description));
        }
        if let Some(r) = rates.get(&t.id) {
            n.insert("rate".into(), crate::net::go_f64(*r));
        }
        transition_nodes.insert(t.id.clone(), Value::Object(n));
    }
    let arcs: Vec<Value> = m
        .arcs
        .iter()
        .map(|a| {
            let (source, target, inhibit) = match a.typ {
                ArcType::Normal => (&a.from, &a.to, false),
                ArcType::Inhibitor => (&a.from, &a.to, true),
                // The editor's read arc: transition -> place with the flag.
                ArcType::Read => (&a.to, &a.from, true),
            };
            json!({
                "@type": "Arrow",
                "inhibitTransition": inhibit,
                "source": source,
                "target": target,
                "weight": [a.effective_weight()],
            })
        })
        .collect();

    let mut doc = Map::new();
    doc.insert("@context".into(), json!(CONTEXT));
    doc.insert("@type".into(), json!("PetriNet"));
    doc.insert("@version".into(), json!("1.1"));
    doc.insert("arcs".into(), Value::Array(arcs));
    let doc_name = if m.name.is_empty() { name.unwrap_or("") } else { &m.name };
    if !doc_name.is_empty() {
        doc.insert("name".into(), json!(doc_name));
    }
    if !m.description.is_empty() {
        doc.insert("description".into(), json!(m.description));
    }
    doc.insert("places".into(), Value::Object(place_nodes));
    doc.insert("token".into(), json!([BLACK]));
    doc.insert("transitions".into(), Value::Object(transition_nodes));
    Ok(Converted::Doc(doc, layout))
}

/// What of the array shape a JSON-LD document cannot say, or `None`.
fn unsupported(m: &Model) -> Option<String> {
    if !m.constraints.is_empty() {
        return Some("the model has constraints, which a pflow.xyz net cannot carry".into());
    }
    if !m.parameters.is_empty() {
        return Some("the model has parameters, which a pflow.xyz net cannot carry".into());
    }
    if m.simulation
        .as_ref()
        .and_then(|s| s.solver.as_ref())
        .is_some_and(|s| s.rates.is_some())
    {
        return Some("the model overrides rates in simulation.solver.rates, which a pflow.xyz net cannot carry".into());
    }
    for p in &m.places {
        if matches!(p.kind, Some(StateKind::Data)) {
            return Some(format!("place {:?} holds data, not tokens, which a pflow.xyz net cannot carry", p.id));
        }
    }
    for t in &m.transitions {
        if t.delay > 0.0 {
            return Some(format!("transition {:?} is timed (delay), which a pflow.xyz net cannot carry", t.id));
        }
    }
    for a in &m.arcs {
        if a.kinetic == Some(false) {
            return Some(format!("arc {:?} -> {:?} is not kinetic, which a pflow.xyz net cannot carry", a.from, a.to));
        }
        if !a.keys.is_empty() || !a.value.is_empty() {
            return Some(format!("arc {:?} -> {:?} reads data, which a pflow.xyz net cannot carry", a.from, a.to));
        }
    }
    None
}

/// The refusals `pflow_wasm::Net::new` makes, so a net that does not load
/// does not convert either, each naming what is wrong.
fn check_array(m: &Model) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut places = HashSet::new();
    let mut transitions = HashSet::new();
    for p in &m.places {
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
    for t in &m.transitions {
        if !ids.insert(t.id.as_str()) {
            return Err(format!("duplicate id {:?}", t.id));
        }
        if !t.guard.is_empty() {
            return Err(format!("transition {:?}: guards are not supported", t.id));
        }
        if t.stages > 1 {
            return Err(format!("transition {:?}: stages are not supported", t.id));
        }
        if !t.schedule.is_empty() {
            return Err(format!("transition {:?}: rate schedules are not supported", t.id));
        }
        if !t.rate.is_finite() || t.rate < 0.0 {
            return Err(format!("transition {:?}: bad rate {}", t.id, t.rate));
        }
        transitions.insert(t.id.as_str());
    }
    for (i, a) in m.arcs.iter().enumerate() {
        let from_place = places.contains(a.from.as_str());
        let joined = (from_place && transitions.contains(a.to.as_str()))
            || (transitions.contains(a.from.as_str()) && places.contains(a.to.as_str()));
        if !joined {
            return Err(format!(
                "arcs[{i}] {:?} -> {:?} does not join a place and a transition",
                a.from, a.to
            ));
        }
        if a.weight < 0 {
            return Err(format!("arcs[{i}] {:?} -> {:?}: negative weight", a.from, a.to));
        }
        if a.typ != ArcType::Normal && !from_place {
            return Err(format!(
                "arcs[{i}] {:?} -> {:?}: a {} arc runs from a place to a transition",
                a.from, a.to, a.typ
            ));
        }
    }
    Ok(())
}

// ---- layout ------------------------------------------------------------------

/// Deterministic coordinates for every node (module docs, "Layout").
fn synthesize_layout(
    places: &[String],
    transitions: &[String],
    edges: &[(String, String)],
) -> HashMap<String, (i64, i64)> {
    let order: Vec<&String> = places.iter().chain(transitions.iter()).collect();
    let mut out_of: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut has_in: HashSet<&str> = HashSet::new();
    for (s, t) in edges {
        out_of.entry(s.as_str()).or_default().push(t.as_str());
        has_in.insert(t.as_str());
    }
    let mut layer: HashMap<&str, usize> = HashMap::new();
    let mut by_layer: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    let mut seeds: Vec<&str> = order
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !has_in.contains(s))
        .collect();
    // A cycle no source reaches: start from its first node, in order.
    let mut i = 0;
    loop {
        for s in std::mem::take(&mut seeds) {
            if layer.contains_key(s) {
                continue;
            }
            layer.insert(s, 0);
            by_layer.entry(0).or_default().push(s);
            queue.push_back(s);
            while let Some(u) = queue.pop_front() {
                let lu = layer[u];
                for &v in out_of.get(u).into_iter().flatten() {
                    if !layer.contains_key(v) {
                        layer.insert(v, lu + 1);
                        by_layer.entry(lu + 1).or_default().push(v);
                        queue.push_back(v);
                    }
                }
            }
        }
        while i < order.len() && layer.contains_key(order[i].as_str()) {
            i += 1;
        }
        if i == order.len() {
            break;
        }
        seeds.push(order[i].as_str());
    }
    let mut pos = HashMap::new();
    for (l, ids) in &by_layer {
        for (row, id) in ids.iter().enumerate() {
            pos.insert((*id).to_string(), (100 + 140 * *l as i64, 80 + 100 * row as i64));
        }
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::PetriNet;
    use pflow_tokenmodel::shape_a::to_shape_a;

    fn convert(model: Value) -> Value {
        run(&json!({"model": model, "cid": true})).unwrap()
    }

    /// The array shape, written with the layout given, is exactly the document
    /// pflow-tokenmodel's Shape A writer writes for the same net: same
    /// fields, same `@id`. (Name, description and rates are left out here:
    /// a `PetriNet` has no slot for them.)
    #[test]
    fn matches_the_shape_a_writer_byte_for_byte() {
        let out = convert(json!({
            "places": [
                {"id": "p", "initial": 2, "capacity": 5, "x": 10, "y": 20, "description": "P"},
                {"id": "q", "x": 30, "y": 20},
                {"id": "lock", "initial": 1, "x": 10, "y": 60}
            ],
            "transitions": [{"id": "t", "x": 20, "y": 20}, {"id": "u", "x": 40, "y": 20, "description": "U"}],
            "arcs": [
                {"from": "p", "to": "t", "weight": 2},
                {"from": "t", "to": "q"},
                {"from": "lock", "to": "t", "type": "inhibitor"},
                {"from": "q", "to": "u", "type": "read", "weight": 3}
            ]
        }));
        assert_eq!(out["layout"], "given");

        let mut net = PetriNet::new();
        net.token = vec![BLACK.to_string()];
        net.add_place("p", vec![2.0], vec![5.0], 10.0, 20.0, Some("P".into()));
        net.add_place("q", vec![0.0], vec![0.0], 30.0, 20.0, None);
        net.add_place("lock", vec![1.0], vec![0.0], 10.0, 60.0, None);
        net.add_transition("t", "default", 20.0, 20.0, None);
        net.add_transition("u", "default", 40.0, 20.0, Some("U".into()));
        net.add_arc("p", "t", vec![2.0], false);
        net.add_arc("t", "q", vec![1.0], false);
        net.add_arc("lock", "t", vec![1.0], true);
        // go-pflow's read arc: transition -> place with inhibitTransition
        net.add_arc("u", "q", vec![3.0], true);
        let want = to_shape_a(&net).unwrap();
        assert_eq!(out["net"], want);
        assert_eq!(out["cid"], want["@id"]);
    }

    /// The meaning of the two flagged arcs is go-pflow's: reading the document
    /// back through `model_from_json` gives the arcs that went in.
    #[test]
    fn inhibitor_and_read_arcs_round_trip_through_the_parser() {
        let arcs = json!([
            {"from": "p", "to": "t"},
            {"from": "t", "to": "q"},
            {"from": "lock", "to": "t", "type": "inhibitor", "weight": 2},
            {"from": "q", "to": "t", "type": "read", "weight": 3}
        ]);
        let out = convert(json!({
            "places": [{"id": "p", "initial": 1}, {"id": "q"}, {"id": "lock"}],
            "transitions": [{"id": "t"}],
            "arcs": arcs,
        }));
        let doc = out["net"].clone();
        // the read arc is stored transition -> place, flagged
        assert_eq!(doc["arcs"][3]["source"], "t");
        assert_eq!(doc["arcs"][3]["target"], "q");
        assert_eq!(doc["arcs"][3]["inhibitTransition"], true);
        assert_eq!(doc["arcs"][2]["source"], "lock");
        assert_eq!(doc["arcs"][2]["inhibitTransition"], true);
        let (m, _) = pflow_parser::model_from_json(&serde_json::to_vec(&doc).unwrap()).unwrap();
        let got: Vec<(String, String, i64, ArcType)> = m
            .arcs
            .iter()
            .map(|a| (a.from.clone(), a.to.clone(), a.effective_weight(), a.typ))
            .collect();
        assert_eq!(
            got,
            vec![
                ("p".into(), "t".into(), 1, ArcType::Normal),
                ("t".into(), "q".into(), 1, ArcType::Normal),
                ("lock".into(), "t".into(), 2, ArcType::Inhibitor),
                ("q".into(), "t".into(), 3, ArcType::Read),
            ]
        );
    }

    /// A converted net answers the hosted-name tools the way the same net
    /// written by hand does, including the arcs that gate.
    #[test]
    fn hosted_tools_read_a_converted_net() {
        let out = convert(json!({
            "name": "mutex",
            "places": [{"id": "idle", "initial": 2}, {"id": "crit"}, {"id": "lock", "initial": 1}],
            "transitions": [{"id": "enter"}, {"id": "leave"}],
            "arcs": [
                {"from": "idle", "to": "enter"}, {"from": "lock", "to": "enter"}, {"from": "enter", "to": "crit"},
                {"from": "crit", "to": "leave"}, {"from": "leave", "to": "idle"}, {"from": "leave", "to": "lock"}
            ]
        }));
        let net = out["net"].clone();
        for tool in ["check", "info", "incidence", "invariants", "validate", "verify", "export", "cid"] {
            let args = if tool == "verify" {
                json!({"net": net, "properties": [{"kind": "deadlock-free"}]})
            } else {
                json!({"net": net})
            };
            crate::call(tool, &args).unwrap_or_else(|e| panic!("{tool}: {e}"));
        }
        let inv = crate::call("invariants", &json!({"net": net})).unwrap();
        let eqs: Vec<&str> = inv["p_invariants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["equation"].as_str().unwrap())
            .collect();
        assert!(eqs.contains(&"crit + lock == 1"), "{eqs:?}");
        assert_eq!(crate::call("cid", &json!({"net": net})).unwrap()["cid"], out["cid"]);
    }

    #[test]
    fn layout_is_synthesized_deterministically_when_absent() {
        let model = json!({
            "places": [{"id": "a", "initial": 1}, {"id": "b"}, {"id": "c"}],
            "transitions": [{"id": "t"}, {"id": "u"}],
            "arcs": [{"from": "a", "to": "t"}, {"from": "t", "to": "b"}, {"from": "b", "to": "u"}, {"from": "u", "to": "c"}]
        });
        let one = convert(model.clone());
        let two = convert(model);
        assert_eq!(one, two);
        assert_eq!(one["layout"], "synthesized");
        assert!(one["cid_note"].as_str().unwrap().contains("synthesized"));
        let doc = &one["net"];
        // a -> t -> b -> u -> c: five layers, one column each
        let xs: Vec<i64> = ["a", "b", "c"]
            .iter()
            .map(|p| doc["places"][p]["x"].as_i64().unwrap())
            .collect();
        assert_eq!(xs, vec![100, 380, 660]);
        assert_eq!(doc["transitions"]["t"]["x"], 240);
        // every node has its own position
        let mut seen = HashSet::new();
        for k in ["places", "transitions"] {
            for n in doc[k].as_object().unwrap().values() {
                assert!(seen.insert((n["x"].as_i64().unwrap(), n["y"].as_i64().unwrap())));
            }
        }
        // a net with a layout keeps it
        let kept = convert(json!({"places": [{"id": "a", "x": 7, "y": 9}], "transitions": [{"id": "t"}], "arcs": [{"from": "a", "to": "t"}]}));
        assert_eq!(kept["layout"], "given");
        assert_eq!(kept["net"]["places"]["a"]["x"], 7);
    }

    #[test]
    fn cycles_with_no_source_still_get_a_layout() {
        let out = convert(json!({
            "places": [{"id": "a", "initial": 1}, {"id": "b"}],
            "transitions": [{"id": "t"}, {"id": "u"}],
            "arcs": [{"from": "a", "to": "t"}, {"from": "t", "to": "b"}, {"from": "b", "to": "u"}, {"from": "u", "to": "a"}]
        }));
        assert_eq!(out["layout"], "synthesized");
        assert_eq!(out["net"]["places"]["a"]["x"], 100);
    }

    #[test]
    fn rates_zero_is_off_and_absent_is_unset() {
        let out = convert(json!({
            "places": [{"id": "a", "initial": 1}],
            "transitions": [{"id": "off", "rate": 0}, {"id": "slow", "rate": 0.25}, {"id": "unset"}, {"id": "nul", "rate": null}],
            "arcs": [{"from": "a", "to": "off"}]
        }));
        let t = &out["net"]["transitions"];
        assert_eq!(t["off"]["rate"], 0);
        assert_eq!(t["slow"]["rate"], 0.25);
        assert!(t["unset"].get("rate").is_none());
        assert!(t["nul"].get("rate").is_none());
    }

    #[test]
    fn refusals_name_the_arc() {
        let e = run(&json!({"model": {
            "places": [{"id": "a"}], "transitions": [{"id": "t"}],
            "arcs": [{"from": "a", "to": "t"}, {"from": "a", "to": "t", "weight": 0.5}]
        }}))
        .unwrap_err();
        assert!(e.contains("arcs[1]") && e.contains("0.5"), "{e}");
        // the editor's own shape: a fractional weight is refused the same way
        let e = run(&json!({"model": {
            "places": {"a": {"initial": [1]}}, "transitions": {"t": {}},
            "arcs": [{"source": "a", "target": "t", "weight": [0.5]}]
        }}))
        .unwrap_err();
        assert!(e.contains("arcs[0]") && e.contains("0.5"), "{e}");
        let e = run(&json!({"model": {
            "places": [{"id": "a"}], "transitions": [{"id": "t"}],
            "arcs": [{"from": "t", "to": "a", "type": "read"}]
        }}))
        .unwrap_err();
        assert!(e.contains("arcs[0]") && e.contains("read"), "{e}");
        let e = run(&json!({"model": {"places": [{"id": "a"}], "transitions": [], "arcs": [{"from": "a", "to": "zz"}]}})).unwrap_err();
        assert!(e.contains("zz"), "{e}");
    }

    /// B7: the tools that run the firing rule refuse a fractional weight, naming
    /// the arc, where they truncated it to 0.
    #[test]
    fn firing_rule_tools_refuse_fractional_weights() {
        let net = json!({"@context": CONTEXT, "places": {"a": {"initial": [1]}}, "transitions": {"t": {}},
            "arcs": [{"source": "a", "target": "t", "weight": [0.5]}]});
        for tool in ["invariants", "incidence", "verify"] {
            let e = crate::call(tool, &json!({"net": net, "properties": [{"kind": "deadlock-free"}]})).unwrap_err();
            assert!(e.contains("arcs[0]") && e.contains("0.5"), "{tool}: {e}");
        }
        // a vector summing to a whole number is fine
        let ok = json!({"@context": CONTEXT, "places": {"a": {}}, "transitions": {"t": {}},
            "arcs": [{"source": "a", "target": "t", "weight": [0.5, 0.5]}]});
        crate::call("invariants", &json!({"net": ok})).unwrap();
        // invariants: none is [], not null
        let src = json!({"@context": CONTEXT, "places": {"a": {}}, "transitions": {"t": {}}, "arcs": [{"source": "t", "target": "a"}]});
        assert_eq!(crate::call("invariants", &json!({"net": src})).unwrap()["p_invariants"], json!([]));
    }

    #[test]
    fn what_a_document_cannot_carry_is_said_not_dropped() {
        let out = run(&json!({"model": {
            "places": [{"id": "d", "kind": "data"}], "transitions": [], "arcs": []
        }}))
        .unwrap();
        assert!(out["net"].is_null());
        assert!(out["unsupported"].as_str().unwrap().contains("\"d\""));
    }

    #[test]
    fn a_model_without_a_name_converts_and_takes_the_given_one() {
        let out = run(&json!({"model": {"places": [{"id": "a"}], "transitions": [], "arcs": []}, "name": "mine"})).unwrap();
        assert_eq!(out["net"]["name"], "mine");
        let out = run(&json!({"model": {"places": [{"id": "a"}], "transitions": [], "arcs": []}})).unwrap();
        assert!(out["net"].get("name").is_none());
    }

    #[test]
    fn editor_documents_pass_through_and_get_what_the_tools_need() {
        let out = run(&json!({"model": {
            "places": {"a": {"initial": [1]}}, "transitions": {"t": {}},
            "arcs": [{"source": "a", "target": "t"}]
        }}))
        .unwrap();
        assert_eq!(out["shape"], "jsonld");
        assert_eq!(out["net"]["@context"], CONTEXT);
        assert_eq!(out["layout"], "none", "an editor document is left as written");
        assert!(out["net"]["places"]["a"].get("x").is_none());
        assert!(out["cid"].is_string());
        // sealed documents keep their coordinates and their CID
        let known = out["net"].clone();
        let again = run(&json!({"model": known.clone()})).unwrap();
        assert_eq!(again["cid"], out["cid"]);
        assert_eq!(again["net"], known);
    }

    #[test]
    fn the_cid_is_computed_eagerly_only_up_to_the_limit() {
        let big = |n: usize| {
            let places: Vec<Value> = (0..n).map(|i| json!({"id": format!("p{i}"), "x": i + 1, "y": 1})).collect();
            json!({"places": places, "transitions": [], "arcs": []})
        };
        let small = run(&json!({"model": big(EAGER_CID_MAX_PLACES)})).unwrap();
        assert!(small["cid"].is_string());
        let over = run(&json!({"model": big(EAGER_CID_MAX_PLACES + 1)})).unwrap();
        assert!(over["cid"].is_null());
        assert!(over["cid_skipped"].as_str().unwrap().contains("cubic"));
        assert!(over["net"]["places"].as_object().unwrap().len() > EAGER_CID_MAX_PLACES);
        // few places, many transitions: the size guard stops it too
        let wide: Vec<Value> = (0..EAGER_CID_MAX_NODES).map(|i| json!({"id": format!("t{i}")})).collect();
        let w = run(&json!({"model": {"places": [{"id": "p", "initial": 1}], "transitions": wide, "arcs": []}})).unwrap();
        assert!(w["cid"].is_null(), "many transitions must not be sealed eagerly");
        assert!(w["cid_skipped"].as_str().unwrap().contains("cubic"));
        // and never when asked not to
        let never = run(&json!({"model": big(3), "cid": false})).unwrap();
        assert!(never["cid"].is_null());
    }
}
