//! Token model schema types.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Discriminates between token-counting and data-holding states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Token,
    Data,
}

impl Default for Kind {
    fn default() -> Self {
        Kind::Data
    }
}

/// A named container in a schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub id: String,

    #[serde(default, skip_serializing_if = "is_default_kind")]
    pub kind: Kind,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial: Option<serde_json::Value>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[serde(rename = "type")]
    pub typ: String,

    #[serde(default, skip_serializing_if = "is_false")]
    pub exported: bool,
}

fn is_default_kind(k: &Kind) -> bool {
    *k == Kind::Data
}

fn is_false(b: &bool) -> bool {
    !b
}

impl State {
    pub fn is_token(&self) -> bool {
        self.kind == Kind::Token
    }

    pub fn is_data(&self) -> bool {
        self.kind == Kind::Data
    }

    /// Returns the initial token count (for TokenState).
    pub fn initial_tokens(&self) -> i64 {
        if !self.is_token() {
            return 0;
        }
        match &self.initial {
            Some(v) => v.as_i64().unwrap_or(0),
            None => 0,
        }
    }
}

/// A state-changing operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guard: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event_id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_bindings: Option<HashMap<String, String>>,
}

/// An arc connecting states and actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arc {
    pub source: String,
    pub target: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
}

/// A constraint that must hold across all snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    pub id: String,
    pub expr: String,
}

/// An Ethereum event that can trigger state changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub signature: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub topic: String,

    #[serde(default)]
    pub parameters: Vec<EventParameter>,
}

/// A single parameter in an event signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventParameter {
    pub name: String,

    #[serde(rename = "type")]
    pub typ: String,

    #[serde(default, skip_serializing_if = "is_false")]
    pub indexed: bool,
}

/// A complete tokenmodel schema definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,

    pub states: Vec<State>,
    pub actions: Vec<Action>,
    pub arcs: Vec<Arc>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<Constraint>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

impl Schema {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: "1.0.0".into(),
            states: Vec::new(),
            actions: Vec::new(),
            arcs: Vec::new(),
            constraints: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn add_state(&mut self, st: State) -> &mut Self {
        self.states.push(st);
        self
    }

    pub fn add_token_state(&mut self, id: impl Into<String>, initial: i64) -> &mut Self {
        self.states.push(State {
            id: id.into(),
            kind: Kind::Token,
            initial: Some(serde_json::Value::Number(initial.into())),
            typ: "int".into(),
            exported: false,
        });
        self
    }

    pub fn add_data_state(
        &mut self,
        id: impl Into<String>,
        typ: impl Into<String>,
        initial: Option<serde_json::Value>,
        exported: bool,
    ) -> &mut Self {
        self.states.push(State {
            id: id.into(),
            kind: Kind::Data,
            typ: typ.into(),
            initial,
            exported,
        });
        self
    }

    pub fn add_action(&mut self, a: Action) -> &mut Self {
        self.actions.push(a);
        self
    }

    pub fn add_arc(&mut self, a: Arc) -> &mut Self {
        self.arcs.push(a);
        self
    }

    pub fn add_constraint(&mut self, c: Constraint) -> &mut Self {
        self.constraints.push(c);
        self
    }

    pub fn add_event(&mut self, e: Event) -> &mut Self {
        self.events.push(e);
        self
    }

    pub fn state_by_id(&self, id: &str) -> Option<&State> {
        self.states.iter().find(|s| s.id == id)
    }

    pub fn action_by_id(&self, id: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.id == id)
    }

    pub fn event_by_id(&self, id: &str) -> Option<&Event> {
        self.events.iter().find(|e| e.id == id)
    }

    pub fn action_for_event(&self, event_id: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.event_id == event_id)
    }

    /// Returns all arcs flowing into an action.
    pub fn input_arcs(&self, action_id: &str) -> Vec<&Arc> {
        self.arcs.iter().filter(|a| a.target == action_id).collect()
    }

    /// Returns all arcs flowing out of an action.
    pub fn output_arcs(&self, action_id: &str) -> Vec<&Arc> {
        self.arcs.iter().filter(|a| a.source == action_id).collect()
    }

    pub fn token_states(&self) -> Vec<&State> {
        self.states.iter().filter(|s| s.is_token()).collect()
    }

    pub fn data_states(&self) -> Vec<&State> {
        self.states.iter().filter(|s| s.is_data()).collect()
    }
}
