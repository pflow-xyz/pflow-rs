use super::parse_model;
use pflow_tokenmodel::Runtime;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
struct FireResult {
    steps: Vec<StepResult>,
    final_tokens: HashMap<String, i64>,
    enabled_actions: Vec<String>,
    sequence: u64,
}

#[derive(Serialize)]
struct StepResult {
    action: String,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    tokens_after: HashMap<String, i64>,
}

pub fn run(model: &str, actions: &[String]) -> Result<String, String> {
    let schema = parse_model(model)?;
    let mut runtime = Runtime::new(schema.clone());
    let mut steps = Vec::new();

    for action_id in actions {
        match runtime.execute(action_id) {
            Ok(()) => {
                let tokens = token_snapshot(&runtime, &schema);
                steps.push(StepResult {
                    action: action_id.clone(),
                    success: true,
                    error: None,
                    tokens_after: tokens,
                });
            }
            Err(e) => {
                let tokens = token_snapshot(&runtime, &schema);
                steps.push(StepResult {
                    action: action_id.clone(),
                    success: false,
                    error: Some(format!("{e}")),
                    tokens_after: tokens,
                });
            }
        }
    }

    let final_tokens = token_snapshot(&runtime, &schema);
    let enabled = runtime.enabled_actions();

    let result = FireResult {
        steps,
        final_tokens,
        enabled_actions: enabled,
        sequence: runtime.sequence,
    };

    serde_json::to_string_pretty(&result).map_err(|e| format!("Serialization error: {e}"))
}

fn token_snapshot(
    runtime: &Runtime,
    schema: &pflow_tokenmodel::Schema,
) -> HashMap<String, i64> {
    schema
        .token_states()
        .iter()
        .map(|s| (s.id.clone(), runtime.tokens(&s.id)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fire_single() {
        let dsl = r#"(schema test
            (states
                (state p1 :kind token :initial 3)
                (state p2 :kind token :initial 0)
            )
            (actions (action t1))
            (arcs (arc p1 -> t1) (arc t1 -> p2))
        )"#;
        let result = run(dsl, &["t1".to_string()]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["steps"][0]["success"], true);
        assert_eq!(v["final_tokens"]["p1"], 2);
        assert_eq!(v["final_tokens"]["p2"], 1);
    }

    #[test]
    fn test_fire_not_enabled() {
        let dsl = r#"(schema test
            (states (state p1 :kind token :initial 0))
            (actions (action t1))
            (arcs (arc p1 -> t1))
        )"#;
        let result = run(dsl, &["t1".to_string()]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["steps"][0]["success"], false);
    }

    #[test]
    fn test_fire_sequence() {
        let dsl = r#"(schema test
            (states
                (state p1 :kind token :initial 2)
                (state p2 :kind token :initial 0)
            )
            (actions (action t1))
            (arcs (arc p1 -> t1) (arc t1 -> p2))
        )"#;
        let result = run(dsl, &["t1".to_string(), "t1".to_string()]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["final_tokens"]["p1"], 0);
        assert_eq!(v["final_tokens"]["p2"], 2);
        assert_eq!(v["sequence"], 2);
    }
}
