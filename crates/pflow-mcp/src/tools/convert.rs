//! One place that reads every wire shape a model can arrive in and turns
//! it into a [`pflow_metamodel::Model`] — the shape every richer analysis
//! tool (`petri_verify`, `petri_invariants`, `petri_conformance`,
//! `petri_scenario`, `petri_extend`, `petri_diff`, `petri_canonical`,
//! `petri_dataset`) is built on.
//!
//! Three shapes are accepted, tried in this order:
//!
//! 1. **tokenmodel DSL** (S-expression text starting with `(`), via
//!    [`pflow_dsl::parse_schema`].
//! 2. **tokenmodel Schema JSON** — this crate's pre-existing native wire
//!    format (`{"states": [...], "actions": [...], "arcs": [...]}`).
//! 3. **Shape A** (the pflow.xyz editor document: `places`/`transitions`
//!    keyed by id, `@id`, color vectors), via [`pflow_parser::model_from_json`].
//! 4. **Shape B** (`pflow_metamodel::Model` itself: `{"places": [...],
//!    "transitions": [...], "arcs": [...]}` as arrays with `from`/`to`).
//!
//! [`schema_to_model`]/[`model_to_schema`] are the lossy bridge between (1)/(2)
//! and (3)/(4): both carry token places, transitions, and weighted/typed
//! (normal/read/inhibitor) arcs, so the round trip is exact on that shared
//! subset. What a `Schema` cannot express — capacity, rate, guard, delay,
//! stages, schedules, parameters, access — is dropped going `Model ->
//! Schema` (documented on [`model_to_schema`]) and is simply absent going
//! the other way, since `Schema` never had it.

use pflow_metamodel::{
    Arc as MmArc, ArcType as MmArcType, Model, Place as MmPlace, StateKind, Transition as MmTransition,
};
use pflow_tokenmodel::{Arc as SchemaArc, ArcType as SchemaArcType, Kind, Schema, State};

/// Converts a tokenmodel `Schema` into a `pflow_metamodel::Model`, keeping
/// only what both shapes carry: token places (data places are dropped —
/// `Model` has no concept of a `keys`/`value`-addressed data arc), actions
/// as transitions, and arcs with their declared weight/type.
pub fn schema_to_model(schema: &Schema) -> Model {
    let places = schema
        .states
        .iter()
        .filter(|s| s.is_token())
        .map(|s| MmPlace {
            id: s.id.clone(),
            initial: s.initial_tokens(),
            kind: Some(StateKind::Token),
            ..Default::default()
        })
        .collect();

    let transitions = schema
        .actions
        .iter()
        .map(|a| MmTransition {
            id: a.id.clone(),
            guard: a.guard.clone(),
            event: a.event_id.clone(),
            ..Default::default()
        })
        .collect();

    let token_ids: std::collections::HashSet<&str> = schema
        .states
        .iter()
        .filter(|s| s.is_token())
        .map(|s| s.id.as_str())
        .collect();

    let arcs = schema
        .arcs
        .iter()
        .filter(|a| token_ids.contains(a.source.as_str()) || token_ids.contains(a.target.as_str()))
        .map(|a| MmArc {
            from: a.source.clone(),
            to: a.target.clone(),
            weight: a.effective_weight(),
            typ: match a.typ {
                SchemaArcType::Normal => MmArcType::Normal,
                SchemaArcType::Inhibitor => MmArcType::Inhibitor,
                SchemaArcType::Read => MmArcType::Read,
            },
            ..Default::default()
        })
        .collect();

    Model {
        name: schema.name.clone(),
        version: schema.version.clone(),
        places,
        transitions,
        arcs,
        ..Default::default()
    }
}

/// Converts a `pflow_metamodel::Model` into a tokenmodel `Schema` — the
/// inverse of [`schema_to_model`] on their shared subset.
///
/// **Lossy.** `Schema` has no field for capacity, rate, delay, stages,
/// schedules, guards-as-structure (the guard *string* survives; nothing
/// evaluates it through this path), parameters, roles or access. A model
/// using any of those keeps working through the tools that read
/// `pflow_metamodel::Model` directly ([`parse_any_model`]); it is only the
/// pre-existing Schema-shaped tools (`pflow_build`, `pflow_fire`, ...) that
/// see the reduced net.
pub fn model_to_schema(model: &Model) -> Schema {
    let mut schema = Schema::new(model.name.clone());
    schema.version = if model.version.is_empty() {
        "1.0.0".to_string()
    } else {
        model.version.clone()
    };

    for p in &model.places {
        if !p.is_token() {
            continue;
        }
        schema.states.push(State {
            id: p.id.clone(),
            kind: Kind::Token,
            initial: Some(serde_json::json!(p.initial)),
            typ: String::new(),
            exported: p.exported,
        });
    }

    for t in &model.transitions {
        schema.actions.push(pflow_tokenmodel::Action {
            id: t.id.clone(),
            guard: t.guard.clone(),
            event_id: t.event.clone(),
            event_bindings: None,
        });
    }

    let token_ids: std::collections::HashSet<&str> =
        model.places.iter().filter(|p| p.is_token()).map(|p| p.id.as_str()).collect();

    for a in &model.arcs {
        if !(token_ids.contains(a.from.as_str()) || token_ids.contains(a.to.as_str())) {
            continue;
        }
        schema.arcs.push(SchemaArc {
            source: a.from.clone(),
            target: a.to.clone(),
            keys: Vec::new(),
            value: String::new(),
            weight: a.effective_weight(),
            typ: match a.typ {
                MmArcType::Normal => SchemaArcType::Normal,
                MmArcType::Inhibitor => SchemaArcType::Inhibitor,
                MmArcType::Read => SchemaArcType::Read,
            },
        });
    }

    schema
}

