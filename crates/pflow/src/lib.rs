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

// ZK proof re-exports (behind feature flags)
#[cfg(feature = "zk")]
pub use pflow_zk as zk;
#[cfg(feature = "zk-arkworks")]
pub use pflow_zk_arkworks as zk_arkworks;
#[cfg(feature = "zk-risc0")]
pub use pflow_zk_risc0 as zk_risc0;

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

    // -----------------------------------------------------------------------
    // Integer reduction tests
    // -----------------------------------------------------------------------

    /// Tic-tac-toe: ODE equilibrium on the analysis net should recover
    /// position values center=4, corner=3, edge=2 purely from topology.
    #[test]
    fn test_integer_reduction_tictactoe() {
        use pflow_solver::equilibrium::{solve_until_equilibrium, EquilibriumOptions};
        use pflow_solver::methods;

        // Win patterns: 3 rows, 3 cols, 2 diags
        let win_patterns: [[usize; 3]; 8] = [
            [0, 1, 2], [3, 4, 5], [6, 7, 8],
            [0, 3, 6], [1, 4, 7], [2, 5, 8],
            [0, 4, 8], [2, 4, 6],
        ];

        // Build analysis net (identical to zk_tictactoe example)
        let mut net = PetriNet::new();
        for i in 0..3 {
            for j in 0..3 {
                net.add_place(format!("P{}_{}", i, j), vec![1.0], vec![], 0.0, 0.0, None);
                net.add_place(format!("_X{}_{}", i, j), vec![0.0], vec![], 0.0, 0.0, None);
            }
        }
        for i in 0..3 {
            for j in 0..3 {
                let t = format!("Play{}_{}", i, j);
                net.add_transition(&t, "play", 0.0, 0.0, None);
                net.add_arc(format!("P{}_{}", i, j), &t, vec![1.0], false);
                net.add_arc(&t, format!("P{}_{}", i, j), vec![1.0], false);
                net.add_arc(&t, format!("_X{}_{}", i, j), vec![1.0], false);
            }
        }
        for (line_idx, pat) in win_patterns.iter().enumerate() {
            for &cell in pat {
                let r = cell / 3;
                let c = cell % 3;
                let t = format!("drain_{}_{}_{}", r, c, line_idx);
                net.add_transition(&t, "drain", 0.0, 0.0, None);
                net.add_arc(format!("_X{}_{}", r, c), &t, vec![1.0], false);
            }
        }

        // Run ODE to equilibrium
        let state = net.set_state(None);
        let rates = net.set_rates(None);
        let prob = Problem::new(net, state, [0.0, 200.0], rates);
        let opts = Options { dt: 0.5, ..Options::default_opts() };
        let eq_opts = EquilibriumOptions {
            tolerance: 1e-4,
            consecutive_steps: 3,
            min_time: 0.5,
            check_interval: 5,
        };
        let (_, result) = solve_until_equilibrium(&prob, &methods::tsit5(), &opts, &eq_opts);
        assert!(result.reached, "ODE should reach equilibrium");

        // Extract and invert concentrations
        let mut raw = [0.0f64; 9];
        for r in 0..3 {
            for c in 0..3 {
                let key = format!("_X{}_{}", r, c);
                raw[r * 3 + c] = result.state.get(&key).copied().unwrap_or(0.0);
            }
        }
        let mut values = [0.0f64; 9];
        for i in 0..9 {
            assert!(raw[i] > 1e-10, "cell {} should have positive concentration", i);
            values[i] = 1.0 / raw[i];
        }
        let min_val = values.iter().copied().fold(f64::MAX, f64::min);
        for v in &mut values {
            *v /= min_val;
        }

        // After normalizing by min (edge), ratios are center:corner:edge = 2:1.5:1.
        // This is the 4:3:2 strategic hierarchy scaled so edge=1.
        let expected = [1.5, 1.0, 1.5, 1.0, 2.0, 1.0, 1.5, 1.0, 1.5];
        for i in 0..9 {
            assert!(
                (values[i] - expected[i]).abs() < 0.1,
                "cell {} value {:.2} != expected {:.1}",
                i, values[i], expected[i]
            );
        }

        // Verify the ratios encode the correct strategic hierarchy:
        // center(4 win-lines) > corner(3) > edge(2)
        let center = values[4];
        let corner = values[0];
        let edge = values[1];
        assert!(center > corner, "center ({:.2}) > corner ({:.2})", center, corner);
        assert!(corner > edge, "corner ({:.2}) > edge ({:.2})", corner, edge);
        assert!(
            (center / edge - 2.0).abs() < 0.05,
            "center:edge ratio should be 2:1 (got {:.3})", center / edge
        );
        assert!(
            (corner / edge - 1.5).abs() < 0.05,
            "corner:edge ratio should be 3:2 (got {:.3})", corner / edge
        );
    }

    /// Hold'em: ODE equilibrium on hand analysis net should recover
    /// monotonically increasing hand strength (HC < Pair < ... < SF).
    #[test]
    fn test_integer_reduction_holdem() {
        use pflow_solver::equilibrium::{solve_until_equilibrium, EquilibriumOptions};
        use pflow_solver::methods;

        let hand_keys = ["hc", "pair", "twopair", "trips", "straight", "flush", "fullhouse", "quads", "stflush"];
        let drain_counts: [usize; 9] = [32, 24, 16, 12, 8, 5, 4, 2, 1];

        // Build hand analysis net (identical to zk_holdem example)
        let mut net = PetriNet::new();
        for key in &hand_keys {
            net.add_place(format!("src_{}", key), vec![1.0], vec![], 0.0, 0.0, None);
            net.add_place(format!("val_{}", key), vec![0.0], vec![], 0.0, 0.0, None);
        }
        for key in &hand_keys {
            let t = format!("play_{}", key);
            net.add_transition(&t, "play", 0.0, 0.0, None);
            net.add_arc(format!("src_{}", key), &t, vec![1.0], false);
            net.add_arc(&t, format!("src_{}", key), vec![1.0], false);
            net.add_arc(&t, format!("val_{}", key), vec![1.0], false);
        }
        for (h, key) in hand_keys.iter().enumerate() {
            for d in 0..drain_counts[h] {
                let t = format!("drain_{}_{}", key, d);
                net.add_transition(&t, "drain", 0.0, 0.0, None);
                net.add_arc(format!("val_{}", key), &t, vec![1.0], false);
            }
        }

        // Run ODE to equilibrium
        let state = net.set_state(None);
        let rates = net.set_rates(None);
        let prob = Problem::new(net, state, [0.0, 200.0], rates);
        let opts = Options { dt: 0.5, ..Options::default_opts() };
        let eq_opts = EquilibriumOptions {
            tolerance: 1e-4,
            consecutive_steps: 3,
            min_time: 0.5,
            check_interval: 5,
        };
        let (_, result) = solve_until_equilibrium(&prob, &methods::tsit5(), &opts, &eq_opts);
        assert!(result.reached, "ODE should reach equilibrium");

        // Extract val_H equilibrium concentrations
        let mut values = [0.0f64; 9];
        for (i, key) in hand_keys.iter().enumerate() {
            let label = format!("val_{}", key);
            values[i] = result.state.get(&label).copied().unwrap_or(0.0);
        }

        // Normalize by minimum
        let min_val = values.iter().copied().filter(|&v| v > 1e-10).fold(f64::MAX, f64::min);
        assert!(min_val > 1e-10, "all hand values should be positive");
        for v in &mut values {
            *v /= min_val;
        }

        // Verify strict monotonic ordering: HC < Pair < 2P < 3K < Str < Flush < FH < 4K < SF
        let hand_names = [
            "High Card", "One Pair", "Two Pair", "Three of a Kind",
            "Straight", "Flush", "Full House", "Four of a Kind", "Straight Flush",
        ];
        for i in 0..8 {
            assert!(
                values[i + 1] > values[i],
                "{} ({:.2}) should be > {} ({:.2})",
                hand_names[i + 1], values[i + 1], hand_names[i], values[i]
            );
        }

        // Verify the expected integer ratios from drain counts:
        // val_H ~ 1/drain_count, so normalized = min_drain / drain_count * scale
        // With HC=32 drains (min value=1.0) and SF=1 drain (max value=32.0)
        assert!(
            (values[8] - 32.0).abs() < 0.5,
            "SF should be ~32.0 (got {:.2})", values[8]
        );
        assert!(
            (values[0] - 1.0).abs() < 0.1,
            "HC should be ~1.0 (got {:.2})", values[0]
        );
    }

    /// 5x5 tic-tac-toe: ODE equilibrium should recover cell values from
    /// topology — center(4 win-lines)=2.0, diagonal(3)=1.5, off-diagonal(2)=1.0.
    #[test]
    fn test_integer_reduction_5x5() {
        use pflow_solver::equilibrium::{solve_until_equilibrium, EquilibriumOptions};
        use pflow_solver::methods;

        // Win patterns: 5 rows + 5 cols + 2 diags = 12 lines of length 5
        let win_patterns: [[usize; 5]; 12] = [
            // rows
            [0, 1, 2, 3, 4],
            [5, 6, 7, 8, 9],
            [10, 11, 12, 13, 14],
            [15, 16, 17, 18, 19],
            [20, 21, 22, 23, 24],
            // cols
            [0, 5, 10, 15, 20],
            [1, 6, 11, 16, 21],
            [2, 7, 12, 17, 22],
            [3, 8, 13, 18, 23],
            [4, 9, 14, 19, 24],
            // diags
            [0, 6, 12, 18, 24],
            [4, 8, 12, 16, 20],
        ];

        // Build analysis net
        let mut net = PetriNet::new();
        for r in 0..5 {
            for c in 0..5 {
                net.add_place(format!("P{}_{}", r, c), vec![1.0], vec![], 0.0, 0.0, None);
                net.add_place(format!("_X{}_{}", r, c), vec![0.0], vec![], 0.0, 0.0, None);
            }
        }
        // Catalytic play transitions: P -> _X + P
        for r in 0..5 {
            for c in 0..5 {
                let t = format!("Play{}_{}", r, c);
                net.add_transition(&t, "play", 0.0, 0.0, None);
                net.add_arc(format!("P{}_{}", r, c), &t, vec![1.0], false);
                net.add_arc(&t, format!("P{}_{}", r, c), vec![1.0], false);
                net.add_arc(&t, format!("_X{}_{}", r, c), vec![1.0], false);
            }
        }
        // Drain transitions: one per cell per win line it belongs to
        for (line_idx, pat) in win_patterns.iter().enumerate() {
            for &cell in pat {
                let r = cell / 5;
                let c = cell % 5;
                let t = format!("drain_{}_{}_{}", r, c, line_idx);
                net.add_transition(&t, "drain", 0.0, 0.0, None);
                net.add_arc(format!("_X{}_{}", r, c), &t, vec![1.0], false);
            }
        }

        // Run ODE to equilibrium
        let state = net.set_state(None);
        let rates = net.set_rates(None);
        let prob = Problem::new(net, state, [0.0, 200.0], rates);
        let opts = Options { dt: 0.5, ..Options::default_opts() };
        let eq_opts = EquilibriumOptions {
            tolerance: 1e-4,
            consecutive_steps: 3,
            min_time: 0.5,
            check_interval: 5,
        };
        let (_, result) = solve_until_equilibrium(&prob, &methods::tsit5(), &opts, &eq_opts);
        assert!(result.reached, "ODE should reach equilibrium");

        // Extract and invert concentrations (lower concentration = more drain = higher value)
        let mut raw = [0.0f64; 25];
        for r in 0..5 {
            for c in 0..5 {
                let key = format!("_X{}_{}", r, c);
                raw[r * 5 + c] = result.state.get(&key).copied().unwrap_or(0.0);
            }
        }
        let mut values = [0.0f64; 25];
        for i in 0..25 {
            assert!(raw[i] > 1e-10, "cell {} should have positive concentration", i);
            values[i] = 1.0 / raw[i];
        }
        let min_val = values.iter().copied().fold(f64::MAX, f64::min);
        for v in &mut values {
            *v /= min_val;
        }

        // Expected normalized values:
        //   center (2,2): 4 drains -> 2.0
        //   diagonal cells: 3 drains -> 1.5
        //   off-diagonal cells: 2 drains -> 1.0
        #[rustfmt::skip]
        let expected = [
            1.5, 1.0, 1.0, 1.0, 1.5,
            1.0, 1.5, 1.0, 1.5, 1.0,
            1.0, 1.0, 2.0, 1.0, 1.0,
            1.0, 1.5, 1.0, 1.5, 1.0,
            1.5, 1.0, 1.0, 1.0, 1.5,
        ];
        for i in 0..25 {
            assert!(
                (values[i] - expected[i]).abs() < 0.1,
                "cell {} value {:.2} != expected {:.1}",
                i, values[i], expected[i]
            );
        }

        // Verify the strategic hierarchy: center > diagonal > off-diagonal
        let center = values[12]; // (2,2)
        let diagonal = values[0]; // (0,0)
        let off_diag = values[1]; // (0,1)
        assert!(center > diagonal, "center ({:.2}) > diagonal ({:.2})", center, diagonal);
        assert!(diagonal > off_diag, "diagonal ({:.2}) > off-diagonal ({:.2})", diagonal, off_diag);
        assert!(
            (center / off_diag - 2.0).abs() < 0.05,
            "center:off-diag ratio should be 2:1 (got {:.3})", center / off_diag
        );
        assert!(
            (diagonal / off_diag - 1.5).abs() < 0.05,
            "diagonal:off-diag ratio should be 3:2 (got {:.3})", diagonal / off_diag
        );
    }

    /// 7x7 tic-tac-toe: ODE equilibrium should recover cell values from
    /// topology — center(4)=2.0, diagonal(3)=1.5, off-diagonal(2)=1.0.
    #[test]
    fn test_integer_reduction_7x7() {
        use pflow_solver::equilibrium::{solve_until_equilibrium, EquilibriumOptions};
        use pflow_solver::methods;

        let n: usize = 7;

        // Win patterns: 7 rows + 7 cols + 2 diags = 16 lines of length 7
        let mut win_patterns: Vec<[usize; 7]> = Vec::new();
        for r in 0..n {
            let mut row = [0usize; 7];
            for c in 0..n { row[c] = r * n + c; }
            win_patterns.push(row);
        }
        for c in 0..n {
            let mut col = [0usize; 7];
            for r in 0..n { col[r] = r * n + c; }
            win_patterns.push(col);
        }
        let mut diag1 = [0usize; 7];
        let mut diag2 = [0usize; 7];
        for i in 0..n {
            diag1[i] = i * n + i;
            diag2[i] = i * n + (n - 1 - i);
        }
        win_patterns.push(diag1);
        win_patterns.push(diag2);
        assert_eq!(win_patterns.len(), 16);

        // Build analysis net
        let mut net = PetriNet::new();
        for r in 0..n {
            for c in 0..n {
                net.add_place(format!("P{}_{}", r, c), vec![1.0], vec![], 0.0, 0.0, None);
                net.add_place(format!("_X{}_{}", r, c), vec![0.0], vec![], 0.0, 0.0, None);
            }
        }
        for r in 0..n {
            for c in 0..n {
                let t = format!("Play{}_{}", r, c);
                net.add_transition(&t, "play", 0.0, 0.0, None);
                net.add_arc(format!("P{}_{}", r, c), &t, vec![1.0], false);
                net.add_arc(&t, format!("P{}_{}", r, c), vec![1.0], false);
                net.add_arc(&t, format!("_X{}_{}", r, c), vec![1.0], false);
            }
        }
        for (line_idx, pat) in win_patterns.iter().enumerate() {
            for &cell in pat {
                let r = cell / n;
                let c = cell % n;
                let t = format!("drain_{}_{}_{}", r, c, line_idx);
                net.add_transition(&t, "drain", 0.0, 0.0, None);
                net.add_arc(format!("_X{}_{}", r, c), &t, vec![1.0], false);
            }
        }

        // Run ODE to equilibrium
        let state = net.set_state(None);
        let rates = net.set_rates(None);
        let prob = Problem::new(net, state, [0.0, 200.0], rates);
        let opts = Options { dt: 0.5, ..Options::default_opts() };
        let eq_opts = EquilibriumOptions {
            tolerance: 1e-4,
            consecutive_steps: 3,
            min_time: 0.5,
            check_interval: 5,
        };
        let (_, result) = solve_until_equilibrium(&prob, &methods::tsit5(), &opts, &eq_opts);
        assert!(result.reached, "ODE should reach equilibrium");

        // Extract and invert concentrations
        let mut raw = vec![0.0f64; n * n];
        for r in 0..n {
            for c in 0..n {
                let key = format!("_X{}_{}", r, c);
                raw[r * n + c] = result.state.get(&key).copied().unwrap_or(0.0);
            }
        }
        let mut values = vec![0.0f64; n * n];
        for i in 0..n * n {
            assert!(raw[i] > 1e-10, "cell {} should have positive concentration", i);
            values[i] = 1.0 / raw[i];
        }
        let min_val = values.iter().copied().fold(f64::MAX, f64::min);
        for v in &mut values {
            *v /= min_val;
        }

        // Expected: center=2.0, 12 diagonal cells=1.5, 36 off-diagonal=1.0
        let mut expected = vec![1.0f64; n * n];
        for i in 0..n {
            expected[i * n + i] = 1.5;             // main diagonal
            expected[i * n + (n - 1 - i)] = 1.5;   // anti-diagonal
        }
        expected[3 * n + 3] = 2.0; // center on both diagonals

        for i in 0..n * n {
            assert!(
                (values[i] - expected[i]).abs() < 0.1,
                "cell {} value {:.2} != expected {:.1}",
                i, values[i], expected[i]
            );
        }

        // Verify the strategic hierarchy
        let center = values[3 * n + 3];
        let diagonal = values[0];       // (0,0)
        let off_diag = values[1];       // (0,1)
        assert!(center > diagonal, "center ({:.2}) > diagonal ({:.2})", center, diagonal);
        assert!(diagonal > off_diag, "diagonal ({:.2}) > off-diagonal ({:.2})", diagonal, off_diag);
        assert!(
            (center / off_diag - 2.0).abs() < 0.05,
            "center:off-diag ratio should be 2:1 (got {:.3})", center / off_diag
        );
        assert!(
            (diagonal / off_diag - 1.5).abs() < 0.05,
            "diagonal:off-diag ratio should be 3:2 (got {:.3})", diagonal / off_diag
        );
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
