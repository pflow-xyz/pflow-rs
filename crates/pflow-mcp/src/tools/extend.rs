//! `petri_extend`: applies structural operations to a model and returns the
//! result, via [`pflow_compose::apply_operations`]. Mirrors petri-pilot's
//! `pkg/mcp/server.go` (`applyOperation` op set): `add_place`,
//! `add_transition`, `add_arc`, `add_event`, `add_event_field`,
//! `add_binding`, `remove_place`, `remove_transition`, `remove_arc`,
//! `remove_event`, `remove_binding`.

use super::convert::parse_any_model;
use pflow_compose::{apply_operations, parse_operations};
use serde::Serialize;

#[derive(Serialize)]
struct ExtendResult {
    applied: Vec<String>,
    errors: Vec<String>,
    model: pflow_metamodel::Model,
}

pub fn run(model: &str, operations_json: &str) -> Result<String, String> {
    let mut model = parse_any_model(model)?;
    let ops = parse_operations(operations_json)?;
    let (applied, errors) = apply_operations(&mut model, &ops);

    let out = ExtendResult { applied, errors, model };
    serde_json::to_string_pretty(&out).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_a_place() {
        let dsl = r#"(schema m (states (state p1 :kind token :initial 1)) (actions) (arcs))"#;
        let ops = r#"[{"op":"add_place","id":"p2","initial":0}]"#;
        let result = run(dsl, ops).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["applied"], serde_json::json!(["add_place"]));
        assert!(v["errors"].as_array().unwrap().is_empty());
        assert_eq!(v["model"]["places"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn reports_errors_without_stopping() {
        let dsl = r#"(schema m (states (state p1 :kind token :initial 1)) (actions) (arcs))"#;
        let ops = r#"[{"op":"add_place"},{"op":"add_place","id":"p3"}]"#;
        let result = run(dsl, ops).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["applied"], serde_json::json!(["add_place"]));
        assert_eq!(v["errors"].as_array().unwrap().len(), 1);
    }
}