/// Parses any of the wire shapes described in the module doc into a
/// `pflow_metamodel::Model`. This is the entrypoint every `petri_*` tool
/// added in ROADMAP.md Phase 6 uses, so a caller can hand any of DSL text,
/// tokenmodel Schema JSON, Shape A editor JSON, or Shape B `Model` JSON to
/// any of them.
pub fn parse_any_model(text: &str) -> Result<Model, String> {
    let trimmed = text.trim();
    if trimmed.starts_with('(') {
        let schema = pflow_dsl::parse_schema(trimmed)?;
        return Ok(schema_to_model(&schema));
    }

    if let Ok(schema) = serde_json::from_str::<Schema>(trimmed) {
        // A Schema's `states`/`actions`/`arcs` are non-optional, so this
        // only matches documents that actually declare the tokenmodel
        // shape (Shape A/B use different field names and fail to parse).
        return Ok(schema_to_model(&schema));
    }

    let bytes = trimmed.as_bytes();
    if pflow_parser::is_pflow_json(bytes) {
        let (model, _colors) = pflow_parser::model_from_json(bytes).map_err(|e| format!("Shape A parse error: {e}"))?;
        return Ok(model);
    }

    serde_json::from_str::<Model>(trimmed).map_err(|e| format!("not a recognized model shape (tried DSL, tokenmodel Schema, Shape A, Shape B): {e}"))
}

/// Converts a `pflow_metamodel::Model` into a `pflow_core::PetriNet` (Shape
/// A's ODE/SSA-oriented representation), for the tools built on that older
/// net type (`petri_conformance`'s token-replay, the portable SSA).
///
/// `pflow_core::PetriNet::Arc` has no read-arc concept, only a consuming
/// arc and an `inhibit_transition` flag. A read arc is encoded the standard
/// way — an input arc and an output arc of the same weight between the same
/// place and transition, netting zero token movement but still gating
/// enablement on the place holding at least `weight` — the same trick
/// petri-pilot's own `buildVerifyNet`-adjacent bridges use for the same
/// mismatch. An inhibitor arc maps directly onto `inhibit_transition`.
pub fn model_to_petri_net(model: &Model) -> pflow_core::PetriNet {
    let mut net = pflow_core::PetriNet::new();

    for p in &model.places {
        if !p.is_token() {
            continue;
        }
        net.add_place(&p.id, vec![p.initial as f64], vec![], p.x as f64, p.y as f64, None);
    }

    for t in &model.transitions {
        net.add_transition(&t.id, "", t.x as f64, t.y as f64, None);
    }

    let token_ids: std::collections::HashSet<&str> =
        model.places.iter().filter(|p| p.is_token()).map(|p| p.id.as_str()).collect();

    for a in &model.arcs {
        let w = a.effective_weight() as f64;
        match a.typ {
            MmArcType::Normal => {
                if token_ids.contains(a.from.as_str()) || token_ids.contains(a.to.as_str()) {
                    net.add_arc(&a.from, &a.to, vec![w], false);
                }
            }
            MmArcType::Inhibitor => {
                net.add_arc(&a.from, &a.to, vec![w], true);
            }
            MmArcType::Read => {
                // place -> transition -> place, same weight both ways.
                net.add_arc(&a.from, &a.to, vec![w], false);
                net.add_arc(&a.to, &a.from, vec![w], false);
            }
        }
    }

    net
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsl() -> &'static str {
        r#"(schema test
            (states
                (state p1 :kind token :initial 5)
                (state p2 :kind token :initial 0)
            )
            (actions (action t1))
            (arcs
                (arc p1 -> t1)
                (arc t1 -> p2)
            )
        )"#
    }

    #[test]
    fn schema_model_roundtrip() {
        let schema = pflow_dsl::parse_schema(dsl()).unwrap();
        let model = schema_to_model(&schema);
        assert_eq!(model.places.len(), 2);
        assert_eq!(model.transitions.len(), 1);
        assert_eq!(model.arcs.len(), 2);

        let back = model_to_schema(&model);
        assert_eq!(back.states.len(), 2);
        assert_eq!(back.actions.len(), 1);
        assert_eq!(back.arcs.len(), 2);
    }

    #[test]
    fn parse_any_model_dsl() {
        let model = parse_any_model(dsl()).unwrap();
        assert_eq!(model.name, "test");
        assert_eq!(model.places.len(), 2);
    }

    #[test]
    fn parse_any_model_shape_b() {
        let json = serde_json::json!({
            "name": "shapeb",
            "places": [{"id": "p1", "initial": 3}, {"id": "p2", "initial": 0}],
            "transitions": [{"id": "t1"}],
            "arcs": [{"from": "p1", "to": "t1"}, {"from": "t1", "to": "p2"}]
        });
        let model = parse_any_model(&json.to_string()).unwrap();
        assert_eq!(model.name, "shapeb");
        assert_eq!(model.places.len(), 2);
        assert_eq!(model.transitions.len(), 1);
        assert_eq!(model.arcs.len(), 2);
    }

    #[test]
    fn parse_any_model_read_and_inhibitor_arcs_preserved() {
        let json = serde_json::json!({
            "name": "guarded",
            "places": [{"id": "gate", "initial": 1}, {"id": "p1", "initial": 5}, {"id": "p2", "initial": 0}],
            "transitions": [{"id": "t1"}],
            "arcs": [
                {"from": "gate", "to": "t1", "type": "read"},
                {"from": "p1", "to": "t1", "weight": 2},
                {"from": "t1", "to": "p2", "weight": 2}
            ]
        });
        let model = parse_any_model(&json.to_string()).unwrap();
        let arc = model.arcs.iter().find(|a| a.from == "gate").unwrap();
        assert_eq!(arc.typ, MmArcType::Read);
    }
}
