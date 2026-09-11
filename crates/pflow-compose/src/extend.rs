//! Model-extension operations: the `petri_extend` / `sim_extend` op set,
//! ported from petri-pilot's `pkg/mcp/server.go` (`applyOperation`,
//! `compareModels`). Not a go-pflow package — petri-pilot is where these
//! operations are actually defined and exposed as an MCP tool — but it is
//! the "one home" for this op language, so it is ported here rather than
//! reinvented against `pflow-metamodel::Model` a second time.

use std::collections::HashSet;

use pflow_metamodel::{Arc as MArc, Binding, Event, EventField, Model, Place, Transition};
use serde_json::Value;

/// One operation from an `apply_operations` call.
#[derive(Debug, Clone)]
pub struct Operation {
    pub op: String,
    pub fields: serde_json::Map<String, Value>,
}

impl Operation {
    fn str(&self, key: &str) -> String {
        self.fields.get(key).and_then(Value::as_str).unwrap_or("").to_string()
    }
    fn i64(&self, key: &str) -> i64 {
        self.fields.get(key).and_then(Value::as_f64).unwrap_or(0.0) as i64
    }
    fn bool(&self, key: &str) -> bool {
        self.fields.get(key).and_then(Value::as_bool).unwrap_or(false)
    }
}

/// Parses the JSON array of operations `petri_extend`/`sim_extend` accept.
pub fn parse_operations(json: &str) -> Result<Vec<Operation>, String> {
    let raw: Vec<serde_json::Map<String, Value>> =
        serde_json::from_str(json).map_err(|e| format!("invalid operations JSON: {e}"))?;
    raw.into_iter()
        .map(|m| {
            let op = m.get("op").and_then(Value::as_str).unwrap_or("").to_string();
            if op.is_empty() {
                return Err("operation: missing 'op' field".to_string());
            }
            Ok(Operation { op, fields: m })
        })
        .collect()
}

