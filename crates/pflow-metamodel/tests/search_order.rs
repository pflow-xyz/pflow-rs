//! The move-order gate: every exact value and node count of the default
//! search, on tic-tac-toe (all positions) and a seeded sample of Connect-3
//! and Connect-4, as they were before ordering became a chain of pieces.
//! Regenerate with the `search_gate` example only when the search itself is
//! meant to change.
mod common;

#[test]
fn default_order_matches_the_golden_values_and_node_counts() {
    let want = include_str!("golden/search-gate.txt");
    let got = common::gate_lines();
    assert_eq!(got.len(), want.lines().count(), "positions");
    for (g, w) in got.iter().zip(want.lines()) {
        assert_eq!(g, w);
    }
}
