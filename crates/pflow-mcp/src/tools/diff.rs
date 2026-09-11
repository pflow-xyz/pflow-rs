//! `petri_diff`: structural differences between two models
//! (added/removed places, transitions, arcs), via
//! [`pflow_compose::extend::compare_models`]. Mirrors petri-pilot's
//! `petri_diff` tool (`pkg/mcp/server.go`'s `diffTool`/`compareModels`).

use super::convert::parse_any_model;
use pflow_compose::compare_models;
use serde::Serialize;

#[derive(Serialize)]
struct DiffResult {
    places_added: Vec<String>,
    places_removed: Vec<String>,
    transitions_added: Vec<String>,
    transitions_removed: Vec<String>,
    arcs_added: Vec<String>,
    arcs_removed: Vec<String>,
    has_changes: bool,
}

pub fn run(model_a: &str, model_b: &str) -> Result<String, String> {
    let a = parse_any_model(model_a)?;
    let b = parse_any_model(model_b)?;
    let diff = compare_models(&a, &b);

    let out = DiffResult {
        places_added: diff.places_added,
        places_removed: diff.places_removed,
        transitions_added: diff.transitions_added,
        transitions_removed: diff.transitions_removed,
        arcs_added: diff.arcs_added,
        arcs_removed: diff.arcs_removed,
        has_changes: diff.has_changes,
    };

    serde_json::to_string_pretty(&out).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_added_place() {
        let a = r#"(schema m (states (state p1 :kind token :initial 1)) (actions) (arcs))"#;
        let b = r#"(schema m (states (state p1 :kind token :initial 1) (state p2 :kind token :initial 0)) (actions) (arcs))"#;
        let result = run(a, b).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["has_changes"], true);
        assert_eq!(v["places_added"][0], "p2");
    }

    #[test]
    fn no_changes_when_identical() {
        let a = r#"(schema m (states (state p1 :kind token :initial 1)) (actions) (arcs))"#;
        let result = run(a, a).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["has_changes"], false);
    }
}
