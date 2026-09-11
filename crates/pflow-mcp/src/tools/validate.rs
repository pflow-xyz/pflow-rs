use super::parse_model;
use serde::Serialize;

#[derive(Serialize)]
struct ValidateResult {
    valid: bool,
    name: String,
    version: String,
    cid: String,
    identity_hash: String,
    token_states: usize,
    data_states: usize,
    actions: usize,
    arcs: usize,
    constraints: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub fn run(model: &str) -> Result<String, String> {
    match parse_model(model) {
        Ok(schema) => {
            let result = ValidateResult {
                valid: true,
                name: schema.name.clone(),
                version: schema.version.clone(),
                cid: schema.cid(),
                identity_hash: schema.identity_hash(),
                token_states: schema.token_states().len(),
                data_states: schema.data_states().len(),
                actions: schema.actions.len(),
                arcs: schema.arcs.len(),
                constraints: schema.constraints.len(),
                error: None,
            };
            serde_json::to_string_pretty(&result).map_err(|e| format!("Serialization error: {e}"))
        }
        Err(e) => {
            let result = ValidateResult {
                valid: false,
                name: String::new(),
                version: String::new(),
                cid: String::new(),
                identity_hash: String::new(),
                token_states: 0,
                data_states: 0,
                actions: 0,
                arcs: 0,
                constraints: 0,
                error: Some(e),
            };
            serde_json::to_string_pretty(&result).map_err(|e| format!("Serialization error: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid() {
        let dsl = r#"(schema test
            (states (state p1 :kind token :initial 1))
            (actions (action t1))
            (arcs (arc p1 -> t1))
        )"#;
        let result = run(dsl).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["valid"], true);
        assert!(v["cid"].as_str().unwrap().starts_with("cid:"));
    }

    #[test]
    fn test_validate_invalid() {
        let result = run("not valid input {{{}}}").unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["valid"], false);
        assert!(!v["error"].as_str().unwrap().is_empty());
    }
}
