//! The cross-implementation golden contract for reading the pflow.xyz editor
//! shape — a port of go-pflow's `parser.NormalizedNet`/`parser.Normalize`.
//!
//! go-pflow generates one golden per fixture (`cmd/shape-goldens`) under
//! `go-pflow/parser/testdata/editor-shape/*.json`; every other reader of the
//! editor shape (this crate, pflow-jl's `from_json`, pflow-xyz's `fromJSON` +
//! `expandColors`) replays the same input and must produce the same `parsed`
//! net and, where it unfolds colors, the same `expanded` net. [`normalize`]
//! renders a [`pflow_core::PetriNet`] in the golden's stable, comparable
//! shape — field names and JSON layout match go-pflow's `NormalizedNet`
//! exactly, so a golden parses into this type without loss.

use std::collections::HashMap;

use pflow_core::PetriNet;
use serde::{Deserialize, Serialize};

/// A [`pflow_core::PetriNet`] in a stable, comparable JSON shape — the
/// `parsed`/`expanded` sections of an editor-shape golden.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NormalizedNet {
    #[serde(default)]
    pub token: Vec<String>,
    #[serde(default)]
    pub places: HashMap<String, NormalizedPlace>,
    #[serde(default)]
    pub transitions: HashMap<String, NormalizedTransition>,
    #[serde(default)]
    pub arcs: Vec<NormalizedArc>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NormalizedPlace {
    #[serde(default)]
    pub initial: Vec<f64>,
    #[serde(default)]
    pub capacity: Vec<f64>,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NormalizedTransition {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NormalizedArc {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub weight: Vec<f64>,
    #[serde(default)]
    pub inhibit: bool,
}

/// Renders `net` in the golden's comparable shape. Arcs keep document
/// order; a `None` label becomes an empty string, matching Go's `omitempty`
/// on the label field.
pub fn normalize(net: &PetriNet) -> NormalizedNet {
    let mut out = NormalizedNet {
        token: net.token.clone(),
        places: HashMap::with_capacity(net.places.len()),
        transitions: HashMap::with_capacity(net.transitions.len()),
        arcs: Vec::with_capacity(net.arcs.len()),
    };
    for (id, p) in &net.places {
        out.places.insert(
            id.clone(),
            NormalizedPlace {
                initial: p.initial.clone(),
                capacity: p.capacity.clone(),
                x: p.x,
                y: p.y,
                label: p.label_text.clone().unwrap_or_default(),
            },
        );
    }
    for (id, t) in &net.transitions {
        out.transitions.insert(
            id.clone(),
            NormalizedTransition {
                role: t.role.clone(),
                x: t.x,
                y: t.y,
                label: t.label_text.clone().unwrap_or_default(),
            },
        );
    }
    for a in &net.arcs {
        out.arcs.push(NormalizedArc {
            source: a.source.clone(),
            target: a.target.clone(),
            weight: a.weight.clone(),
            inhibit: a.inhibit_transition,
        });
    }
    out
}
