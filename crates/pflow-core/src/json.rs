//! Shape A JSON support — the pflow.xyz editor document, a port of
//! go-pflow's `parser.FromJSON`.
//!
//! The ecosystem has two JSON shapes for a net: this one (places and
//! transitions as objects keyed by id, arcs with `source`/`target`,
//! per-color vectors, `inhibitTransition`, a CID as `@id` — the editor's
//! wire and identity format) and the metamodel shape (arrays with ids,
//! `from`/`to`, rate, schedule, ...) that every engine reads. Converting the
//! former into the latter is `pflow-parser`'s job; this module only reads
//! Shape A into a [`PetriNet`], exactly as go-pflow's `parser.FromJSON`
//! does.
//!
//! Unknown top-level and per-node fields (`@context`, `@type`, `parents`,
//! `offset`, ...) are tolerated, not rejected — the same as Go's
//! `encoding/json` default behaviour. Tests hold the accepted shape to a
//! stricter, `deny_unknown_fields` view instead of loosening the library.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::net::PetriNet;

/// Errors parsing a Shape A document.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A vector of per-color values, deserialized the way go-pflow's
/// `toFloatSlice` reads a place's `initial`/`capacity` or an arc's `weight`
/// field:
///
///   - an array: each element becomes its numeric value, or `0` if it is
///     `null` or not a finite number — a `null` slot is the editor's
///     serialisation of `Infinity` ("unbounded"), and keeping the slot
///     rather than dropping it keeps a mixed vector like `[5, null]`
///     aligned with its colors;
///   - a bare scalar number or numeric string: one component;
///   - absent or `null`: no components (`[]`) — "no colors declared".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FloatVec(pub Vec<f64>);

impl<'de> Deserialize<'de> for FloatVec {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(FloatVec(value_to_float_vec(&value)))
    }
}

fn value_to_float_vec(v: &Value) -> Vec<f64> {
    match v {
        Value::Null => vec![],
        Value::Array(items) => items.iter().map(value_to_float_or_zero).collect(),
        Value::Number(_) | Value::String(_) => match value_as_finite_f64(v) {
            Some(f) => vec![f],
            None => vec![],
        },
        _ => vec![],
    }
}

fn value_to_float_or_zero(v: &Value) -> f64 {
    value_as_finite_f64(v).unwrap_or(0.0)
}

fn value_as_finite_f64(v: &Value) -> Option<f64> {
    let f = match v {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.parse::<f64>().ok()?,
        _ => return None,
    };
    if f.is_finite() {
        Some(f)
    } else {
        None
    }
}

fn default_role() -> String {
    "default".to_string()
}

/// A place as it appears under Shape A's `places` object.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawPlace {
    #[serde(default)]
    pub initial: FloatVec,
    #[serde(default)]
    pub capacity: FloatVec,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub label: Option<String>,
}

/// A transition as it appears under Shape A's `transitions` object.
#[derive(Debug, Clone, Deserialize)]
pub struct RawTransition {
    #[serde(default = "default_role")]
    pub role: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub label: Option<String>,
}

/// An arc as it appears in Shape A's `arcs` array.
#[derive(Debug, Clone, Deserialize)]
pub struct RawArc {
    pub source: String,
    pub target: String,
    /// Absent or explicit `null` both mean "not declared" and default to
    /// `[1]` once converted (see [`ShapeADocument::into_net`]) — matching
    /// go-pflow's `FromJSON`, where a JSON `null` unmarshals into a `nil`
    /// interface indistinguishable from an absent key.
    #[serde(default)]
    pub weight: Option<FloatVec>,
    #[serde(default, rename = "inhibitTransition")]
    pub inhibit_transition: bool,
}

/// The pflow.xyz editor "Shape A" document.
///
/// ```json
/// {
///   "@id": "z4EB...",
///   "token": ["color1", "color2"],
///   "places": {
///     "p1": {"initial": [1, 0], "capacity": [10, 10], "x": 100, "y": 100, "label": "Place 1"}
///   },
///   "transitions": {
///     "t1": {"role": "default", "x": 200, "y": 100, "label": "Transition 1"}
///   },
///   "arcs": [
///     {"source": "p1", "target": "t1", "weight": [1, 0], "inhibitTransition": false}
///   ]
/// }
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ShapeADocument {
    /// The document's content identifier. Carried but not otherwise
    /// interpreted by this crate — computing or verifying it is
    /// `pflow-tokenmodel`/CID territory.
    #[serde(rename = "@id", default)]
    pub id: Option<String>,
    #[serde(default)]
    pub token: Vec<String>,
    #[serde(default)]
    pub places: HashMap<String, RawPlace>,
    #[serde(default)]
    pub transitions: HashMap<String, RawTransition>,
    #[serde(default)]
    pub arcs: Vec<RawArc>,
}

