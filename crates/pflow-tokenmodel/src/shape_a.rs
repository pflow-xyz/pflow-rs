//! The Shape A writer: a [`pflow_core::PetriNet`] as a sealed pflow.xyz editor
//! document, with `@context`, `@type`, `@version` and its CID as `@id`. The
//! counterpart of `pflow_core::from_json`, and of pflow-jl's `to_json`.
//!
//! It lives here, not in `pflow-core`, because the CID does: `pflow-core` has
//! no pflow dependencies and sits below this crate.
//!
//! **Deterministic.** Places and transitions are written in id order, whatever
//! the `HashMap`s hold, and whichever way `serde_json` orders its maps. Arcs
//! keep their order, since `arcs` is a list and its order is part of the CID.
//! An integral number is written as a JSON integer, because `1` and `1.0` are
//! different literals in the CID's preimage (`xsd:integer`, `xsd:double`).
//!
//! **What the CID does and does not reproduce.** The CID hashes the document,
//! and [`to_shape_a`] can only write what a `PetriNet` holds. A document whose
//! identity rests on something else gets a new CID when re-written from its
//! net, not the one it came in with:
//!
//!   - top-level fields a net has no slot for (`parents`, `author`, ...);
//!   - a `null` in a vector, which the reader takes as `0` (a `null` entry
//!     hashes as an absent one, `0` as a literal);
//!   - an absent `role`, `inhibitTransition`, `initial`, `capacity`, `token` or
//!     `@version`, which the reader defaults and the writer then states.
//!
//! To seal a document as it stands, without going through a net, use
//! [`seal_shape_a`]. Both are held to the cross-language goldens in
//! `tests/shape_a.rs`.

use std::collections::BTreeMap;

use pflow_core::PetriNet;
use serde_json::{json, Map, Value};

use crate::canonical_cid::{compute_cid, CanonicalCidError};

/// The `@context` every Shape A document names.
pub const SHAPE_A_CONTEXT: &str = "https://pflow.xyz/schema";
/// The `@type` of a Shape A document.
pub const SHAPE_A_TYPE: &str = "PetriNet";
/// The `@version` this writer stamps.
pub const SHAPE_A_VERSION: &str = "1.1";

/// `net` as a sealed Shape A document. See the module docs.
pub fn to_shape_a(net: &PetriNet) -> Result<Value, CanonicalCidError> {
    let mut places: Vec<_> = net.places.iter().collect();
    places.sort_by_key(|(id, _)| *id);
    let mut place_nodes = Map::new();
    for (id, p) in places {
        let mut node = BTreeMap::from([
            ("@type", json!("Place")),
            ("capacity", numbers(&p.capacity)),
            ("initial", numbers(&p.initial)),
            ("x", number(p.x)),
            ("y", number(p.y)),
        ]);
        if let Some(label) = &p.label_text {
            node.insert("label", json!(label));
        }
        if let Some(offset) = p.offset {
            node.insert("offset", json!(offset));
        }
        place_nodes.insert(id.clone(), object(node));
    }

    let mut transitions: Vec<_> = net.transitions.iter().collect();
    transitions.sort_by_key(|(id, _)| *id);
    let mut transition_nodes = Map::new();
    for (id, t) in transitions {
        // The reader gives an absent role this default, so writing it keeps a
        // written document stable across a round trip.
        let role = if t.role.is_empty() { "default" } else { t.role.as_str() };
        let mut node = BTreeMap::from([
            ("@type", json!("Transition")),
            ("role", json!(role)),
            ("x", number(t.x)),
            ("y", number(t.y)),
        ]);
        if let Some(label) = &t.label_text {
            node.insert("label", json!(label));
        }
        if let Some(offset) = t.offset {
            node.insert("offset", json!(offset));
        }
        transition_nodes.insert(id.clone(), object(node));
    }

    let arcs: Vec<Value> = net
        .arcs
        .iter()
        .map(|a| {
            json!({
                "@type": "Arrow",
                "inhibitTransition": a.inhibit_transition,
                "source": a.source,
                "target": a.target,
                "weight": numbers(&a.weight),
            })
        })
        .collect();

    seal_shape_a(json!({
        "@context": SHAPE_A_CONTEXT,
        "@type": SHAPE_A_TYPE,
        "@version": SHAPE_A_VERSION,
        "arcs": arcs,
        "places": place_nodes,
        "token": net.token,
        "transitions": transition_nodes,
    }))
}

/// [`to_shape_a`] as text. The same net gives the same bytes on every run.
pub fn to_shape_a_string(net: &PetriNet) -> Result<String, CanonicalCidError> {
    let doc = to_shape_a(net)?;
    Ok(serde_json::to_string(&doc).expect("a JSON value serializes"))
}

/// Seals a Shape A document as it stands: sets `@id` to its CID, replacing any
/// `@id` it had, and changes nothing else. A document that is not a JSON object
/// is an error.
pub fn seal_shape_a(doc: Value) -> Result<Value, CanonicalCidError> {
    let raw = serde_json::to_vec(&doc).expect("a JSON value serializes");
    let (cid, _) = compute_cid(&raw)?;
    let Value::Object(mut fields) = doc else {
        return Err(CanonicalCidError::NotAnObject);
    };
    fields.insert("@id".to_string(), Value::String(cid));
    // Sorted however `serde_json` keeps its maps.
    let mut sorted: Vec<_> = fields.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Value::Object(sorted.into_iter().collect()))
}

fn object(fields: BTreeMap<&str, Value>) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn numbers(xs: &[f64]) -> Value {
    Value::Array(xs.iter().map(|&x| number(x)).collect())
}

/// An integral value as a JSON integer. A non-finite one is `null`, the editor's
/// spelling of "unbounded"; `serde_json` has no number for it anyway.
fn number(x: f64) -> Value {
    const EXACT: f64 = 9_007_199_254_740_992.0; // 2^53
    if x.fract() == 0.0 && x.abs() < EXACT {
        json!(x as i64)
    } else {
        json!(x)
    }
}
