//! Fluent builder API for token model schemas via DSL.

use pflow_tokenmodel::Schema;

use crate::ast::*;
use crate::interpret;
use crate::sexpr;

/// Fluent builder for constructing token model schemas.
pub struct Builder {
    node: SchemaNode,
    current_state: Option<usize>,
    current_action: Option<usize>,
    current_arc: Option<usize>,
}

impl Builder {
    /// Creates a new schema builder with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            node: SchemaNode {
                name: name.into(),
                version: "v1.0.0".into(),
                states: Vec::new(),
                actions: Vec::new(),
                arcs: Vec::new(),
                constraints: Vec::new(),
            },
            current_state: None,
            current_action: None,
            current_arc: None,
        }
    }

    /// Sets the schema version.
    pub fn version(mut self, v: &str) -> Self {
        self.node.version = v.into();
        self
    }

    /// Adds a data state.
    pub fn data(mut self, id: &str, typ: &str) -> Self {
        self.clear_current();
        let idx = self.node.states.len();
        self.node.states.push(StateNode {
            id: id.into(),
            typ: typ.into(),
            kind: "data".into(),
            initial: None,
            exported: false,
        });
        self.current_state = Some(idx);
        self
    }

    /// Adds a token-counting state.
    pub fn token(mut self, id: &str, initial: Option<i64>) -> Self {
        self.clear_current();
        let idx = self.node.states.len();
        self.node.states.push(StateNode {
            id: id.into(),
            typ: "int".into(),
            kind: "token".into(),
            initial: initial.map(InitialValue::Int),
            exported: false,
        });
        self.current_state = Some(idx);
        self
    }

    /// Marks the current state as exported.
    pub fn exported(mut self) -> Self {
        if let Some(idx) = self.current_state {
            self.node.states[idx].exported = true;
        }
        self
    }

    /// Sets the initial value for the current state.
    pub fn initial(mut self, value: i64) -> Self {
        if let Some(idx) = self.current_state {
            self.node.states[idx].initial = Some(InitialValue::Int(value));
        }
        self
    }

    /// Adds an action.
    pub fn action(mut self, id: &str) -> Self {
        self.clear_current();
        let idx = self.node.actions.len();
        self.node.actions.push(ActionNode {
            id: id.into(),
            guard: String::new(),
        });
        self.current_action = Some(idx);
        self
    }

    /// Sets the guard expression for the current action.
    pub fn guard(mut self, expr: &str) -> Self {
        if let Some(idx) = self.current_action {
            self.node.actions[idx].guard = expr.into();
        }
        self
    }

    /// Adds an arc from source to target.
    pub fn flow(mut self, source: &str, target: &str) -> Self {
        self.clear_current();
        let idx = self.node.arcs.len();
        self.node.arcs.push(ArcNode {
            source: source.into(),
            target: target.into(),
            keys: Vec::new(),
            value: String::new(),
        });
        self.current_arc = Some(idx);
        self
    }

    /// Alias for flow.
    pub fn arc(self, source: &str, target: &str) -> Self {
        self.flow(source, target)
    }

    /// Sets the map access keys for the current arc.
    pub fn keys(mut self, keys: &[&str]) -> Self {
        if let Some(idx) = self.current_arc {
            self.node.arcs[idx].keys = keys.iter().map(|s| s.to_string()).collect();
        }
        self
    }

    /// Sets the value binding name for the current arc.
    pub fn value(mut self, v: &str) -> Self {
        if let Some(idx) = self.current_arc {
            self.node.arcs[idx].value = v.into();
        }
        self
    }

    /// Adds a constraint.
    pub fn constraint(mut self, id: &str, expr: &str) -> Self {
        self.clear_current();
        self.node.constraints.push(ConstraintNode {
            id: id.into(),
            expr: expr.into(),
        });
        self
    }

    fn clear_current(&mut self) {
        self.current_state = None;
        self.current_action = None;
        self.current_arc = None;
    }

    /// Returns the underlying AST node.
    pub fn ast(&self) -> &SchemaNode {
        &self.node
    }

    /// Builds and returns the tokenmodel Schema.
    pub fn schema(self) -> Result<Schema, String> {
        interpret::interpret(&self.node)
    }

    /// Builds and returns the tokenmodel Schema. Panics on error.
    pub fn must_schema(self) -> Schema {
        self.schema().expect("schema validation failed")
    }

    /// Generates the S-expression DSL representation.
    pub fn to_string(&self) -> String {
        sexpr::to_sexpr(&self.node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_basic() {
        let schema = Builder::new("ERC-020")
            .data("balances", "map[address]uint256")
            .exported()
            .data("totalSupply", "uint256")
            .action("transfer")
            .guard("balances[from] >= amount")
            .flow("balances", "transfer")
            .keys(&["from"])
            .flow("transfer", "balances")
            .keys(&["to"])
            .constraint("conservation", "sum(balances) == totalSupply")
            .must_schema();

        assert_eq!(schema.name, "ERC-020");
        assert_eq!(schema.states.len(), 2);
        assert_eq!(schema.actions.len(), 1);
        assert_eq!(schema.arcs.len(), 2);
        assert_eq!(schema.constraints.len(), 1);
        assert!(schema.states[0].exported);
    }

    #[test]
    fn test_builder_token() {
        let schema = Builder::new("counter")
            .token("count", Some(5))
            .action("inc")
            .flow("inc", "count")
            .must_schema();

        assert_eq!(schema.states[0].initial_tokens(), 5);
        assert!(schema.states[0].is_token());
    }

    #[test]
    fn test_builder_to_string() {
        let b = Builder::new("test")
            .data("balances", "map[address]uint256")
            .action("transfer")
            .flow("balances", "transfer")
            .keys(&["from"]);

        let sexpr = b.to_string();
        assert!(sexpr.contains("schema test"));
        assert!(sexpr.contains("balances"));
        assert!(sexpr.contains("transfer"));
    }
}
