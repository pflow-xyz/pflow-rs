//! The search gate: every exact value and node count of the default search
//! (its order chain, its filters, its bounded table), on tic-tac-toe (all
//! positions) and a seeded sample of Connect-3 and Connect-4. The values are
//! the ones the search found before its order became a chain of pieces; the
//! node counts last changed when `must_win` became a default filter.
//! Regenerate with the `search_gate` example only when the search itself is
//! meant to change, and never when a value would.
mod common;

use pflow_metamodel::game::Solver;
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
fn filters_find_the_same_values_under_any_order() {
    use pflow_metamodel::Filter;
    for name in ["tic-tac-toe", "connect-3"] {
        let (g, m0) = common::game(name);
        let mut all = common::positions(name, &g, &m0);
        all.truncate(400);
        let want: Vec<f64> = common::fresh(&g, &all).iter().map(|x| x.0).collect();
        for filters in [
            vec![Filter::MustWin],
            vec![Filter::NonLosing],
            vec![Filter::MustWin, Filter::NonLosing],
        ] {
            for chain in chains().iter().step_by(4) {
                let got: Vec<f64> = common::fresh_filtered(&g, &all, chain, &filters)
                    .iter()
                    .map(|x| x.0)
                    .collect();
                assert_eq!(got, want, "{name} by {:?} {filters:?}", chain.pieces());
            }
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
    let g = common::goalless();
    assert!(g.goals.is_empty());
    for name in ["centrality", "threats"] {
        let chain = Chain::new(&[Piece::parse(name).unwrap()]).unwrap();
        let e = Solver::with_chain(&g, 100, chain)
            .err()
            .expect("refused")
            .to_string();
        assert!(e.contains(name) && e.contains("goals"), "{e}");
    }
    // The default chain needs none.
    assert!(Solver::with_chain(&g, 100, Chain::default()).is_ok());
}
