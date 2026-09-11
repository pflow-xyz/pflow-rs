//! Reads a model file in any of the shapes the ecosystem uses (tokenmodel
//! DSL, pflow.xyz Shape A JSON, `pflow_metamodel::Model` Shape B JSON) and
//! gives back both representations a command might need: the Shape B
//! `Model` every structural/verification crate reads, and a
//! `pflow_core::PetriNet` for the ODE solver and SVG renderer, which still
//! speak Shape A's vocabulary.
//!
//! This mirrors `pflow-mcp`'s `tools::convert` module in spirit (same three
//! shapes, same "try Shape A, then Shape B" order) but is not the same
//! code: the MCP crate is a library dependency of nothing, and a CLI binary
//! depending on another binary's crate is not a thing Cargo does cleanly,
//! so the small amount of shape-detection logic is duplicated here rather
//! than factored into a third crate for two call sites.

use pflow_core::PetriNet;
use pflow_metamodel::{ArcType, Model};
use std::path::Path;

pub fn read_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))
}

/// Parses model text into a Shape B `Model`, trying DSL, Shape A, then
/// Shape B in that order.
pub fn parse_model(text: &str) -> Result<Model, String> {
    let trimmed = text.trim();
    if trimmed.starts_with('(') {
        let schema = pflow_dsl::parse_schema(trimmed)?;
        return Ok(schema_to_model(&schema));
    }
    let bytes = trimmed.as_bytes();
    if pflow_parser::is_pflow_json(bytes) {
        let (model, _colors) = pflow_parser::model_from_json(bytes).map_err(|e| e.to_string())?;
        return Ok(model);
    }
    serde_json::from_str::<Model>(trimmed).map_err(|e| format!("not a recognized model shape (tried DSL, Shape A, Shape B): {e}"))
}

pub fn load_model(path: &str) -> Result<Model, String> {
    parse_model(&read_file(path)?)
}

/// Converts a tokenmodel `Schema` (the DSL's native shape) into `Model`,
/// keeping only token places/actions/arcs — see `pflow-mcp`'s
/// `tools::convert::schema_to_model` for the full rationale; duplicated
/// here in miniature for the same reason as this module's doc comment.
fn schema_to_model(schema: &pflow_tokenmodel::Schema) -> Model {
    use pflow_metamodel::{Arc as MmArc, Place as MmPlace, StateKind, Transition as MmTransition};
    use pflow_tokenmodel::ArcType as SchemaArcType;

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
        .map(|a| MmTransition { id: a.id.clone(), guard: a.guard.clone(), event: a.event_id.clone(), ..Default::default() })
        .collect();

    let token_ids: std::collections::HashSet<&str> =
        schema.states.iter().filter(|s| s.is_token()).map(|s| s.id.as_str()).collect();

    let arcs = schema
        .arcs
        .iter()
        .filter(|a| token_ids.contains(a.source.as_str()) || token_ids.contains(a.target.as_str()))
        .map(|a| MmArc {
            from: a.source.clone(),
            to: a.target.clone(),
            weight: a.effective_weight(),
            typ: match a.typ {
                SchemaArcType::Normal => ArcType::Normal,
                SchemaArcType::Inhibitor => ArcType::Inhibitor,
                SchemaArcType::Read => ArcType::Read,
            },
            ..Default::default()
        })
        .collect();

    Model { name: schema.name.clone(), version: schema.version.clone(), places, transitions, arcs, ..Default::default() }
}

/// Converts a `Model` into a `pflow_core::PetriNet` — the ODE/SSA/SVG
/// representation. Read arcs become an input+output pair of the same
/// weight (nets to zero movement, still gates on the marking); inhibitor
/// arcs map onto `inhibit_transition`. See `pflow-mcp`'s
/// `tools::convert::model_to_petri_net` for the identical rationale.
pub fn model_to_petri_net(model: &Model) -> PetriNet {
    let mut net = PetriNet::new();
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
            ArcType::Normal => {
                if token_ids.contains(a.from.as_str()) || token_ids.contains(a.to.as_str()) {
                    net.add_arc(&a.from, &a.to, vec![w], false);
                }
            }
            ArcType::Inhibitor => net.add_arc(&a.from, &a.to, vec![w], true),
            ArcType::Read => {
                net.add_arc(&a.from, &a.to, vec![w], false);
                net.add_arc(&a.to, &a.from, vec![w], false);
            }
        }
    }
    net
}

