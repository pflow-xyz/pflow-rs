//! Conjecture 4: LLM-extracted nets converge with declared nets.
//!
//! Two independent paths to the same Petri net:
//! 1. **Builder (hand-declared)**: programmatic construction via pflow-rs builder API
//! 2. **Petri-pilot (LLM-declared)**: generated from source code by petri-pilot
//!
//! If both converge on the same structural invariants — P-invariants, T-invariants,
//! delta sign patterns, place/transition counts — that's evidence the structure is
//! real, not an artifact of either construction method.

use pflow_tropical::ttt_fixtures::{builder_ttt, builder_ttt_full, pilot_ttt};
use pflow_tropical::{dense_incidence, p_invariants, sign_pattern, support, t_invariants};
use pflow_zk::IncidenceMatrix;

// ──────────────────────────────────────────────────────────
// Test 1: Both nets have identical place/transition/arc counts
// ──────────────────────────────────────────────────────────

#[test]
fn full_builder_matches_pilot_dimensions() {
    let builder = builder_ttt_full();
    let pilot = pilot_ttt();

    let im_b = IncidenceMatrix::from_petri_net(&builder);
    let im_p = IncidenceMatrix::from_petri_net(&pilot);

    assert_eq!(
        im_b.num_places, im_p.num_places,
        "place count: builder={}, pilot={}",
        im_b.num_places, im_p.num_places
    );
    assert_eq!(
        im_b.num_transitions, im_p.num_transitions,
        "transition count: builder={}, pilot={}",
        im_b.num_transitions, im_p.num_transitions
    );
}

// ──────────────────────────────────────────────────────────
// Test 2: Place labels match (canonical ordering)
// ──────────────────────────────────────────────────────────

#[test]
fn place_labels_match() {
    let builder = builder_ttt_full();
    let pilot = pilot_ttt();

    let im_b = IncidenceMatrix::from_petri_net(&builder);
    let im_p = IncidenceMatrix::from_petri_net(&pilot);

    assert_eq!(
        im_b.place_labels, im_p.place_labels,
        "place labels should match"
    );
}

// ──────────────────────────────────────────────────────────
// Test 3: Transition labels match
// ──────────────────────────────────────────────────────────

#[test]
fn transition_labels_match() {
    let builder = builder_ttt_full();
    let pilot = pilot_ttt();

    let im_b = IncidenceMatrix::from_petri_net(&builder);
    let im_p = IncidenceMatrix::from_petri_net(&pilot);

    assert_eq!(
        im_b.transition_labels, im_p.transition_labels,
        "transition labels should match"
    );
}

// ──────────────────────────────────────────────────────────
// Test 4: Incidence matrices are identical
// ──────────────────────────────────────────────────────────

