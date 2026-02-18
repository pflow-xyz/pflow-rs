//! Code generation from AST to Rust source code.

use crate::ast::*;

/// Generates Rust source code that constructs the schema.
pub fn generate_rust(node: &SchemaNode, module_name: &str, fn_name: &str) -> Result<String, String> {
    let mut b = String::new();

    b.push_str(&format!("//! Generated schema for {}.\n\n", node.name));
    b.push_str("use pflow_tokenmodel::schema::*;\n\n");

    b.push_str(&format!(
        "/// {} creates a schema from DSL definition.\n",
        fn_name
    ));
    b.push_str(&format!("pub fn {}() -> Schema {{\n", fn_name));

    b.push_str(&format!(
        "    let mut schema = Schema::new({:?});\n",
        node.name
    ));
    if !node.version.is_empty() {
        b.push_str(&format!(
            "    schema.version = {:?}.into();\n",
            node.version
        ));
    }
    b.push('\n');

    // States
    if !node.states.is_empty() {
        b.push_str("    // States\n");
        for s in &node.states {
            b.push_str("    schema.add_state(State {\n");
            b.push_str(&format!("        id: {:?}.into(),\n", s.id));
            if s.kind == "token" {
                b.push_str("        kind: Kind::Token,\n");
            } else {
                b.push_str("        kind: Kind::Data,\n");
            }
            if !s.typ.is_empty() {
                b.push_str(&format!("        typ: {:?}.into(),\n", s.typ));
            } else {
                b.push_str("        typ: String::new(),\n");
            }
            match &s.initial {
                Some(InitialValue::Int(n)) => {
                    b.push_str(&format!(
                        "        initial: Some(serde_json::Value::Number({}.into())),\n",
                        n
                    ));
                }
                Some(InitialValue::Str(v)) => {
                    b.push_str(&format!(
                        "        initial: Some(serde_json::Value::String({:?}.into())),\n",
                        v
                    ));
                }
                _ => {
                    b.push_str("        initial: None,\n");
                }
            }
            b.push_str(&format!("        exported: {},\n", s.exported));
            b.push_str("    });\n");
        }
        b.push('\n');
    }

    // Actions
    if !node.actions.is_empty() {
        b.push_str("    // Actions\n");
        for a in &node.actions {
            b.push_str("    schema.add_action(Action {\n");
            b.push_str(&format!("        id: {:?}.into(),\n", a.id));
            if !a.guard.is_empty() {
                b.push_str(&format!("        guard: {:?}.into(),\n", a.guard));
            } else {
                b.push_str("        guard: String::new(),\n");
            }
            b.push_str("        event_id: String::new(),\n");
            b.push_str("        event_bindings: None,\n");
            b.push_str("    });\n");
        }
        b.push('\n');
    }

    // Arcs
    if !node.arcs.is_empty() {
        b.push_str("    // Arcs\n");
        for a in &node.arcs {
            b.push_str("    schema.add_arc(Arc {\n");
            b.push_str(&format!("        source: {:?}.into(),\n", a.source));
            b.push_str(&format!("        target: {:?}.into(),\n", a.target));
            if !a.keys.is_empty() {
                let keys: Vec<String> = a.keys.iter().map(|k| format!("{:?}.into()", k)).collect();
                b.push_str(&format!("        keys: vec![{}],\n", keys.join(", ")));
            } else {
                b.push_str("        keys: vec![],\n");
            }
            if !a.value.is_empty() {
                b.push_str(&format!("        value: {:?}.into(),\n", a.value));
            } else {
                b.push_str("        value: String::new(),\n");
            }
            b.push_str("    });\n");
        }
        b.push('\n');
    }

    // Constraints
    if !node.constraints.is_empty() {
        b.push_str("    // Constraints\n");
        for c in &node.constraints {
            b.push_str("    schema.add_constraint(Constraint {\n");
            b.push_str(&format!("        id: {:?}.into(),\n", c.id));
            b.push_str(&format!("        expr: {:?}.into(),\n", c.expr));
            b.push_str("    });\n");
        }
        b.push('\n');
    }

    b.push_str("    schema\n");
    b.push_str("}\n");

    let _ = module_name; // reserved for future use
    Ok(b)
}

/// Parse DSL input and generate Rust code.
pub fn generate_rust_from_dsl(
    input: &str,
    module_name: &str,
    fn_name: &str,
) -> Result<String, String> {
    let node = crate::parser::parse(input)?;
    generate_rust(&node, module_name, fn_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codegen_basic() {
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

        let code = generate_rust_from_dsl(input, "test", "make_schema").unwrap();
        assert!(code.contains("pub fn make_schema()"));
        assert!(code.contains("Schema::new"));
        assert!(code.contains("Kind::Token"));
        assert!(code.contains("\"count\""));
    }
}
