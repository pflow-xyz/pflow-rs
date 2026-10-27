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

/// Every chain of some of the first three pieces, in every order, including
/// none; and some that read the goals.
fn chains() -> Vec<Chain> {
    let all = Piece::NAMES.map(|n| Piece::parse(n).unwrap());
    let mut out = vec![vec![]];
    for p in &all[..3] {
        let p = *p;
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
    let [tt, win, hist, cen, thr] = all;
    out.extend([
        vec![cen],
        vec![thr],
        vec![thr, cen],
        vec![tt, win, cen, hist],
        vec![tt, win, thr, hist],
        vec![tt, win, thr, cen, hist],
        vec![hist, cen, thr, win, tt],
    ]);
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

#[test]
fn centrality_counts_the_lines_through_a_cell() {
    let (g, _) = common::game("tic-tac-toe");
    let at = |name: &str| g.ix.transitions.iter().position(|t| t == name).unwrap();
    // The centre is on four lines, a corner on three, an edge on two; the
    // turn token every detector consumes does not count.
    for s in ["x", "o"] {
        assert_eq!(g.centrality[at(&format!("{s}_play_11"))], 4);
        for corner in ["00", "02", "20", "22"] {
            assert_eq!(g.centrality[at(&format!("{s}_play_{corner}"))], 3);
        }
        for edge in ["01", "10", "12", "21"] {
            assert_eq!(g.centrality[at(&format!("{s}_play_{edge}"))], 2);
        }
    }
}

#[test]
fn goals_are_the_automatic_transitions_paying_a_reward() {
    let (g, _) = common::game("tic-tac-toe");
    // Eight lines for each side.
    assert_eq!(g.goals.len(), 16);
    for goal in &g.goals {
        let name = &g.ix.transitions[goal.t];
        assert_eq!(name.starts_with("x_"), goal.side > 0.0, "{name}");
    }
}

#[test]
fn centrality_and_threats_are_refused_without_goals() {
    // Nim-like: a net whose automatic transitions pay nothing.
    use pflow_metamodel::{Arc, Model, Place, Transition};
    let place = |id: &str, initial| Place {
        id: id.into(),
        initial,
        ..Default::default()
    };
    let arc = |from: &str, to: &str| Arc {
        from: from.into(),
        to: to.into(),
        ..Default::default()
    };
    let m = Model {
        places: vec![place("a", 1), place("b", 0), place("won", 0)],
        transitions: ["x_go", "o_go"]
            .map(|t| Transition {
                id: t.into(),
                ..Default::default()
            })
            .to_vec(),
        arcs: vec![arc("a", "x_go"), arc("x_go", "b"), arc("b", "o_go")],
        ..Default::default()
    };
    let ix = m.indexed();
    let won = ix.places.iter().position(|p| p == "won").unwrap();
    let g = pflow_metamodel::game::Game::new(ix, vec![false; 2], vec![1.0, -1.0], vec![(won, 1.0)]);
    assert!(g.goals.is_empty());
    for name in ["centrality", "threats"] {
        let chain = Chain::new(&[Piece::parse(name).unwrap()]).unwrap();
        let e = pflow_metamodel::game::Solver::with_chain(&g, 100, chain)
            .err()
            .expect("refused")
            .to_string();
        assert!(e.contains(name) && e.contains("goals"), "{e}");
    }
    // The default chain needs none.
    assert!(pflow_metamodel::game::Solver::with_chain(&g, 100, Chain::default()).is_ok());
}