/// Loads a `pflow_core::PetriNet` directly: Shape A parses natively; DSL
/// and Shape B route through [`parse_model`] then [`model_to_petri_net`].
pub fn load_petri_net(path: &str) -> Result<PetriNet, String> {
    let text = read_file(path)?;
    let trimmed = text.trim();
    if !trimmed.starts_with('(') && pflow_parser::is_pflow_json(trimmed.as_bytes()) {
        return pflow_core::from_json(trimmed.as_bytes()).map_err(|e| e.to_string());
    }
    let model = parse_model(&text)?;
    Ok(model_to_petri_net(&model))
}

/// Writes a `pflow_core::PetriNet` back out as Shape A JSON — there is no
/// Shape A serializer in `pflow-core` (it only reads Shape A; every writer
/// in the ecosystem produces Shape B), so this is a minimal one, just
/// enough for `pflow create`/`pflow expand` to hand back something
/// `pflow_core::from_json`/the pflow.xyz editor can read.
pub fn petri_net_to_shape_a_json(net: &PetriNet, name: &str) -> serde_json::Value {
    let mut places = serde_json::Map::new();
    for (id, p) in &net.places {
        places.insert(
            id.clone(),
            serde_json::json!({"initial": p.initial, "capacity": p.capacity, "x": p.x, "y": p.y}),
        );
    }
    let mut transitions = serde_json::Map::new();
    for (id, t) in &net.transitions {
        transitions.insert(id.clone(), serde_json::json!({"role": t.role, "x": t.x, "y": t.y}));
    }
    let arcs: Vec<serde_json::Value> = net
        .arcs
        .iter()
        .map(|a| {
            serde_json::json!({
                "source": a.source,
                "target": a.target,
                "weight": a.weight,
                "inhibitTransition": a.inhibit_transition,
            })
        })
        .collect();
    serde_json::json!({
        "name": name,
        "token": net.token,
        "places": places,
        "transitions": transitions,
        "arcs": arcs,
    })
}

pub fn write_output(json: &serde_json::Value, output: Option<&str>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(json).map_err(|e| e.to_string())?;
    match output {
        Some(path) => std::fs::write(path, text).map_err(|e| format!("write {path}: {e}")),
        None => {
            println!("{text}");
            Ok(())
        }
    }
}

pub fn model_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".to_string())
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
            (arcs (arc p1 -> t1) (arc t1 -> p2))
        )"#
    }

    #[test]
    fn parse_model_reads_dsl() {
        let model = parse_model(dsl()).unwrap();
        assert_eq!(model.places.len(), 2);
        assert_eq!(model.transitions.len(), 1);
    }

    #[test]
    fn parse_model_reads_shape_b() {
        let json = serde_json::json!({
            "name": "shapeb",
            "places": [{"id": "p1", "initial": 3}],
            "transitions": [{"id": "t1"}],
            "arcs": [{"from": "p1", "to": "t1"}]
        });
        let model = parse_model(&json.to_string()).unwrap();
        assert_eq!(model.name, "shapeb");
        assert_eq!(model.places.len(), 1);
    }

    #[test]
    fn model_to_petri_net_preserves_topology() {
        let model = parse_model(dsl()).unwrap();
        let net = model_to_petri_net(&model);
        assert_eq!(net.places.len(), 2);
        assert_eq!(net.transitions.len(), 1);
        assert_eq!(net.places["p1"].initial, vec![5.0]);
    }

    #[test]
    fn shape_a_json_round_trips_through_from_json() {
        let model = parse_model(dsl()).unwrap();
        let net = model_to_petri_net(&model);
        let json = petri_net_to_shape_a_json(&net, "test");
        let bytes = serde_json::to_vec(&json).unwrap();
        let reread = pflow_core::from_json(&bytes).unwrap();
        assert_eq!(reread.places.len(), net.places.len());
        assert_eq!(reread.transitions.len(), net.transitions.len());
    }
}
