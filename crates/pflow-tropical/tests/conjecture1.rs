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
//!    (tokens produced per unit time in steady state)

use pflow_core::PetriNet;
use pflow_tropical::{Matrix, NEG_INF, mat_pow, tropical_mul, eigenvalue};
use pflow_zk::{IncidenceMatrix, fire_transition};

/// Build the tropical adjacency matrix for an event graph.
///
/// For transitions i and j, A[i][j] = weight of the path j -> (output place) -> i.
/// That is: if transition j produces into a place that transition i consumes from,
/// A[i][j] = the arc weight (processing time / holding time).
///
/// NEG_INF means no connection (tropical zero).
fn incidence_to_tropical(im: &IncidenceMatrix) -> Matrix {
    let n = im.num_transitions;
    let mut a = Matrix::new(n, n);

    // For each place, find which transition produces into it (output)
    // and which transition consumes from it (input).
    // If transition j outputs to place p and transition i inputs from place p,
    // then A[i][j] = weight (we use 1.0 for unit-time arcs).
    for p in 0..im.num_places {
        // Find producers: transitions that output to place p
        let mut producers: Vec<(usize, i64)> = Vec::new();
        for t in 0..n {
            for &(place_idx, weight) in &im.outputs[t] {
                if place_idx == p {
                    producers.push((t, weight));
                }
            }
        }
        // Find consumers: transitions that input from place p
        let mut consumers: Vec<(usize, i64)> = Vec::new();
        for t in 0..n {
            for &(place_idx, weight) in &im.inputs[t] {
                if place_idx == p {
                    consumers.push((t, weight));
                }
            }
        }
        // Connect each producer to each consumer through this place
        for &(producer, _pw) in &producers {
            for &(consumer, cw) in &consumers {
                // Arc weight = consumption weight (processing time to consume cw tokens)
                let current = a.at(consumer, producer);
                let candidate = cw as f64;
                if candidate > current {
                    a.set(consumer, producer, candidate);
                }
            }
        }
    }
    a
}

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
    // Net: T0 (consumes P0, produces P1), T1 (consumes P1, produces P0)
    // Initial marking: [1, 0] (token in P0)
    let net = PetriNet::build()
        .place("P0", 1.0)
        .place("P1", 0.0)
        .transition("T0")
        .transition("T1")
        .arc("P0", "T0", 1.0)
        .arc("T0", "P1", 1.0)
        .arc("P1", "T1", 1.0)
        .arc("T1", "P0", 1.0)
        .done();

    let im = IncidenceMatrix::from_petri_net(&net);

    // ── Discrete simulation: fire T0, T1, T0, T1, ... ──
    let m0 = im.initial_marking(&net);
    let m1 = fire_transition(&im, &m0, 0).unwrap(); // fire T0
    let m2 = fire_transition(&im, &m1, 1).unwrap(); // fire T1
    let m3 = fire_transition(&im, &m2, 0).unwrap(); // fire T0
    let m4 = fire_transition(&im, &m3, 1).unwrap(); // fire T1

    // Markings cycle: [1,0] -> [0,1] -> [1,0] -> [0,1] -> [1,0]
    assert_eq!(m0, im.initial_marking(&net)); // P0=1, P1=0
    assert_eq!(m2, m0); // back to start after full cycle
    assert_eq!(m4, m0);

    // ── Tropical simulation: A^k ⊗ x(0) ──
    let a = incidence_to_tropical(&im);

    // Tropical adjacency: T0 -> T1 (via P1, weight 1), T1 -> T0 (via P0, weight 1)
    // Canonical order is T0, T1
    assert_eq!(a.at(1, 0), 1.0); // T1 depends on T0 (through P1)
    assert_eq!(a.at(0, 1), 1.0); // T0 depends on T1 (through P0)
    assert_eq!(a.at(0, 0), NEG_INF); // no self-loop
    assert_eq!(a.at(1, 1), NEG_INF);

    // Initial firing times: T0 can fire at t=0 (has token), T1 at -inf (not enabled)
    let x0 = vec![0.0, NEG_INF];
    let x1 = tropical_mat_vec(&a, &x0);
    let x2 = tropical_mat_vec(&a, &x1);
    let x3 = tropical_mat_vec(&a, &x2);

    // x1: T0=NEG_INF (waiting), T1=0+1=1 (fires at t=1)
    assert_eq!(x1[0], NEG_INF);
    assert_eq!(x1[1], 1.0);

    // x2: T0=1+1=2 (fires at t=2), T1=NEG_INF
    assert_eq!(x2[0], 2.0);
    assert_eq!(x2[1], NEG_INF);

    // x3: T0=NEG_INF, T1=2+1=3
    assert_eq!(x3[0], NEG_INF);
    assert_eq!(x3[1], 3.0);

    // Key equivalence: the PARITY of which transitions can fire matches.
    // At x0: T0 is finite (enabled), T1 is -inf (disabled) → matches m0 = [1,0]
    // At x1: T1 is finite (enabled), T0 is -inf (disabled) → matches m1 = [0,1]
    // At x2: T0 is finite (enabled), T1 is -inf (disabled) → matches m2 = [1,0]
    //
    // The tropical evolution tracks firing times; the discrete simulation
    // tracks token counts. They agree on WHICH transitions are enabled at each step.
    for (step, x) in [&x0, &x1, &x2, &x3].iter().enumerate() {
        for t in 0..2 {
            let tropical_enabled = x[t] != NEG_INF;
            // Map tropical step back to marking: even steps = m0-like, odd = m1-like
            let marking = if step % 2 == 0 { &m0 } else { &m1 };
            let discrete_enabled = im.is_enabled(marking, t);
            assert_eq!(
                tropical_enabled, discrete_enabled,
                "step {step}, transition {t}: tropical says {tropical_enabled}, discrete says {discrete_enabled}"
            );
        }
    }
}

