//! Conjecture 1: Elementary nets are natively max-plus.
//!
//! Proof by construction: given a Petri net, show that the discrete firing rule
//! (consume/produce tokens via incidence matrix) and the tropical matrix-vector
//! product (max-plus evolution of firing times) agree on reachable state sequences.
//!
//! For a timed event graph, the evolution x(k+1) = A ⊗ x(k) where:
//!   - A[i][j] = arc weight from transition j's output place to transition i's input place
//!   - x(k) = vector of earliest firing times at step k
//!   - ⊗ is tropical matrix-vector multiply: x'[i] = max_j(A[i][j] + x[j])
//!
//! We verify two properties:
//! 1. **Firing-time equivalence**: tropical A^k ⊗ x(0) matches step-by-step simulation
//! 2. **Throughput from topology**: the tropical eigenvalue λ equals the net's throughput

use pflow_tropical::{Matrix, NEG_INF, mat_pow, tropical_mul, eigenvalue, NetMatrix};
use pflow_tropical::ttt_fixtures::pilot_ttt;

/// Tropical matrix-vector multiply: y[i] = max_j(A[i][j] + x[j]).
fn tropical_mat_vec(a: &Matrix, x: &[f64]) -> Vec<f64> {
    assert_eq!(a.cols, x.len());
    let mut y = vec![NEG_INF; a.rows];
    for i in 0..a.rows {
        for j in 0..a.cols {
            let v = tropical_mul(a.at(i, j), x[j]);
            if v > y[i] {
                y[i] = v;
            }
        }
    }
    y
}

// ──────────────────────────────────────────────────────────
// Test 1: Simple loop (P0 <-> P1)
// ──────────────────────────────────────────────────────────

#[test]
fn simple_loop_firing_matches_tropical() {
    let net = NetMatrix::from_incidence(
        vec![vec![-1, 1], vec![1, -1]],
        vec![1, 0],
        vec!["P0".into(), "P1".into()],
        vec!["T0".into(), "T1".into()],
    );

    // ── Discrete simulation: fire T0, T1, T0, T1, ... ──
    let m0 = net.initial.clone();
    let m1 = net.fire(&m0, 0).unwrap();
    let m2 = net.fire(&m1, 1).unwrap();
    let m3 = net.fire(&m2, 0).unwrap();
    let m4 = net.fire(&m3, 1).unwrap();

    assert_eq!(m0, vec![1, 0]);
    assert_eq!(m2, m0); // back to start after full cycle
    assert_eq!(m4, m0);

    // ── Tropical simulation: A^k ⊗ x(0) ──
    let a = net.tropical_adjacency();

    assert_eq!(a.at(1, 0), 1.0); // T1 depends on T0 (through P1)
    assert_eq!(a.at(0, 1), 1.0); // T0 depends on T1 (through P0)
    assert_eq!(a.at(0, 0), NEG_INF);
    assert_eq!(a.at(1, 1), NEG_INF);

    let x0 = vec![0.0, NEG_INF];
    let x1 = tropical_mat_vec(&a, &x0);
    let x2 = tropical_mat_vec(&a, &x1);
    let x3 = tropical_mat_vec(&a, &x2);

    assert_eq!(x1[0], NEG_INF);
    assert_eq!(x1[1], 1.0);
    assert_eq!(x2[0], 2.0);
    assert_eq!(x2[1], NEG_INF);
    assert_eq!(x3[0], NEG_INF);
    assert_eq!(x3[1], 3.0);

    // Tropical and discrete agree on which transitions are enabled
    for (step, x) in [&x0, &x1, &x2, &x3].iter().enumerate() {
        for t in 0..2 {
            let tropical_enabled = x[t] != NEG_INF;
            let marking = if step % 2 == 0 { &m0 } else { &m1 };
            let discrete_enabled = net.is_enabled(marking, t);
            assert_eq!(
                tropical_enabled, discrete_enabled,
                "step {step}, transition {t}: tropical={tropical_enabled}, discrete={discrete_enabled}"
            );
        }
    }
}

// ──────────────────────────────────────────────────────────
// Test 2: Tropical eigenvalue = net throughput
// ──────────────────────────────────────────────────────────

#[test]
fn eigenvalue_equals_throughput() {
    let net = NetMatrix::from_incidence(
        vec![vec![-1, 1], vec![1, -1]],
        vec![1, 0],
        vec!["P0".into(), "P1".into()],
        vec!["T0".into(), "T1".into()],
    );

    let a = net.tropical_adjacency();
    let (lambda, circuit) = eigenvalue(&a).unwrap();

    assert!(
        (lambda - 1.0).abs() < 1e-10,
        "expected throughput 1.0, got {lambda}"
    );
    assert_eq!(circuit.len(), 2, "critical circuit should have 2 nodes");
}

// ──────────────────────────────────────────────────────────
// Test 3: SIR model — asymmetric topology
// ──────────────────────────────────────────────────────────

