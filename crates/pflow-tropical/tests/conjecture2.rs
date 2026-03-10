//! Conjecture 2: Structure trumps weights.
//!
//! For Petri nets with identical topology but different arc weights:
//! - Conservation laws (P-invariants) have the SAME support
//! - Reproducible firing sequences (T-invariants) have the SAME support
//! - Reachability structure (which transitions are enabled, sign of deltas) is identical
//! - Only throughput (tropical eigenvalue) scales with weights
//!
//! This proves that behavioral properties are topological while performance
//! properties are parametric.

use pflow_core::PetriNet;
use pflow_tropical::{
    dense_incidence, eigenvalue, p_invariants, sign_pattern, support, t_invariants, Matrix,
    NEG_INF,
};
use pflow_zk::{fire_transition, IncidenceMatrix};

/// Build the tropical adjacency from an incidence matrix (same as conjecture1).
fn incidence_to_tropical(im: &IncidenceMatrix) -> Matrix {
    let n = im.num_transitions;
    let mut a = Matrix::new(n, n);
    for p in 0..im.num_places {
        let mut producers: Vec<(usize, i64)> = Vec::new();
        for t in 0..n {
            for &(place_idx, weight) in &im.outputs[t] {
                if place_idx == p {
                    producers.push((t, weight));
                }
            }
        }
        let mut consumers: Vec<(usize, i64)> = Vec::new();
        for t in 0..n {
            for &(place_idx, weight) in &im.inputs[t] {
                if place_idx == p {
                    consumers.push((t, weight));
                }
            }
        }
        for &(producer, _pw) in &producers {
            for &(consumer, cw) in &consumers {
                let candidate = cw as f64;
                if candidate > a.at(consumer, producer) {
                    a.set(consumer, producer, candidate);
                }
            }
        }
    }
    a
}

/// Build a loop net with given arc weight (all arcs same weight).
fn loop_net(weight: f64) -> PetriNet {
    PetriNet::build()
        .place("P0", weight) // enough tokens to fire
        .place("P1", 0.0)
        .transition("T0")
        .transition("T1")
        .arc("P0", "T0", weight)
        .arc("T0", "P1", weight)
        .arc("P1", "T1", weight)
        .arc("T1", "P0", weight)
        .done()
}

/// Build a 3-place pipeline with given arc weight.
fn pipeline_net(weight: f64) -> PetriNet {
    PetriNet::build()
        .place("A", weight)
        .place("B", 0.0)
        .place("C", 0.0)
        .transition("T0")
        .transition("T1")
        .transition("T2")
        .arc("A", "T0", weight)
        .arc("T0", "B", weight)
        .arc("B", "T1", weight)
        .arc("T1", "C", weight)
        .arc("C", "T2", weight)
        .arc("T2", "A", weight)
        .done()
}

// ──────────────────────────────────────────────────────────
// Test 1: P-invariant support is topological
// ──────────────────────────────────────────────────────────

#[test]
fn p_invariant_support_identical_across_weights() {
    for weight in [1.0, 2.0, 5.0, 10.0] {
        let net = loop_net(weight);
        let im = IncidenceMatrix::from_petri_net(&net);
        let pinv = p_invariants(&im);

        assert_eq!(
            pinv.len(),
            1,
            "weight={weight}: expected 1 P-invariant, got {}",
            pinv.len()
        );

        // Support is always {P0, P1} regardless of weight
        let sup = support(&pinv[0]);
        assert_eq!(
            sup,
            vec![0, 1],
            "weight={weight}: P-invariant should span both places"
        );
    }
}