impl ShapeADocument {
    /// Parses a Shape A document from JSON bytes.
    pub fn from_json(data: &[u8]) -> Result<Self> {
        Ok(serde_json::from_slice(data)?)
    }

    /// Converts a parsed document into a [`PetriNet`] — the port of
    /// go-pflow's `parser.FromJSON`.
    pub fn into_net(self) -> PetriNet {
        let mut net = PetriNet::new();
        net.token = self.token;

        for (id, p) in self.places {
            net.add_place(id, p.initial.0, p.capacity.0, p.x, p.y, p.label);
        }
        for (id, t) in self.transitions {
            net.add_transition(id, t.role, t.x, t.y, t.label);
        }
        for a in self.arcs {
            let weight = match a.weight {
                Some(fv) => fv.0,
                None => vec![1.0],
            };
            net.add_arc(a.source, a.target, weight, a.inhibit_transition);
        }
        net
    }
}

/// Parses a Shape A document into a [`PetriNet`] in one step — the direct
/// counterpart to go-pflow's `parser.FromJSON`.
pub fn from_json(data: &[u8]) -> Result<PetriNet> {
    Ok(ShapeADocument::from_json(data)?.into_net())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_places_transitions_and_arcs() {
        let doc = br#"{
            "@id": "z4EBexample",
            "token": ["red", "blue"],
            "places": {
                "p1": {"initial": [1, 0], "capacity": [10, 10], "x": 100, "y": 100, "label": "Place 1"}
            },
            "transitions": {
                "t1": {"role": "default", "x": 200, "y": 100, "label": "Transition 1"}
            },
            "arcs": [
                {"source": "p1", "target": "t1", "weight": [1, 0], "inhibitTransition": false}
            ]
        }"#;

        let net = from_json(doc).unwrap();
        assert_eq!(net.token, vec!["red".to_string(), "blue".to_string()]);
        assert_eq!(net.places.len(), 1);
        assert_eq!(net.places["p1"].initial, vec![1.0, 0.0]);
        assert_eq!(net.places["p1"].capacity, vec![10.0, 10.0]);
        assert_eq!(net.places["p1"].label_text, Some("Place 1".to_string()));
        assert_eq!(net.transitions.len(), 1);
        assert_eq!(net.transitions["t1"].role, "default");
        assert_eq!(net.arcs.len(), 1);
        assert_eq!(net.arcs[0].weight, vec![1.0, 0.0]);
        assert!(!net.arcs[0].inhibit_transition);
    }

    #[test]
    fn null_vector_slots_read_as_zero() {
        let doc = br#"{
            "places": {
                "p": {"initial": [5, null, 2], "capacity": [null]}
            }
        }"#;
        let net = from_json(doc).unwrap();
        assert_eq!(net.places["p"].initial, vec![5.0, 0.0, 2.0]);
        assert_eq!(net.places["p"].capacity, vec![0.0]);
    }

    #[test]
    fn missing_initial_and_capacity_are_empty() {
        let doc = br#"{"places": {"p": {}}}"#;
        let net = from_json(doc).unwrap();
        assert!(net.places["p"].initial.is_empty());
        assert!(net.places["p"].capacity.is_empty());
    }

    #[test]
    fn arc_weight_defaults_to_one_when_absent_or_null() {
        let doc = br#"{
            "arcs": [
                {"source": "a", "target": "t"},
                {"source": "b", "target": "t", "weight": null}
            ]
        }"#;
        let net = from_json(doc).unwrap();
        assert_eq!(net.arcs[0].weight, vec![1.0]);
        assert_eq!(net.arcs[1].weight, vec![1.0]);
    }

    #[test]
    fn arc_weight_empty_array_stays_empty() {
        let doc = br#"{"arcs": [{"source": "a", "target": "t", "weight": []}]}"#;
        let net = from_json(doc).unwrap();
        assert!(net.arcs[0].weight.is_empty());
    }

    #[test]
    fn transition_role_defaults_to_default() {
        let doc = br#"{"transitions": {"t": {}}}"#;
        let net = from_json(doc).unwrap();
        assert_eq!(net.transitions["t"].role, "default");
    }

    #[test]
    fn inhibit_transition_flag_is_read() {
        let doc = br#"{"arcs": [{"source": "p", "target": "t", "inhibitTransition": true}]}"#;
        let net = from_json(doc).unwrap();
        assert!(net.arcs[0].inhibit_transition);
    }

    #[test]
    fn unknown_top_level_and_node_fields_are_tolerated() {
        let doc = br#"{
            "@context": "https://pflow.xyz/schema",
            "@type": "PetriNet",
            "parents": {},
            "places": {"p": {"@type": "Place", "offset": 0, "initial": [1]}},
            "transitions": {"t": {"@type": "Transition", "offset": 0}},
            "arcs": [{"@type": "Arrow", "source": "p", "target": "t"}]
        }"#;
        let net = from_json(doc).expect("unknown fields must not fail parsing");
        assert_eq!(net.places["p"].initial, vec![1.0]);
    }

    #[test]
    fn empty_document_parses_to_empty_net() {
        let net = from_json(b"{}").unwrap();
        assert!(net.places.is_empty());
        assert!(net.transitions.is_empty());
        assert!(net.arcs.is_empty());
        assert!(net.token.is_empty());
    }

    /// The accepted-fields contract, checked separately from the tolerant
    /// library type: ground rule is "unknown fields rejected in tests,
    /// tolerated in the library". A doc using only the fields below must
    /// still round-trip through the stricter view.
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StrictDocument {
        #[serde(rename = "@id", default)]
        #[allow(dead_code)]
        id: Option<String>,
        #[serde(default)]
        #[allow(dead_code)]
        token: Vec<String>,
        #[serde(default)]
        #[allow(dead_code)]
        places: HashMap<String, StrictPlace>,
        #[serde(default)]
        #[allow(dead_code)]
        transitions: HashMap<String, StrictTransition>,
        #[serde(default)]
        #[allow(dead_code)]
        arcs: Vec<StrictArc>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StrictPlace {
        #[serde(default)]
        #[allow(dead_code)]
        initial: FloatVec,
        #[serde(default)]
        #[allow(dead_code)]
        capacity: FloatVec,
        #[serde(default)]
        #[allow(dead_code)]
        x: f64,
        #[serde(default)]
        #[allow(dead_code)]
        y: f64,
        #[serde(default)]
        #[allow(dead_code)]
        label: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StrictTransition {
        #[serde(default = "default_role")]
        #[allow(dead_code)]
        role: String,
        #[serde(default)]
        #[allow(dead_code)]
        x: f64,
        #[serde(default)]
        #[allow(dead_code)]
        y: f64,
        #[serde(default)]
        #[allow(dead_code)]
        label: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StrictArc {
        #[allow(dead_code)]
        source: String,
        #[allow(dead_code)]
        target: String,
        #[serde(default)]
        #[allow(dead_code)]
        weight: Option<FloatVec>,
        #[serde(default, rename = "inhibitTransition")]
        #[allow(dead_code)]
        inhibit_transition: bool,
    }

    #[test]
    fn known_shape_is_accepted_strictly() {
        let doc = br#"{
            "@id": "z4EBexample",
            "token": ["red"],
            "places": {"p": {"initial": [1], "capacity": [0], "x": 1, "y": 2, "label": "P"}},
            "transitions": {"t": {"role": "default", "x": 3, "y": 4, "label": "T"}},
            "arcs": [{"source": "p", "target": "t", "weight": [1], "inhibitTransition": false}]
        }"#;
        let strict: std::result::Result<StrictDocument, _> = serde_json::from_slice(doc);
        assert!(strict.is_ok(), "{strict:?}");
    }

    #[test]
    fn unknown_field_rejected_by_strict_view() {
        let doc = br#"{"places": {}, "bogus": true}"#;
        let strict: std::result::Result<StrictDocument, _> = serde_json::from_slice(doc);
        assert!(strict.is_err(), "strict view must reject an unknown field");

        // The library itself tolerates the same document.
        assert!(from_json(doc).is_ok());
    }
}