#[test]
fn sir_tropical_structure() {
    // SIR: infect (S+I -> 2I), recover (I -> R)
    // Places alphabetical: I(0), R(1), S(2)
    // infect: consumes I(1)+S(1), produces 2I => incidence = [+1, 0, -1], input = [1, 0, 1]
    // recover: consumes I(1), produces R(1) => incidence = [-1, +1, 0], input = [1, 0, 0]
    let net = NetMatrix::new(
        vec![vec![1, 0, -1], vec![-1, 1, 0]],
        vec![vec![1, 0, 1], vec![1, 0, 0]],
        vec![1, 0, 10],
        vec!["I".into(), "R".into(), "S".into()],
        vec!["infect".into(), "recover".into()],
    );

    let a = net.tropical_adjacency();

    // infect -> infect: via I (infect produces I, infect consumes I)
    assert!(a.at(0, 0) > NEG_INF, "infect self-loop through I");
    // infect -> recover: via I (infect produces I, recover consumes I)
    assert!(a.at(1, 0) > NEG_INF, "infect enables recover through I");
    // recover -> infect: no (recover produces R, infect doesn't consume R)
    assert_eq!(a.at(0, 1), NEG_INF, "recover doesn't enable infect");
    // recover -> recover: no
    assert_eq!(a.at(1, 1), NEG_INF, "recover has no self-loop");

    // Discrete: after infect fires, recover is still enabled
    let m0 = net.initial.clone();
    assert!(net.is_enabled(&m0, 0), "infect enabled");
    assert!(net.is_enabled(&m0, 1), "recover enabled (I=10)");

    let m1 = net.fire(&m0, 0).unwrap(); // infect: I grows, S shrinks
    assert!(net.is_enabled(&m1, 1), "recover still enabled after infect");
}

// ──────────────────────────────────────────────────────────
// Test 4: Weighted arcs scale eigenvalue, not reachability
// ──────────────────────────────────────────────────────────

#[test]
fn weights_scale_throughput_not_structure() {
    let net_a = NetMatrix::from_incidence(
        vec![vec![-1, 1], vec![1, -1]],
        vec![1, 0],
        vec!["P0".into(), "P1".into()],
        vec!["T0".into(), "T1".into()],
    );
    let net_b = NetMatrix::from_incidence(
        vec![vec![-3, 3], vec![3, -3]],
        vec![3, 0],
        vec!["P0".into(), "P1".into()],
        vec!["T0".into(), "T1".into()],
    );

    let trop_a = net_a.tropical_adjacency();
    let trop_b = net_b.tropical_adjacency();

    let (lambda_a, _) = eigenvalue(&trop_a).unwrap();
    let (lambda_b, _) = eigenvalue(&trop_b).unwrap();

    assert!((lambda_a - 1.0).abs() < 1e-10, "net_a throughput: {lambda_a}");
    assert!((lambda_b - 3.0).abs() < 1e-10, "net_b throughput: {lambda_b}");

    // Reachability structure is identical
    for t in 0..2 {
        assert_eq!(
            net_a.is_enabled(&net_a.initial, t),
            net_b.is_enabled(&net_b.initial, t),
        );
    }

    let m1_a = net_a.fire(&net_a.initial, 0).unwrap();
    let m1_b = net_b.fire(&net_b.initial, 0).unwrap();
    assert_eq!(m1_a[0], 0);
    assert_eq!(m1_b[0], 0);
    assert!(m1_a[1] > 0);
    assert!(m1_b[1] > 0);

    // Sign pattern matches
    let delta_a = net_a.delta(0);
    let delta_b = net_b.delta(0);
    for p in 0..delta_a.len() {
        assert_eq!(delta_a[p].signum(), delta_b[p].signum());
    }
}

// ──────────────────────────────────────────────────────────
// Test 5: Matrix power equivalence — 3-transition pipeline
// ──────────────────────────────────────────────────────────

#[test]
fn tropical_power_tracks_firing_times() {
    let net = NetMatrix::from_incidence(
        vec![
            vec![-1, 1, 0], // T0: A -> B
            vec![0, -1, 1], // T1: B -> C
            vec![1, 0, -1], // T2: C -> A
        ],
        vec![1, 0, 0],
        vec!["A".into(), "B".into(), "C".into()],
        vec!["T0".into(), "T1".into(), "T2".into()],
    );

    let a = net.tropical_adjacency();
    let (lambda, circuit) = eigenvalue(&a).unwrap();
    assert!((lambda - 1.0).abs() < 1e-10);
    assert_eq!(circuit.len(), 3);

    let x0 = vec![0.0, NEG_INF, NEG_INF];

    let a1 = mat_pow(&a, 1);
    let x1 = tropical_mat_vec(&a1, &x0);
    assert_eq!(x1, vec![NEG_INF, 1.0, NEG_INF]);

    let a2 = mat_pow(&a, 2);
    let x2 = tropical_mat_vec(&a2, &x0);
    assert_eq!(x2, vec![NEG_INF, NEG_INF, 2.0]);

    let a3 = mat_pow(&a, 3);
    let x3 = tropical_mat_vec(&a3, &x0);
    assert_eq!(x3, vec![3.0, NEG_INF, NEG_INF]);

    // Discrete simulation agrees
    let m0 = net.initial.clone();
    let m1 = net.fire(&m0, 0).unwrap();
    let m2 = net.fire(&m1, 1).unwrap();
    let m3 = net.fire(&m2, 2).unwrap();
    assert_eq!(m3, m0, "full cycle returns to initial marking");

    let markings = [&m0, &m1, &m2];
    let times = [&x0, &x1, &x2];
    for (step, (marking, time)) in markings.iter().zip(times.iter()).enumerate() {
        for t in 0..3 {
            let tropical_can_fire = time[t] != NEG_INF;
            let discrete_can_fire = net.is_enabled(marking, t);
            assert_eq!(
                tropical_can_fire, discrete_can_fire,
                "step {step}, T{t}: tropical={tropical_can_fire}, discrete={discrete_can_fire}"
            );
        }
    }
}