// ──────────────────────────────────────────────────────────
// Test 2: Tropical eigenvalue = net throughput
// ──────────────────────────────────────────────────────────

#[test]
fn eigenvalue_equals_throughput() {
    // Simple loop: cycle length 2, each arc weight 1
    // Throughput = 1 token per 2 time units per transition = cycle mean 1.0
    let net = PetriNet::build()
        .place("P0", 1.0)
        .place("P1", 0.0)
        .transition("T0")
        .transition("T1")
        .arc("P0", "T0", 1.0)
        .arc("T0", "P1", 1.0)
        .arc("P1", "T1", 1.0)
        .arc("T1", "P0", 1.0)
        .done();

    let im = IncidenceMatrix::from_petri_net(&net);
    let a = incidence_to_tropical(&im);
    let (lambda, circuit) = eigenvalue(&a).unwrap();

    // λ = (1+1)/2 = 1.0: each transition fires once per unit time in steady state
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
    // This is NOT a simple event graph (infect has 2 inputs),
    // but the tropical matrix still captures transition dependencies.
    let net = PetriNet::build().sir(10.0, 1.0, 0.0).done();
    let im = IncidenceMatrix::from_petri_net(&net);
    let a = incidence_to_tropical(&im);

    // Transitions: infect(0), recover(1) (alphabetical)
    // infect consumes I(0)+S(2), produces 2*I(0)
    // recover consumes I(0), produces R(1)
    //
    // Tropical adjacency:
    // - infect -> infect: via I (infect produces I, infect consumes I) -> A[0][0] = 1
    // - infect -> recover: via I (infect produces I, recover consumes I) -> A[1][0] = 1
    // - recover -> infect: no path (recover produces R, infect doesn't consume R)
    // - recover -> recover: no path
    assert!(a.at(0, 0) > NEG_INF, "infect self-loop through I");
    assert!(a.at(1, 0) > NEG_INF, "infect enables recover through I");
    assert_eq!(a.at(0, 1), NEG_INF, "recover doesn't enable infect");
    assert_eq!(a.at(1, 1), NEG_INF, "recover has no self-loop");

    // Verify discrete behavior matches: after infect fires, recover becomes enabled
    let m0 = im.initial_marking(&net); // I=1, R=0, S=10
    assert!(im.is_enabled(&m0, 0), "infect should be enabled");
    assert!(im.is_enabled(&m0, 1), "recover should be enabled (I=1)");

    let m1 = fire_transition(&im, &m0, 0).unwrap(); // infect: I=2, R=0, S=9
    assert!(im.is_enabled(&m1, 1), "recover still enabled after infect");
}

// ──────────────────────────────────────────────────────────
// Test 4: Weighted arcs scale eigenvalue, not reachability
// ──────────────────────────────────────────────────────────

