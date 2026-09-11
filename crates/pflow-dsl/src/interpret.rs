//! AST to Schema conversion.

use pflow_tokenmodel::schema::{self, Schema};

use crate::ast::*;

/// Converts a parsed SchemaNode into a tokenmodel Schema.
pub fn interpret(node: &SchemaNode) -> Result<Schema, String> {
    let mut schema = Schema::new(&node.name);
    schema.version = node.version.clone();

    for s in &node.states {
        let kind = if s.kind == "token" {
            schema::Kind::Token
        } else {
            schema::Kind::Data
        };

        let initial = match &s.initial {
            Some(InitialValue::Int(n)) => Some(serde_json::Value::Number((*n).into())),
            Some(InitialValue::Str(s)) => Some(serde_json::Value::String(s.clone())),
            Some(InitialValue::Nil) | None => None,
        };

        schema.add_state(schema::State {
            id: s.id.clone(),
            kind,
            typ: s.typ.clone(),
            initial,
            exported: s.exported,
        });
    }

    for a in &node.actions {
        schema.add_action(schema::Action {
            id: a.id.clone(),
            guard: a.guard.clone(),
            event_id: String::new(),
            event_bindings: None,
        });
    }

    for a in &node.arcs {
        schema.add_arc(schema::Arc {
            source: a.source.clone(),
            target: a.target.clone(),
            keys: a.keys.clone(),
            value: a.value.clone(),
            ..Default::default()
        });
    }

    for c in &node.constraints {
        schema.add_constraint(schema::Constraint {
            id: c.id.clone(),
            expr: c.expr.clone(),
        });
    }

    schema.validate().map_err(|e| e.to_string())?;

    Ok(schema)
}

/// Parse DSL input and return a Schema.
pub fn parse_schema(input: &str) -> Result<Schema, String> {
    let node = crate::parser::parse(input)?;
    interpret(&node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpret_basic() {
        let input = r#"(schema test
  (version v1.0.0)
  (states
    (state count :kind token :initial 5)
  )
  (actions
    (action inc)
  )
  (arcs
    (arc inc -> count)
  )
)"#;

        let schema = parse_schema(input).unwrap();
        assert_eq!(schema.name, "test");
        assert_eq!(schema.states.len(), 1);
        assert!(schema.states[0].is_token());
        assert_eq!(schema.states[0].initial_tokens(), 5);
    }
}
