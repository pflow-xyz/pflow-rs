use serde_json::Value;
use std::str::FromStr;
use std::fmt;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Action {
    action: String,
    scalar: i64,
    #[serde(default)]
    seq: Option<i64>,
}

impl Default for Action {
    fn default() -> Self {
        Action {
            action: "noop".to_string(),
            scalar: 1,
            seq: Some(0),
        }
    }
}

impl FromStr for Action {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let action = serde_json::from_str(s);
        match action {
            Ok(a) => Ok(a),
            Err(e) => Err(e.to_string()),
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap())
    }
}

pub fn run_command(action: Action) -> Value {
    let result = match action.action.as_str() {
        _ => -1,
    };

    serde_json::json!({"result": result})
}