//! The `partial_key` memo layout: an entry keeps only the part of its marking
//! that its bucket does not say. It cannot change a value, so the gate is
//! the default's values on the golden nets, on tables small enough to evict
//! and double, and across counts keys; and a legality check that refuses the
//! nets and sizes it cannot hold. Its bytes per entry and peak memory are
//! reported by the `search_partial` example and recorded in
//! `docs/search-pieces-roadmap.md`.
mod common;

use pflow_metamodel::game::Solver;
use pflow_metamodel::SearchPlan;

fn plan(json: &str) -> SearchPlan {
    SearchPlan::from_json(json).unwrap()
}

#[test]
fn partial_key_finds_the_default_values_on_the_golden_nets() {
    for name in common::NETS {
        let (g, m0) = common::game(name);
        let all = common::positions(name, &g, &m0);
        let want: Vec<f64> = common::fresh(&g, &all).iter().map(|x| x.0).collect();
        let p = plan(r#"{"memo": {"partial_key": true}}"#);
        let got: Vec<f64> = all
            .iter()
            .map(|mk| p.solver(&g).unwrap().value(mk).unwrap().unwrap())
            .collect();
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn partial_key_keeps_the_values_in_small_tables_that_evict_and_double() {
    let (g, m0) = common::game("tic-tac-toe");
    let all = common::reachable(&g, &m0);
    let want: Vec<f64> = common::fresh(&g, &all).iter().map(|x| x.0).collect();
    for entries in [4, 16, 100, 1000, 5000, 1 << 16] {
        let p = plan(&format!(
            r#"{{"memo": {{"entries": {entries}, "partial_key": true}}}}"#
        ));
        // A fresh solver each, then one solver for every position, twice.
        let fresh: Vec<f64> = all
            .iter()
            .map(|mk| p.solver(&g).unwrap().value(mk).unwrap().unwrap())
            .collect();
        assert_eq!(fresh, want, "{entries} entries, fresh");
        let mut s = p.solver(&g).unwrap();
        for _ in 0..2 {
            let shared: Vec<f64> = all.iter().map(|mk| s.value(mk).unwrap().unwrap()).collect();
            assert_eq!(shared, want, "{entries} entries, shared");
        }
        assert!(s.memo_len() <= entries);
    }
}

#[test]
fn partial_key_keeps_the_values_when_the_search_goes_by_counts() {
    let (g, m0) = common::piling_prizes(&[1, 1, 1, 1, 2]);
    let all = common::reachable(&g, &m0);
    assert!(all.len() > 40);
    // The net does not say it leaves 0/1 markings, and the search does.
    let p = plan(r#"{"memo": {"entries": 4000, "partial_key": true}}"#);
    p.solver(&g).unwrap();
    let want: Vec<f64> = all
        .iter()
        .map(|mk| Solver::new(&g, usize::MAX).value(mk).unwrap().unwrap())
        .collect();
    let mut s = p.solver(&g).unwrap();
    let got: Vec<f64> = all.iter().map(|mk| s.value(mk).unwrap().unwrap()).collect();
    assert_eq!(got, want);
    assert!(s.memo_len() > 0);
}

#[test]
fn partial_key_is_refused_where_the_net_or_the_size_cannot_hold_it() {
    let refused = |g: &_, json: &str| {
        plan(json)
            .solver(g)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_else(|| panic!("{json} was allowed"))
    };
    // 2^23 entries on Connect-4: 131 places, three words become two.
    let (c4, _) = common::game("connect-4");
    assert!(plan(r#"{"memo": {"partial_key": true}}"#)
        .solver(&c4)
        .is_ok());
    // Too small a table saves nothing on Connect-4 (123 bits of q).
    let e = refused(&c4, r#"{"memo": {"entries": 4, "partial_key": true}}"#);
    assert!(e.contains("saves nothing"), "{e}");
    // Too many places for the bit engine.
    let (wide, _) = common::prizes(&[1; 100]);
    let e = refused(&wide, r#"{"memo": {"partial_key": true}}"#);
    assert!(e.contains("bit engine"), "{e}");
    // A net that changes a place by two tokens.
    let e = refused(&double(), r#"{"memo": {"partial_key": true}}"#);
    assert!(e.contains("0/1"), "{e}");
    // The check needs no plan.
    assert!(Solver::partial_key_check(&c4, Solver::MEMO).is_ok());
    // Every default plan is untouched by it.
    assert!(!plan("{}").normalized().unwrap().memo.unwrap().partial_key);
}

/// A net with a transition that puts two tokens on a place.
fn double() -> pflow_metamodel::game::Game {
    use pflow_metamodel::{Arc, Model, Place, Transition};
    let place = |id: &str, initial| Place {
        id: id.into(),
        initial,
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
        arcs: vec![
            Arc {
                from: "a".into(),
                to: "x_go".into(),
                ..Default::default()
            },
            Arc {
                from: "x_go".into(),
                to: "b".into(),
                weight: 2,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let ix = m.indexed();
    let won = ix.places.iter().position(|p| p == "won").unwrap();
    pflow_metamodel::game::Game::new(ix, vec![false; 2], vec![1.0, -1.0], vec![(won, 1.0)])
}
