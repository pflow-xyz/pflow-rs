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

use pflow_tropical::{
    eigenvalue, p_invariants_from_dense, sign_pattern, support, t_invariants_from_dense,
    NEG_INF, NetMatrix,
};

/// Build a loop net with given arc weight (all arcs same weight).
fn loop_net(w: i64) -> NetMatrix {
    NetMatrix::from_incidence(
        vec![vec![-w, w], vec![w, -w]],
        vec![w, 0],
        vec!["P0".into(), "P1".into()],
        vec!["T0".into(), "T1".into()],
    )
}

/// Build a 3-place pipeline with given arc weight.
fn pipeline_net(w: i64) -> NetMatrix {
    NetMatrix::from_incidence(
        vec![
            vec![-w, w, 0],
            vec![0, -w, w],
            vec![w, 0, -w],
        ],
        vec![w, 0, 0],
        vec!["A".into(), "B".into(), "C".into()],
        vec!["T0".into(), "T1".into(), "T2".into()],
    )
}

// ──────────────────────────────────────────────────────────
// Test 1: P-invariant support is topological
// ──────────────────────────────────────────────────────────

#[test]
fn p_invariant_support_identical_across_weights() {
    for w in [1, 2, 5, 10] {
        let net = loop_net(w);
        let pinv = p_invariants_from_dense(&net.incidence);

        assert_eq!(pinv.len(), 1, "weight={w}: expected 1 P-invariant, got {}", pinv.len());
        let sup = support(&pinv[0]);
        assert_eq!(sup, vec![0, 1], "weight={w}: P-invariant should span both places");
    }
}

#[test]
fn p_invariant_support_pipeline() {
    for w in [1, 3, 7] {
        let net = pipeline_net(w);
        let pinv = p_invariants_from_dense(&net.incidence);

        assert_eq!(pinv.len(), 1, "weight={w}: pipeline has 1 P-invariant");
        let sup = support(&pinv[0]);
        assert_eq!(sup, vec![0, 1, 2], "weight={w}: all 3 places conserved");
    }
}

// ──────────────────────────────────────────────────────────
// Test 2: T-invariant support is topological
// ──────────────────────────────────────────────────────────

#[test]
fn t_invariant_support_identical_across_weights() {
    for w in [1, 2, 5] {
        let net = loop_net(w);
        let tinv = t_invariants_from_dense(&net.incidence);

        assert_eq!(tinv.len(), 1, "weight={w}: expected 1 T-invariant, got {}", tinv.len());
        let sup = support(&tinv[0]);
        assert_eq!(sup, vec![0, 1], "weight={w}: T-invariant should include both transitions");
    }
}

// ──────────────────────────────────────────────────────────
// Test 3: Delta sign patterns are topological
// ──────────────────────────────────────────────────────────

#[test]
fn delta_sign_pattern_independent_of_weights() {
    let weights = [1, 2, 5, 10];
    let patterns: Vec<Vec<Vec<i8>>> = weights
        .iter()
        .map(|&w| {
            let net = loop_net(w);
            net.incidence.iter().map(|row| sign_pattern(row)).collect()
        })
        .collect();

    let reference = &patterns[0];
    for (i, pattern) in patterns.iter().enumerate().skip(1) {
        assert_eq!(pattern, reference, "weight={}: sign pattern differs", weights[i]);
    }
}

#[test]
fn delta_sign_pattern_pipeline() {
    let weights = [1, 3, 7];
    let patterns: Vec<Vec<Vec<i8>>> = weights
        .iter()
        .map(|&w| {
            let net = pipeline_net(w);
            net.incidence.iter().map(|row| sign_pattern(row)).collect()
        })
        .collect();

    let reference = &patterns[0];
    for (i, pattern) in patterns.iter().enumerate().skip(1) {
        assert_eq!(pattern, reference, "weight={}: pipeline sign pattern differs", weights[i]);
    }
}

