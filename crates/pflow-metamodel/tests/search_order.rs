//! The move-order gate: every exact value and node count of the default
//! search, on tic-tac-toe (all positions) and a seeded sample of Connect-3
//! and Connect-4, as they were before ordering became a chain of pieces.
//! Regenerate with the `search_gate` example only when the search itself is
//! meant to change.
mod common;

use pflow_metamodel::order::{Chain, Piece};

#[test]
fn default_order_matches_the_golden_values_and_node_counts() {
    let want = include_str!("golden/search-gate.txt");
    let got = common::gate_lines();
    assert_eq!(got.len(), want.lines().count(), "positions");
    for (g, w) in got.iter().zip(want.lines()) {
        assert_eq!(g, w);
    }
}

/// Every chain of some of the pieces, in every order, including none.
fn chains() -> Vec<Chain> {
    let all = Piece::NAMES.map(|n| Piece::parse(n).unwrap());
    let mut out = vec![vec![]];
    for p in all {
        let more: Vec<Vec<Piece>> = out
            .iter()
            .flat_map(|c| {
                (0..=c.len()).map(move |i| {
                    let mut c = c.clone();
                    c.insert(i, p);
                    c
                })
            })
            .collect();
        out.extend(more);
    }
    out.iter().map(|c| Chain::new(c).unwrap()).collect()
}

#[test]
fn any_order_finds_the_same_values() {
    for name in ["tic-tac-toe", "connect-3"] {
        let (g, m0) = common::game(name);
        let mut all = common::positions(name, &g, &m0);
        all.truncate(400);
        let want: Vec<f64> = common::fresh(&g, &all).iter().map(|x| x.0).collect();
        let chains = chains();
        assert!(chains.len() > 10);
        for chain in &chains {
            let got: Vec<f64> = common::fresh_with(&g, &all, chain)
                .iter()
                .map(|x| x.0)
                .collect();
            assert_eq!(got, want, "{name} by {:?}", chain.pieces());
        }
    }
}
