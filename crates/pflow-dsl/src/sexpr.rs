//! Schema to S-expression string conversion.

use crate::ast::*;

/// Converts a SchemaNode to an S-expression DSL string.
pub fn to_sexpr(node: &SchemaNode) -> String {
    let mut s = String::new();
    s.push_str(&format!("(schema {}\n", node.name));
    s.push_str(&format!("  (version {})\n", node.version));

    if !node.states.is_empty() {
        s.push_str("\n  (states\n");
        for st in &node.states {
            s.push_str(&format!("    (state {}", st.id));
            if !st.typ.is_empty() {
                s.push_str(&format!(" :type {}", st.typ));
            }
            if st.kind == "token" {
                s.push_str(" :kind token");
            }
            if let Some(initial) = &st.initial {
                match initial {
                    InitialValue::Int(n) => s.push_str(&format!(" :initial {}", n)),
                    InitialValue::Str(v) => s.push_str(&format!(" :initial \"{}\"", v)),
                    InitialValue::Nil => {}
                }
            }
            if st.exported {
                s.push_str(" :exported");
            }
            s.push_str(")\n");
        }
        s.push_str("  )\n");
    }

    if !node.actions.is_empty() {
        s.push_str("\n  (actions\n");
        for a in &node.actions {
            s.push_str(&format!("    (action {}", a.id));
            if !a.guard.is_empty() {
                s.push_str(&format!(" :guard {{{}}}", a.guard));
            }
            s.push_str(")\n");
        }
        s.push_str("  )\n");
    }

    if !node.arcs.is_empty() {
        s.push_str("\n  (arcs\n");
        for a in &node.arcs {
            s.push_str(&format!("    (arc {} -> {}", a.source, a.target));
            if !a.keys.is_empty() {
                s.push_str(&format!(" :keys ({})", a.keys.join(" ")));
            }
            if !a.value.is_empty() {
                s.push_str(&format!(" :value {}", a.value));
            }
            s.push_str(")\n");
        }
        s.push_str("  )\n");
    }

    if !node.constraints.is_empty() {
        s.push_str("\n  (constraints\n");
        for c in &node.constraints {
            s.push_str(&format!("    (constraint {} {{{}}})\n", c.id, c.expr));
        }
        s.push_str("  )\n");
    }

    s.push(')');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn test_roundtrip() {
        let input = r#"(schema ERC-020
  (version v1.0.0)

  (states
    (state balances :type map[address]uint256 :exported)
  )

  (actions
    (action transfer :guard {balances[from] >= amount})
  )

  (arcs
    (arc balances -> transfer :keys (from))
    (arc transfer -> balances :keys (to))
  )

  (constraints
    (constraint conservation {sum(balances) == totalSupply})
  )
)"#;

        let node = parser::parse(input).unwrap();
        let output = to_sexpr(&node);

        // Parse the output again to verify roundtrip
        let node2 = parser::parse(&output).unwrap();
        assert_eq!(node.name, node2.name);
        assert_eq!(node.states.len(), node2.states.len());
        assert_eq!(node.actions.len(), node2.actions.len());
        assert_eq!(node.arcs.len(), node2.arcs.len());
        assert_eq!(node.constraints.len(), node2.constraints.len());
    }
}