// ──────────────────────────────────────────────────────────
// Test 4: Eigenvalue scales with weight (throughput is parametric)
// ──────────────────────────────────────────────────────────

#[test]
fn eigenvalue_scales_linearly_with_weight() {
    for w in [1, 2, 5, 10] {
        let net = loop_net(w);
        let a = net.tropical_adjacency();
        let (lambda, _) = eigenvalue(&a).unwrap();
        let expected = w as f64;
        assert!(
            (lambda - expected).abs() < 1e-10,
            "weight={w}: expected eigenvalue {expected}, got {lambda}"
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 5: Enabledness structure is topological
// ──────────────────────────────────────────────────────────

#[test]
fn enabledness_sequence_identical_across_weights() {
    for w in [1, 3, 7] {
        let net = loop_net(w);
        let m0 = net.initial.clone();

        assert!(net.is_enabled(&m0, 0), "weight={w}: T0 should be enabled at m0");
        assert!(!net.is_enabled(&m0, 1), "weight={w}: T1 should be disabled at m0");

        let m1 = net.fire(&m0, 0).unwrap();
        assert!(!net.is_enabled(&m1, 0), "weight={w}: T0 disabled after firing");
        assert!(net.is_enabled(&m1, 1), "weight={w}: T1 enabled after T0 fires");

        let m2 = net.fire(&m1, 1).unwrap();
        assert_eq!(m2, m0, "weight={w}: full cycle returns to initial marking");
    }
}

// ──────────────────────────────────────────────────────────
// Test 6: Conservation law holds at every reachable marking
// ──────────────────────────────────────────────────────────

#[test]
fn conservation_law_holds_through_firing() {
    for w in [1, 3, 7] {
        let net = pipeline_net(w);
        let pinv = p_invariants_from_dense(&net.incidence);
        assert!(!pinv.is_empty());
        let y = &pinv[0];

        let m0 = net.initial.clone();
        let conserved_sum: i64 = m0.iter().zip(y.iter()).map(|(m, y)| m * y).sum();

        let mut marking = m0;
        for t in 0..3 {
            marking = net.fire(&marking, t).unwrap();
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
    for pop in [10, 100, 1000] {
        // SIR: infect consumes I+S produces 2I, recover consumes I produces R
        // Places: I(0), R(1), S(2)
        // infect is catalytic in I: input=[1,0,1], incidence=[+1,0,-1]
        let net = NetMatrix::new(
            vec![vec![1, 0, -1], vec![-1, 1, 0]],
            vec![vec![1, 0, 1], vec![1, 0, 0]],
            vec![1, 0, pop],
            vec!["I".into(), "R".into(), "S".into()],
            vec!["infect".into(), "recover".into()],
        );

        let pinv = p_invariants_from_dense(&net.incidence);
        let tinv = t_invariants_from_dense(&net.incidence);

        assert_eq!(pinv.len(), 1, "pop={pop}: SIR should have 1 P-invariant");
        assert_eq!(tinv.len(), 0, "pop={pop}: SIR should have 0 T-invariants");

        let sup = support(&pinv[0]);
        assert_eq!(sup.len(), 3, "pop={pop}: all places conserved");
    }
}

// ──────────────────────────────────────────────────────────
// Test 8: Tropical adjacency structure independent of weights
// ──────────────────────────────────────────────────────────

#[test]
fn tropical_adjacency_connectivity_independent_of_weights() {
    let weights = [1, 3, 7];
    let connectivities: Vec<Vec<bool>> = weights
        .iter()
        .map(|&w| {
            let net = pipeline_net(w);
            let a = net.tropical_adjacency();
            (0..a.rows * a.cols)
                .map(|idx| a.data[idx] != NEG_INF)
                .collect()
        })
        .collect();

    let reference = &connectivities[0];
    for (i, conn) in connectivities.iter().enumerate().skip(1) {
        assert_eq!(conn, reference, "weight={}: tropical connectivity differs", weights[i]);
    }
}