/// Applies one operation to `model` in place. Ported field-for-field from
/// `applyOperation`'s `switch opType`.
pub fn apply_operation(model: &mut Model, op: &Operation) -> Result<(), String> {
    match op.op.as_str() {
        "add_place" => {
            let id = op.str("id");
            if id.is_empty() {
                return Err("missing 'id' for add_place".to_string());
            }
            model.places.push(Place {
                id,
                description: op.str("description"),
                initial: op.i64("initial"),
                ..Default::default()
            });
        }
        "add_transition" => {
            let id = op.str("id");
            if id.is_empty() {
                return Err("missing 'id' for add_transition".to_string());
            }
            let bindings = op
                .fields
                .get("bindings")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(parse_binding).filter(|b| !b.name.is_empty()).collect())
                .unwrap_or_default();
            model.transitions.push(Transition {
                id,
                description: op.str("description"),
                event: op.str("event"),
                guard: op.str("guard"),
                bindings,
                ..Default::default()
            });
        }
        "add_arc" => {
            let from = op.str("from");
            let to = op.str("to");
            if from.is_empty() || to.is_empty() {
                return Err("missing 'from' or 'to' for add_arc".to_string());
            }
            model.arcs.push(MArc {
                from,
                to,
                ..Default::default()
            });
        }
        "add_role" => return Err("add_role is deprecated: roles are now managed via extensions".to_string()),
        "add_access" => return Err("add_access is deprecated: access rules are now managed via extensions".to_string()),
        "remove_place" => {
            let id = op.str("id");
            if id.is_empty() {
                return Err("missing 'id' for remove_place".to_string());
            }
            model.places.retain(|p| p.id != id);
        }
        "remove_transition" => {
            let id = op.str("id");
            if id.is_empty() {
                return Err("missing 'id' for remove_transition".to_string());
            }
            model.transitions.retain(|t| t.id != id);
        }
        "remove_arc" => {
            let from = op.str("from");
            let to = op.str("to");
            if from.is_empty() || to.is_empty() {
                return Err("missing 'from' or 'to' for remove_arc".to_string());
            }
            model.arcs.retain(|a| a.from != from || a.to != to);
        }
        "remove_role" => return Err("remove_role is deprecated: roles are now managed via extensions".to_string()),
        "remove_access" => return Err("remove_access is deprecated: access rules are now managed via extensions".to_string()),
        "add_event" => {
            let id = op.str("id");
            if id.is_empty() {
                return Err("missing 'id' for add_event".to_string());
            }
            let fields = op
                .fields
                .get("fields")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|f| f.as_object())
                        .filter_map(|f| {
                            let name = f.get("name").and_then(Value::as_str).unwrap_or_default();
                            let typ = f.get("type").and_then(Value::as_str).unwrap_or_default();
                            if name.is_empty() || typ.is_empty() {
                                return None;
                            }
                            Some(EventField {
                                name: name.to_string(),
                                typ: typ.to_string(),
                                of: f.get("of").and_then(Value::as_str).unwrap_or_default().to_string(),
                                required: f.get("required").and_then(Value::as_bool).unwrap_or(false),
                                description: f.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            model.events.push(Event {
                id,
                name: op.str("name"),
                description: op.str("description"),
                fields,
            });
        }
        "add_event_field" => {
            let event_id = op.str("event");
            if event_id.is_empty() {
                return Err("missing 'event' for add_event_field".to_string());
            }
            let field_name = op.str("name");
            if field_name.is_empty() {
                return Err("missing 'name' for add_event_field".to_string());
            }
            let field_type = op.str("type");
            if field_type.is_empty() {
                return Err("missing 'type' for add_event_field".to_string());
            }
            let field = EventField {
                name: field_name,
                typ: field_type,
                of: op.str("of"),
                required: op.bool("required"),
                description: op.str("description"),
            };
            let ev = model
                .events
                .iter_mut()
                .find(|e| e.id == event_id)
                .ok_or_else(|| format!("event '{event_id}' not found for add_event_field"))?;
            ev.fields.push(field);
        }
        "remove_event" => {
            let id = op.str("id");
            if id.is_empty() {
                return Err("missing 'id' for remove_event".to_string());
            }
            model.events.retain(|e| e.id != id);
        }
        "add_binding" => {
            let transition_id = op.str("transition");
            if transition_id.is_empty() {
                return Err("missing 'transition' for add_binding".to_string());
            }
            let name = op.str("name");
            if name.is_empty() {
                return Err("missing 'name' for add_binding".to_string());
            }
            let typ = op.str("type");
            if typ.is_empty() {
                return Err("missing 'type' for add_binding".to_string());
            }
            let binding = Binding {
                name,
                typ,
                place: op.str("place"),
                value: op.bool("value"),
                keys: op
                    .fields
                    .get("keys")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default(),
            };
            let t = model
                .transitions
                .iter_mut()
                .find(|t| t.id == transition_id)
                .ok_or_else(|| format!("transition '{transition_id}' not found for add_binding"))?;
            t.bindings.push(binding);
        }
        "remove_binding" => {
            let transition_id = op.str("transition");
            if transition_id.is_empty() {
                return Err("missing 'transition' for remove_binding".to_string());
            }
            let binding_name = op.str("name");
            if binding_name.is_empty() {
                return Err("missing 'name' for remove_binding".to_string());
            }
            let t = model
                .transitions
                .iter_mut()
                .find(|t| t.id == transition_id)
                .ok_or_else(|| format!("transition '{transition_id}' not found for remove_binding"))?;
            t.bindings.retain(|b| b.name != binding_name);
        }
        other => return Err(format!("unknown operation: {other}")),
    }
    Ok(())
}

fn parse_binding(v: &Value) -> Option<Binding> {
    let m = v.as_object()?;
    Some(Binding {
        name: m.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        typ: m.get("type").and_then(Value::as_str).unwrap_or_default().to_string(),
        place: m.get("place").and_then(Value::as_str).unwrap_or_default().to_string(),
        value: m.get("value").and_then(Value::as_bool).unwrap_or(false),
        keys: m
            .get("keys")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|k| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    })
}

/// Applies every operation in order, collecting which succeeded and which
/// failed rather than stopping at the first error — matching
/// `handleExtend`'s "apply what you can, report the rest" behaviour.
pub fn apply_operations(model: &mut Model, ops: &[Operation]) -> (Vec<String>, Vec<String>) {
    let mut applied = Vec::new();
    let mut errors = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        match apply_operation(model, op) {
            Ok(()) => applied.push(op.op.clone()),
            Err(e) => errors.push(format!("operation {i} ({}): {e}", op.op)),
        }
    }
    (applied, errors)
}

/// The added/removed sets between two models, ported from `compareModels`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelDiff {
    pub places_added: Vec<String>,
    pub places_removed: Vec<String>,
    pub transitions_added: Vec<String>,
    pub transitions_removed: Vec<String>,
    pub arcs_added: Vec<String>,
    pub arcs_removed: Vec<String>,
    pub has_changes: bool,
}