#[test]
fn incidence_matrices_identical() {
    let builder = builder_ttt_full();
    let pilot = pilot_ttt();

    let im_b = IncidenceMatrix::from_petri_net(&builder);
    let im_p = IncidenceMatrix::from_petri_net(&pilot);

    let c_b = dense_incidence(&im_b);
    let c_p = dense_incidence(&im_p);

    assert_eq!(c_b.len(), c_p.len(), "transition count mismatch");

    for t in 0..c_b.len() {
        assert_eq!(
            c_b[t], c_p[t],
            "transition {} ({}) incidence differs:\n  builder: {:?}\n  pilot:   {:?}",
            t, im_b.transition_labels[t], c_b[t], c_p[t]
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 5: Delta sign patterns match for all transitions
// ──────────────────────────────────────────────────────────

#[test]
fn delta_sign_patterns_match() {
    let builder = builder_ttt_full();
    let pilot = pilot_ttt();

    let im_b = IncidenceMatrix::from_petri_net(&builder);
    let im_p = IncidenceMatrix::from_petri_net(&pilot);

    for t in 0..im_b.num_transitions {
        let d_b = im_b.delta(t);
        let d_p = im_p.delta(t);
        let s_b = sign_pattern(&d_b);
        let s_p = sign_pattern(&d_p);
        assert_eq!(
            s_b, s_p,
            "transition {} ({}) sign patterns differ",
            t, im_b.transition_labels[t]
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 6: P-invariant count and support match
// ──────────────────────────────────────────────────────────

#[test]
fn p_invariant_structure_matches() {
    let builder = builder_ttt_full();
    let pilot = pilot_ttt();

    let im_b = IncidenceMatrix::from_petri_net(&builder);
    let im_p = IncidenceMatrix::from_petri_net(&pilot);

    let pinv_b = p_invariants(&im_b);
    let pinv_p = p_invariants(&im_p);

    assert_eq!(
        pinv_b.len(),
        pinv_p.len(),
        "P-invariant count: builder={}, pilot={}",
        pinv_b.len(),
        pinv_p.len()
    );

    // Compare supports (sorted for determinism)
    let mut sup_b: Vec<Vec<usize>> = pinv_b.iter().map(|inv| support(inv)).collect();
    let mut sup_p: Vec<Vec<usize>> = pinv_p.iter().map(|inv| support(inv)).collect();
    sup_b.sort();
    sup_p.sort();

    assert_eq!(sup_b, sup_p, "P-invariant supports should match");
}

// ──────────────────────────────────────────────────────────
// Test 7: T-invariant count matches
// ──────────────────────────────────────────────────────────

#[test]
fn t_invariant_count_matches() {
    let builder = builder_ttt_full();
    let pilot = pilot_ttt();

    let im_b = IncidenceMatrix::from_petri_net(&builder);
    let im_p = IncidenceMatrix::from_petri_net(&pilot);

    let tinv_b = t_invariants(&im_b);
    let tinv_p = t_invariants(&im_p);

    assert_eq!(
        tinv_b.len(),
        tinv_p.len(),
        "T-invariant count: builder={}, pilot={}",
        tinv_b.len(),
        tinv_p.len()
    );
}

// ──────────────────────────────────────────────────────────
// Test 8: Input/output arc structure identical
// ──────────────────────────────────────────────────────────

#[test]
fn arc_structure_identical() {
    let builder = builder_ttt_full();
    let pilot = pilot_ttt();

    let im_b = IncidenceMatrix::from_petri_net(&builder);
    let im_p = IncidenceMatrix::from_petri_net(&pilot);

    for t in 0..im_b.num_transitions {
        assert_eq!(
            im_b.inputs[t], im_p.inputs[t],
            "transition {} ({}) input arcs differ",
            t, im_b.transition_labels[t]
        );
        assert_eq!(
            im_b.outputs[t], im_p.outputs[t],
            "transition {} ({}) output arcs differ",
            t, im_b.transition_labels[t]
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 9: Basic builder (no turn/win) is a substructure
// ──────────────────────────────────────────────────────────

#[test]
fn basic_builder_is_substructure_of_pilot() {
    // The simple builder net (27 places, 18 transitions) should be
    // a structural subgraph of the full pilot net (33 places, 35 transitions).
    // Every play transition's core arcs (cell -> transition -> history) should
    // appear in both.
    let basic = builder_ttt();
    let pilot = pilot_ttt();

    let im_basic = IncidenceMatrix::from_petri_net(&basic);
    let im_pilot = IncidenceMatrix::from_petri_net(&pilot);

    // Basic net places should be a subset of pilot places
    for label in &im_basic.place_labels {
        assert!(
            im_pilot.place_labels.contains(label),
            "basic place '{}' missing from pilot",
            label
        );
    }

    // Basic net transitions should be a subset of pilot transitions
    for label in &im_basic.transition_labels {
        assert!(
            im_pilot.transition_labels.contains(label),
            "basic transition '{}' missing from pilot",
            label
        );
    }

    // For each basic transition, verify core arcs appear in pilot
    for (t_idx, label) in im_basic.transition_labels.iter().enumerate() {
        let pilot_t_idx = im_pilot
            .transition_labels
            .iter()
            .position(|l| l == label)
            .unwrap();

        // Basic transition's input places should appear in pilot's inputs
        for &(basic_p, basic_w) in &im_basic.inputs[t_idx] {
            let basic_place = &im_basic.place_labels[basic_p];
            let pilot_p = im_pilot
                .place_labels
                .iter()
                .position(|l| l == basic_place)
                .unwrap();
            assert!(
                im_pilot.inputs[pilot_t_idx]
                    .iter()
                    .any(|&(p, w)| p == pilot_p && w == basic_w),
                "transition '{}': input arc from '{}' (w={}) missing in pilot",
                label,
                basic_place,
                basic_w
            );
        }

        // Same for outputs
        for &(basic_p, basic_w) in &im_basic.outputs[t_idx] {
            let basic_place = &im_basic.place_labels[basic_p];
            let pilot_p = im_pilot
                .place_labels
                .iter()
                .position(|l| l == basic_place)
                .unwrap();
            assert!(
                im_pilot.outputs[pilot_t_idx]
                    .iter()
                    .any(|&(p, w)| p == pilot_p && w == basic_w),
                "transition '{}': output arc to '{}' (w={}) missing in pilot",
                label,
                basic_place,
                basic_w
            );
        }
    }
}

// ──────────────────────────────────────────────────────────
// Test 10: Conservation laws — cell places conserved
// ──────────────────────────────────────────────────────────

#[test]
fn cell_conservation_in_both_nets() {
    // In both nets, each cell's token is conserved:
    // p{i}{j} + x{i}{j} + o{i}{j} = 1 for all (i,j)
    let builder = builder_ttt_full();
    let pilot = pilot_ttt();

    for (name, net) in [("builder", &builder), ("pilot", &pilot)] {
        let im = IncidenceMatrix::from_petri_net(net);
        let c = dense_incidence(&im);

        for i in 0..3 {
            for j in 0..3 {
                let p_idx = im
                    .place_labels
                    .iter()
                    .position(|l| l == &format!("p{}{}", i, j))
                    .unwrap();
                let x_idx = im
                    .place_labels
                    .iter()
                    .position(|l| l == &format!("x{}{}", i, j))
                    .unwrap();
                let o_idx = im
                    .place_labels
                    .iter()
                    .position(|l| l == &format!("o{}{}", i, j))
                    .unwrap();

                // For every transition, delta[p] + delta[x] + delta[o] = 0
                for t in 0..im.num_transitions {
                    let sum = c[t][p_idx] + c[t][x_idx] + c[t][o_idx];
                    assert_eq!(
                        sum, 0,
                        "{}: cell ({},{}), transition {} ({}): conservation violated (sum={})",
                        name, i, j, t, im.transition_labels[t], sum
                    );
                }
            }
        }
    }
}
