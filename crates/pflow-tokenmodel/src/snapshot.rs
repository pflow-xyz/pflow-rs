//! Snapshot: current state of all states in a schema.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::schema::Schema;

/// The current state of all states in a schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tokens: HashMap<String, i64>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub data: HashMap<String, Value>,
}

impl Snapshot {
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
            data: HashMap::new(),
        }
    }

    /// Creates a snapshot initialized from schema defaults.
    pub fn from_schema(s: &Schema) -> Self {
        let mut snap = Self::new();
        for st in &s.states {
            if st.is_token() {
                snap.tokens.insert(st.id.clone(), st.initial_tokens());
            } else if let Some(initial) = &st.initial {
                snap.data.insert(st.id.clone(), initial.clone());
            } else {
                snap.data
                    .insert(st.id.clone(), Value::Object(Default::default()));
            }
        }
        snap
    }

    pub fn get_tokens(&self, state_id: &str) -> i64 {
        self.tokens.get(state_id).copied().unwrap_or(0)
    }

    pub fn set_tokens(&mut self, state_id: &str, count: i64) {
        self.tokens.insert(state_id.to_string(), count);
    }

    pub fn add_tokens(&mut self, state_id: &str, delta: i64) {
        let entry = self.tokens.entry(state_id.to_string()).or_insert(0);
        *entry += delta;
    }

    pub fn get_data(&self, state_id: &str) -> Option<&Value> {
        self.data.get(state_id)
    }

    pub fn set_data(&mut self, state_id: &str, value: Value) {
        self.data.insert(state_id.to_string(), value);
    }

    /// Returns data as a mutable JSON object, or None.
    pub fn get_data_map(&self, state_id: &str) -> Option<&serde_json::Map<String, Value>> {
        self.data.get(state_id).and_then(|v| v.as_object())
    }

    /// Gets a value from a data state map.
    pub fn get_data_map_value(&self, state_id: &str, key: &str) -> Option<&Value> {
        self.get_data_map(state_id).and_then(|m| m.get(key))
    }

    /// Sets a value in a data state map.
    pub fn set_data_map_value(&mut self, state_id: &str, key: &str, value: Value) {
        let entry = self
            .data
            .entry(state_id.to_string())
            .or_insert_with(|| Value::Object(Default::default()));
        if let Value::Object(map) = entry {
            map.insert(key.to_string(), value);
        }
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Variable bindings for parameterized action execution.
pub type Bindings = HashMap<String, Value>;

/// Extension trait for Bindings convenience methods.
pub trait BindingsExt {
    fn get_string(&self, key: &str) -> String;
    fn get_i64(&self, key: &str) -> i64;
}

impl BindingsExt for Bindings {
    fn get_string(&self, key: &str) -> String {
        self.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    fn get_i64(&self, key: &str) -> i64 {
        self.get(key)
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_f64().map(|f| f as i64))
                    .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
            })
            .unwrap_or(0)
    }
}