fn sorted_diff(a: &HashSet<String>, b: &HashSet<String>) -> (Vec<String>, Vec<String>) {
    let mut added: Vec<String> = b.difference(a).cloned().collect();
    let mut removed: Vec<String> = a.difference(b).cloned().collect();
    added.sort();
    removed.sort();
    (added, removed)
}

/// Compares two models' places, transitions and arcs by ID/endpoint pair.
pub fn compare_models(a: &Model, b: &Model) -> ModelDiff {
    let places_a: HashSet<String> = a.places.iter().map(|p| p.id.clone()).collect();
    let places_b: HashSet<String> = b.places.iter().map(|p| p.id.clone()).collect();
    let (places_added, places_removed) = sorted_diff(&places_a, &places_b);

    let trans_a: HashSet<String> = a.transitions.iter().map(|t| t.id.clone()).collect();
    let trans_b: HashSet<String> = b.transitions.iter().map(|t| t.id.clone()).collect();
    let (transitions_added, transitions_removed) = sorted_diff(&trans_a, &trans_b);

    let arc_key = |arc: &MArc| format!("{}->{}", arc.from, arc.to);
    let arcs_a: HashSet<String> = a.arcs.iter().map(arc_key).collect();
    let arcs_b: HashSet<String> = b.arcs.iter().map(arc_key).collect();
    let (arcs_added, arcs_removed) = sorted_diff(&arcs_a, &arcs_b);

    let has_changes = !places_added.is_empty()
        || !places_removed.is_empty()
        || !transitions_added.is_empty()
        || !transitions_removed.is_empty()
        || !arcs_added.is_empty()
        || !arcs_removed.is_empty();

    ModelDiff {
        places_added,
        places_removed,
        transitions_added,
        transitions_removed,
        arcs_added,
        arcs_removed,
        has_changes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_model() -> Model {
        Model {
            name: "m".to_string(),
            places: vec![Place {
                id: "p".to_string(),
                initial: 1,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn add_place_appends() {
        let mut m = base_model();
        let ops = parse_operations(r#"[{"op":"add_place","id":"q","initial":2}]"#).unwrap();
        let (applied, errors) = apply_operations(&mut m, &ops);
        assert_eq!(applied, vec!["add_place"]);
        assert!(errors.is_empty());
        assert_eq!(m.places.len(), 2);
        assert_eq!(m.place_by_id("q").unwrap().initial, 2);
    }

    #[test]
    fn remove_place_and_unknown_op_reports_error() {
        let mut m = base_model();
        let ops = parse_operations(r#"[{"op":"remove_place","id":"p"},{"op":"nonsense"}]"#).unwrap();
        let (applied, errors) = apply_operations(&mut m, &ops);
        assert_eq!(applied, vec!["remove_place"]);
        assert_eq!(errors.len(), 1);
        assert!(m.places.is_empty());
    }

    #[test]
    fn add_arc_requires_from_and_to() {
        let mut m = base_model();
        let op = Operation {
            op: "add_arc".to_string(),
            fields: serde_json::from_str(r#"{"from":"p"}"#).unwrap(),
        };
        assert!(apply_operation(&mut m, &op).is_err());
    }

    #[test]
    fn compare_models_reports_additions_and_removals() {
        let a = base_model();
        let mut b = base_model();
        b.places.push(Place {
            id: "q".to_string(),
            ..Default::default()
        });
        b.places.retain(|p| p.id != "p");
        let diff = compare_models(&a, &b);
        assert_eq!(diff.places_added, vec!["q".to_string()]);
        assert_eq!(diff.places_removed, vec!["p".to_string()]);
        assert!(diff.has_changes);
    }

    #[test]
    fn identical_models_have_no_changes() {
        let a = base_model();
        let b = base_model();
        assert!(!compare_models(&a, &b).has_changes);
    }
}