// ──────────────────────────────────────────────────────────
// Test 6: TTT — tropical adjacency captures game structure
// ──────────────────────────────────────────────────────────

#[test]
fn ttt_tropical_adjacency_structure() {
    let net = pilot_ttt();

    // Verify dimensions
    assert_eq!(net.num_places(), 33);
    assert_eq!(net.num_transitions(), 35);

    let a = net.tropical_adjacency();
    assert_eq!(a.rows, 35);
    assert_eq!(a.cols, 35);

    // Find transition indices by label
    let find_t = |name: &str| -> usize {
        net.transition_labels.iter().position(|l| l == name).unwrap()
    };

    let x_play_00 = find_t("x_play_00");
    let o_play_11 = find_t("o_play_11");
    let x_win_row0 = find_t("x_win_row0");

    // x_play_00 -> o_play_11: connected via o_turn (x produces o_turn, o consumes it)
    assert!(
        a.at(o_play_11, x_play_00) > NEG_INF,
        "x_play should enable o_play via o_turn"
    );

    // o_play_11 -> x_play_00: connected via x_turn
    assert!(
        a.at(x_play_00, o_play_11) > NEG_INF,
        "o_play should enable x_play via x_turn"
    );

    // x_play_00 -> x_play_00: NOT connected (x_play consumes x_turn, doesn't produce it)
    assert_eq!(
        a.at(x_play_00, x_play_00), NEG_INF,
        "x_play shouldn't self-loop"
    );

    // x_play_00 -> x_win_row0: connected via x00 (x_play produces x00, x_win reads it)
    assert!(
        a.at(x_win_row0, x_play_00) > NEG_INF,
        "x_play_00 should enable x_win_row0 via x00"
    );
}

// ──────────────────────────────────────────────────────────
// Test 7: TTT — discrete firing matches expected game flow
// ──────────────────────────────────────────────────────────

#[test]
fn ttt_discrete_firing_flow() {
    let net = pilot_ttt();

    let find_t = |name: &str| -> usize {
        net.transition_labels.iter().position(|l| l == name).unwrap()
    };
    let find_p = |name: &str| -> usize {
        net.place_labels.iter().position(|l| l == name).unwrap()
    };

    let m0 = net.initial.clone();

    // Initially: x_turn=1, game_active=1, all p cells=1
    assert_eq!(m0[find_p("x_turn")], 1);
    assert_eq!(m0[find_p("o_turn")], 0);
    assert_eq!(m0[find_p("game_active")], 1);

    // X plays center (x_play_11)
    let x_play_11 = find_t("x_play_11");
    assert!(net.is_enabled(&m0, x_play_11));
    let m1 = net.fire(&m0, x_play_11).unwrap();

    // After x_play_11: x11=1, p11=0, o_turn=1, x_turn=0, move_tokens=1
    assert_eq!(m1[find_p("x11")], 1);
    assert_eq!(m1[find_p("p11")], 0);
    assert_eq!(m1[find_p("o_turn")], 1);
    assert_eq!(m1[find_p("x_turn")], 0);
    assert_eq!(m1[find_p("move_tokens")], 1);

    // O plays corner (o_play_00)
    let o_play_00 = find_t("o_play_00");
    assert!(net.is_enabled(&m1, o_play_00));
    let m2 = net.fire(&m1, o_play_00).unwrap();

    // After o_play_00: o00=1, p00=0, x_turn=1, o_turn=0, move_tokens=2
    assert_eq!(m2[find_p("o00")], 1);
    assert_eq!(m2[find_p("p00")], 0);
    assert_eq!(m2[find_p("x_turn")], 1);
    assert_eq!(m2[find_p("move_tokens")], 2);

    // Can't play same cell twice
    assert!(!net.is_enabled(&m2, x_play_11), "p11 empty, can't replay");
    assert!(!net.is_enabled(&m2, o_play_00), "p00 empty + wrong turn");
}
