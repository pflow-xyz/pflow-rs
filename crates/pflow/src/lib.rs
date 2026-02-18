//! Petri net modeling with ODE simulation and token model DSL.
//!
//! This crate re-exports the public API from all pflow crates.

pub use pflow_core as core;
pub use pflow_dsl as dsl;
pub use pflow_solver as solver;
pub use pflow_tokenmodel as tokenmodel;

// Convenient re-exports
pub use pflow_core::{Builder, PetriNet, State};
pub use pflow_macros::schema;
pub use pflow_solver::{find_equilibrium, solve, Options, Problem, Solution};

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::stateutil;

    #[test]
    fn test_sir_parity() {
        // Build SIR model
        let (net, rates) = PetriNet::build().sir(999.0, 1.0, 0.0).with_rates(1.0);

        let state = net.set_state(None);
        let prob = Problem::new(net, state, [0.0, 100.0], rates);
        let (final_state, reached) = find_equilibrium(&prob);

        assert!(reached, "SIR should reach equilibrium");

        // Conservation: S + I + R = 1000
        let total = final_state["S"] + final_state["I"] + final_state["R"];
        assert!(
            (total - 1000.0).abs() < 1.0,
            "Conservation violated: total = {}",
            total
        );

        // At equilibrium: I near 0, R >> S
        assert!(final_state["I"] < 1.0, "I should be near 0");
        assert!(final_state["R"] > 900.0, "Most population should be recovered");
    }

    #[test]
    fn test_stateutil_integration() {
        let state = [("S", 999.0), ("I", 1.0), ("R", 0.0)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<State>();

        assert_eq!(stateutil::sum(&state), 1000.0);

        let updated = stateutil::apply(
            &state,
            &[("S", 500.0), ("R", 500.0)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        );
        assert_eq!(stateutil::sum(&updated), 1001.0); // S=500, I=1, R=500
    }

    #[test]
    fn test_dsl_parse_to_runtime() {
        let input = r#"(schema counter
  (version v1.0.0)
  (states
    (state ready :kind token :initial 3)
    (state done :kind token :initial 0)
  )
  (actions
    (action process)
  )
  (arcs
    (arc ready -> process)
    (arc process -> done)
  )
)"#;

        let schema = dsl::parse_schema(input).unwrap();
        assert_eq!(schema.name, "counter");
        assert_eq!(schema.states.len(), 2);

        let mut rt = tokenmodel::Runtime::new(schema);
        assert_eq!(rt.tokens("ready"), 3);
        assert_eq!(rt.tokens("done"), 0);

        // Execute 3 times
        for _ in 0..3 {
            rt.execute("process").unwrap();
        }

        assert_eq!(rt.tokens("ready"), 0);
        assert_eq!(rt.tokens("done"), 3);

        // Can't execute again — not enabled
        assert!(rt.execute("process").is_err());
    }

    #[test]
    fn test_dsl_builder_to_runtime() {
        let schema = dsl::Builder::new("ERC-020")
            .data("balances", "map[address]uint256")
            .exported()
            .action("transfer")
            .guard("balances[from] >= amount")
            .flow("balances", "transfer")
            .keys(&["from"])
            .flow("transfer", "balances")
            .keys(&["to"])
            .constraint("conservation", "sum(balances) == totalSupply")
            .must_schema();

        assert_eq!(schema.name, "ERC-020");
        assert_eq!(schema.states.len(), 1);
        assert_eq!(schema.actions.len(), 1);
        assert_eq!(schema.arcs.len(), 2);

        let mut rt = tokenmodel::Runtime::new(schema);

        // Set up initial balance
        rt.snapshot.set_data_map_value(
            "balances",
            "alice",
            serde_json::Value::Number(1000.into()),
        );

        // Execute a transfer
        let mut bindings = tokenmodel::Bindings::new();
        bindings.insert(
            "from".into(),
            serde_json::Value::String("alice".into()),
        );
        bindings.insert(
            "to".into(),
            serde_json::Value::String("bob".into()),
        );
        bindings.insert(
            "amount".into(),
            serde_json::Value::Number(250.into()),
        );

        rt.check_constraints = false; // No guard evaluator for this test
        rt.execute_with_bindings("transfer", &bindings).unwrap();

        // Verify balances
        let alice = rt
            .snapshot
            .get_data_map_value("balances", "alice")
            .and_then(|v| v.as_i64())
            .unwrap();
        let bob = rt
            .snapshot
            .get_data_map_value("balances", "bob")
            .and_then(|v| v.as_i64())
            .unwrap();

        assert_eq!(alice, 750);
        assert_eq!(bob, 250);
    }

    #[test]
    fn test_cid_deterministic() {
        let s1 = dsl::Builder::new("test")
            .token("p1", Some(1))
            .token("p2", Some(0))
            .action("t1")
            .flow("p1", "t1")
            .flow("t1", "p2")
            .must_schema();

        let s2 = dsl::Builder::new("test")
            .token("p2", Some(0))
            .token("p1", Some(1))
            .action("t1")
            .flow("t1", "p2")
            .flow("p1", "t1")
            .must_schema();

        assert_eq!(s1.cid(), s2.cid(), "CID should be deterministic regardless of insertion order");
    }

    #[test]
    fn test_sexpr_roundtrip() {
        let builder = dsl::Builder::new("ERC-020")
            .data("balances", "map[address]uint256")
            .exported()
            .action("transfer")
            .guard("balances[from] >= amount")
            .flow("balances", "transfer")
            .keys(&["from"])
            .flow("transfer", "balances")
            .keys(&["to"]);

        let sexpr = builder.to_string();
        let parsed = dsl::parse(&sexpr).unwrap();

        assert_eq!(parsed.name, "ERC-020");
        assert_eq!(parsed.states.len(), 1);
        assert_eq!(parsed.actions.len(), 1);
        assert_eq!(parsed.arcs.len(), 2);
    }

    #[test]
    fn test_codegen() {
        let input = r#"(schema counter
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

        let code = dsl::generate_rust_from_dsl(input, "counter", "make_counter").unwrap();
        assert!(code.contains("pub fn make_counter()"));
        assert!(code.contains("Schema::new"));
    }

    #[test]
    fn test_schema_macro_counter() {
        let s = schema!(r#"(schema counter
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
)"#);

        assert_eq!(s.name, "counter");
        assert_eq!(s.version, "v1.0.0");
        assert_eq!(s.states.len(), 1);
        assert_eq!(s.states[0].id, "count");
        assert!(s.states[0].is_token());
        assert_eq!(s.states[0].initial_tokens(), 5);
        assert_eq!(s.actions.len(), 1);
        assert_eq!(s.actions[0].id, "inc");
        assert_eq!(s.arcs.len(), 1);
        assert_eq!(s.arcs[0].source, "inc");
        assert_eq!(s.arcs[0].target, "count");
    }

    #[test]
    fn test_schema_macro_erc020() {
        let s = schema!(r#"(schema ERC-020
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
)"#);

        assert_eq!(s.name, "ERC-020");
        assert_eq!(s.version, "v1.0.0");
        assert_eq!(s.states.len(), 1);
        assert_eq!(s.states[0].id, "balances");
        assert!(s.states[0].exported);
        assert_eq!(s.states[0].typ, "map[address]uint256");
        assert_eq!(s.actions.len(), 1);
        assert_eq!(s.actions[0].id, "transfer");
        assert_eq!(s.actions[0].guard, "balances[from] >= amount");
        assert_eq!(s.arcs.len(), 2);
        assert_eq!(s.arcs[0].keys, vec!["from"]);
        assert_eq!(s.arcs[1].keys, vec!["to"]);
        assert_eq!(s.constraints.len(), 1);
        assert_eq!(s.constraints[0].id, "conservation");
        assert_eq!(s.constraints[0].expr, "sum(balances) == totalSupply");
    }

    #[test]
    fn test_schema_macro_runtime() {
        let s = schema!(r#"(schema counter
  (states
    (state ready :kind token :initial 3)
    (state done :kind token :initial 0)
  )
  (actions
    (action process)
  )
  (arcs
    (arc ready -> process)
    (arc process -> done)
  )
)"#);

        let mut rt = tokenmodel::Runtime::new(s);
        assert_eq!(rt.tokens("ready"), 3);
        assert_eq!(rt.tokens("done"), 0);

        for _ in 0..3 {
            rt.execute("process").unwrap();
        }

        assert_eq!(rt.tokens("ready"), 0);
        assert_eq!(rt.tokens("done"), 3);
        assert!(rt.execute("process").is_err());
    }

    #[test]
    fn test_schema_macro_matches_runtime_parse() {
        let dsl_input = r#"(schema counter
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

        let macro_schema = schema!(r#"(schema counter
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
)"#);
        let runtime_schema = dsl::parse_schema(dsl_input).unwrap();

        assert_eq!(macro_schema.cid(), runtime_schema.cid());
    }
}