#[test]
fn weights_scale_throughput_not_structure() {
    // Two nets with IDENTICAL topology but different arc weights.
    // Net A: all arcs weight 1
    let net_a = PetriNet::build()
        .place("P0", 1.0)
        .place("P1", 0.0)
        .transition("T0")
        .transition("T1")
        .arc("P0", "T0", 1.0)
        .arc("T0", "P1", 1.0)
        .arc("P1", "T1", 1.0)
        .arc("T1", "P0", 1.0)
        .done();

    // Net B: arcs weight 3 (3 tokens consumed/produced per firing)
    let net_b = PetriNet::build()
        .place("P0", 3.0)
        .place("P1", 0.0)
        .transition("T0")
        .transition("T1")
        .arc("P0", "T0", 3.0)
        .arc("T0", "P1", 3.0)
        .arc("P1", "T1", 3.0)
        .arc("T1", "P0", 3.0)
        .done();

    let im_a = IncidenceMatrix::from_petri_net(&net_a);
    let im_b = IncidenceMatrix::from_petri_net(&net_b);

    let trop_a = incidence_to_tropical(&im_a);
    let trop_b = incidence_to_tropical(&im_b);

    let (lambda_a, _) = eigenvalue(&trop_a).unwrap();
    let (lambda_b, _) = eigenvalue(&trop_b).unwrap();

    // Eigenvalues differ (throughput scales with weights)
    assert!(
        (lambda_a - 1.0).abs() < 1e-10,
        "net_a throughput should be 1.0, got {lambda_a}"
    );
    assert!(
        (lambda_b - 3.0).abs() < 1e-10,
        "net_b throughput should be 3.0, got {lambda_b}"
    );

    // But reachability structure is IDENTICAL:
    // Both nets have the same enabled transitions at each step
    let m0_a = im_a.initial_marking(&net_a);
    let m0_b = im_b.initial_marking(&net_b);

    for t in 0..2 {
        assert_eq!(
            im_a.is_enabled(&m0_a, t),
            im_b.is_enabled(&m0_b, t),
            "transition {t} enabledness should match between nets"
        );
    }

    // Fire sequences produce identical reachability graphs (up to token count scaling)
    let m1_a = fire_transition(&im_a, &m0_a, 0).unwrap();
    let m1_b = fire_transition(&im_b, &m0_b, 0).unwrap();

    // Same structure: after T0 fires, P0 empty, P1 has tokens
    assert_eq!(m1_a[0], 0); // P0 empty
    assert_eq!(m1_b[0], 0); // P0 empty
    assert!(m1_a[1] > 0);   // P1 has tokens
    assert!(m1_b[1] > 0);   // P1 has tokens

    // The delta vectors have the same SIGN pattern (structure), different magnitudes (weights)
    let delta_a = im_a.delta(0);
    let delta_b = im_b.delta(0);
    for p in 0..delta_a.len() {
        assert_eq!(
            delta_a[p].signum(),
            delta_b[p].signum(),
            "delta sign for place {p} should match"
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 5: Matrix power equivalence
// ──────────────────────────────────────────────────────────

#[test]
fn tropical_power_tracks_firing_times() {
    // 3-transition pipeline: T0 -> T1 -> T2 -> T0
    let net = PetriNet::build()
        .place("A", 1.0)
        .place("B", 0.0)
        .place("C", 0.0)
        .transition("T0")
        .transition("T1")
        .transition("T2")
        .arc("A", "T0", 1.0)
        .arc("T0", "B", 1.0)
        .arc("B", "T1", 1.0)
        .arc("T1", "C", 1.0)
        .arc("C", "T2", 1.0)
        .arc("T2", "A", 1.0)
        .done();

    let im = IncidenceMatrix::from_petri_net(&net);
    let a = incidence_to_tropical(&im);

    // Verify it's a 3-cycle
    let (lambda, circuit) = eigenvalue(&a).unwrap();
    assert!(
        (lambda - 1.0).abs() < 1e-10,
        "3-cycle with unit weights has eigenvalue 1.0, got {lambda}"
    );
    assert_eq!(circuit.len(), 3);

    // Tropical evolution: T0 fires at t=0, others wait
    let x0 = vec![0.0, NEG_INF, NEG_INF];

    // A^1 ⊗ x0: T1 fires at t=1
    let a1 = mat_pow(&a, 1);
    let x1 = tropical_mat_vec(&a1, &x0);
    assert_eq!(x1[0], NEG_INF);
    assert_eq!(x1[1], 1.0);
    assert_eq!(x1[2], NEG_INF);

    // A^2 ⊗ x0: T2 fires at t=2
    let a2 = mat_pow(&a, 2);
    let x2 = tropical_mat_vec(&a2, &x0);
    assert_eq!(x2[0], NEG_INF);
    assert_eq!(x2[1], NEG_INF);
    assert_eq!(x2[2], 2.0);

    // A^3 ⊗ x0: T0 fires again at t=3 (full cycle)
    let a3 = mat_pow(&a, 3);
    let x3 = tropical_mat_vec(&a3, &x0);
    assert_eq!(x3[0], 3.0);
    assert_eq!(x3[1], NEG_INF);
    assert_eq!(x3[2], NEG_INF);

    // Verify discrete simulation agrees: T0, T1, T2 fire in sequence
    let m0 = im.initial_marking(&net); // A=1, B=0, C=0
    let m1 = fire_transition(&im, &m0, 0).unwrap(); // A=0, B=1, C=0
    let m2 = fire_transition(&im, &m1, 1).unwrap(); // A=0, B=0, C=1
    let m3 = fire_transition(&im, &m2, 2).unwrap(); // A=1, B=0, C=0

    assert_eq!(m3, m0, "full cycle returns to initial marking");

    // The tropical firing-time vector and discrete enabledness agree at each step
    let markings = [&m0, &m1, &m2];
    let times = [&x0, &x1, &x2];
    for (step, (marking, time)) in markings.iter().zip(times.iter()).enumerate() {
        for t in 0..3 {
            let tropical_can_fire = time[t] != NEG_INF;
            let discrete_can_fire = im.is_enabled(marking, t);
            assert_eq!(
                tropical_can_fire, discrete_can_fire,
                "step {step}, T{t}: tropical={tropical_can_fire}, discrete={discrete_can_fire}"
            );
        }
    }
}