#[test]
fn p_invariant_support_pipeline() {
    for weight in [1.0, 3.0, 7.0] {
        let net = pipeline_net(weight);
        let im = IncidenceMatrix::from_petri_net(&net);
        let pinv = p_invariants(&im);

        assert_eq!(pinv.len(), 1, "weight={weight}: pipeline has 1 P-invariant");
        let sup = support(&pinv[0]);
        assert_eq!(
            sup,
            vec![0, 1, 2],
            "weight={weight}: all 3 places conserved"
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 2: T-invariant support is topological
// ──────────────────────────────────────────────────────────

#[test]
fn t_invariant_support_identical_across_weights() {
    for weight in [1.0, 2.0, 5.0] {
        let net = loop_net(weight);
        let im = IncidenceMatrix::from_petri_net(&net);
        let tinv = t_invariants(&im);

        assert_eq!(
            tinv.len(),
            1,
            "weight={weight}: expected 1 T-invariant, got {}",
            tinv.len()
        );

        // Both transitions participate in the cycle
        let sup = support(&tinv[0]);
        assert_eq!(
            sup,
            vec![0, 1],
            "weight={weight}: T-invariant should include both transitions"
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 3: Delta sign patterns are topological
// ──────────────────────────────────────────────────────────

#[test]
fn delta_sign_pattern_independent_of_weights() {
    let weights = [1.0, 2.0, 5.0, 10.0];

    // Compute sign patterns for each weight
    let patterns: Vec<Vec<Vec<i8>>> = weights
        .iter()
        .map(|&w| {
            let net = loop_net(w);
            let im = IncidenceMatrix::from_petri_net(&net);
            let c = dense_incidence(&im);
            c.iter().map(|row| sign_pattern(row)).collect()
        })
        .collect();

    // All sign patterns must be identical
    let reference = &patterns[0];
    for (i, pattern) in patterns.iter().enumerate().skip(1) {
        assert_eq!(
            pattern, reference,
            "weight={}: delta sign pattern differs from weight={}",
            weights[i], weights[0]
        );
    }
}

#[test]
fn delta_sign_pattern_pipeline() {
    let weights = [1.0, 3.0, 7.0];
    let patterns: Vec<Vec<Vec<i8>>> = weights
        .iter()
        .map(|&w| {
            let net = pipeline_net(w);
            let im = IncidenceMatrix::from_petri_net(&net);
            let c = dense_incidence(&im);
            c.iter().map(|row| sign_pattern(row)).collect()
        })
        .collect();

    let reference = &patterns[0];
    for (i, pattern) in patterns.iter().enumerate().skip(1) {
        assert_eq!(
            pattern, reference,
            "weight={}: pipeline sign pattern differs",
            weights[i]
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 4: Eigenvalue scales with weight (throughput is parametric)
// ──────────────────────────────────────────────────────────

#[test]
fn eigenvalue_scales_linearly_with_weight() {
    let weights = [1.0, 2.0, 5.0, 10.0];
    for &w in &weights {
        let net = loop_net(w);
        let im = IncidenceMatrix::from_petri_net(&net);
        let a = incidence_to_tropical(&im);
        let (lambda, _) = eigenvalue(&a).unwrap();
        assert!(
            (lambda - w).abs() < 1e-10,
            "weight={w}: expected eigenvalue {w}, got {lambda}"
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 5: Enabledness structure is topological
// ──────────────────────────────────────────────────────────

#[test]
fn enabledness_sequence_identical_across_weights() {
    // Fire the same transition sequence on nets with different weights.
    // The sequence of which transitions are enabled should be identical.
    let weights = [1.0, 3.0, 7.0];

    for &w in &weights {
        let net = loop_net(w);
        let im = IncidenceMatrix::from_petri_net(&net);
        let m0 = im.initial_marking(&net);

        // T0 enabled, T1 disabled (tokens in P0 only)
        assert!(
            im.is_enabled(&m0, 0),
            "weight={w}: T0 should be enabled at m0"
        );
        assert!(
            !im.is_enabled(&m0, 1),
            "weight={w}: T1 should be disabled at m0"
        );

        // Fire T0: T0 disabled, T1 enabled
        let m1 = fire_transition(&im, &m0, 0).unwrap();
        assert!(
            !im.is_enabled(&m1, 0),
            "weight={w}: T0 should be disabled after firing"
        );
        assert!(
            im.is_enabled(&m1, 1),
            "weight={w}: T1 should be enabled after T0 fires"
        );

        // Fire T1: back to m0
        let m2 = fire_transition(&im, &m1, 1).unwrap();
        assert_eq!(m2, m0, "weight={w}: full cycle returns to initial marking");
    }
}

// ──────────────────────────────────────────────────────────
// Test 6: Conservation law holds at every reachable marking
// ──────────────────────────────────────────────────────────

#[test]
fn conservation_law_holds_through_firing() {
    for &w in &[1.0, 3.0, 7.0] {
        let net = pipeline_net(w);
        let im = IncidenceMatrix::from_petri_net(&net);
        let pinv = p_invariants(&im);
        assert!(!pinv.is_empty());
        let y = &pinv[0];

        let m0 = im.initial_marking(&net);
        let conserved_sum: i64 = m0.iter().zip(y.iter()).map(|(m, y)| m * y).sum();

        // Fire through full cycle: T0, T1, T2
        let mut marking = m0;
        for t in 0..3 {
            marking = fire_transition(&im, &marking, t).unwrap();
            let current_sum: i64 = marking.iter().zip(y.iter()).map(|(m, y)| m * y).sum();
            assert_eq!(
                current_sum, conserved_sum,
                "weight={w}, after firing T{t}: conservation violated"
            );
        }
    }
}

// ──────────────────────────────────────────────────────────
// Test 7: SIR — invariant dimensions match across parameterizations
// ──────────────────────────────────────────────────────────

#[test]
fn sir_invariant_structure_independent_of_population() {
    // SIR with different initial populations — same topology
    for pop in [10.0, 100.0, 1000.0] {
        let net = PetriNet::build().sir(pop, 1.0, 0.0).done();
        let im = IncidenceMatrix::from_petri_net(&net);

        let pinv = p_invariants(&im);
        let tinv = t_invariants(&im);

        // Always 1 P-invariant (total population), 0 T-invariants (SIR is acyclic in R)
        assert_eq!(
            pinv.len(),
            1,
            "pop={pop}: SIR should have 1 P-invariant"
        );
        assert_eq!(
            tinv.len(),
            0,
            "pop={pop}: SIR should have 0 T-invariants (no reproducible cycle)"
        );

        // P-invariant spans all 3 places: I + R + S = const
        let sup = support(&pinv[0]);
        assert_eq!(sup.len(), 3, "pop={pop}: all places conserved");
    }
}

// ──────────────────────────────────────────────────────────
// Test 8: Tropical adjacency structure independent of weights
// ──────────────────────────────────────────────────────────

#[test]
fn tropical_adjacency_connectivity_independent_of_weights() {
    let weights = [1.0, 3.0, 7.0];
    let connectivities: Vec<Vec<bool>> = weights
        .iter()
        .map(|&w| {
            let net = pipeline_net(w);
            let im = IncidenceMatrix::from_petri_net(&net);
            let a = incidence_to_tropical(&im);
            // Extract connectivity (non-NEG_INF entries)
            (0..a.rows * a.cols)
                .map(|idx| a.data[idx] != NEG_INF)
                .collect()
        })
        .collect();

    let reference = &connectivities[0];
    for (i, conn) in connectivities.iter().enumerate().skip(1) {
        assert_eq!(
            conn, reference,
            "weight={}: tropical connectivity differs",
            weights[i]
        );
    }
}
