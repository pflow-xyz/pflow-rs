//! AST types for the S-expression DSL.

/// A parsed schema definition.
#[derive(Debug, Clone)]
pub struct SchemaNode {
    pub name: String,
    pub version: String,
    pub states: Vec<StateNode>,
    pub actions: Vec<ActionNode>,
    pub arcs: Vec<ArcNode>,
    pub constraints: Vec<ConstraintNode>,
}

/// A parsed state definition.
#[derive(Debug, Clone)]
pub struct StateNode {
    pub id: String,
    pub typ: String,
    pub kind: String, // "token" or "data"
    pub initial: Option<InitialValue>,
    pub exported: bool,
}

/// Initial value for a state.
#[derive(Debug, Clone)]
pub enum InitialValue {
    Int(i64),
    Str(String),
    Nil,
}

/// A parsed action definition.
#[derive(Debug, Clone)]
pub struct ActionNode {
    pub id: String,
    pub guard: String,
}

/// A parsed arc definition.
#[derive(Debug, Clone)]
pub struct ArcNode {
    pub source: String,
    pub target: String,
    pub keys: Vec<String>,
    pub value: String,
}

/// A parsed constraint definition.
#[derive(Debug, Clone)]
pub struct ConstraintNode {
    pub id: String,
    pub expr: String,
}
