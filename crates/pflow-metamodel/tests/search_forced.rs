//! The `forced_moves` filter: `non_losing`, asked only where a goal of the
//! other side could be in reach. The gate is the default's values on the
//! golden nets, and the very same nodes as `non_losing` (the check is
//! skipped only where it would drop nothing). Its time against `non_losing`
//! is reported by the `search_forced` example and recorded in
//! `docs/search-pieces-roadmap.md`.
mod common;

use pflow_metamodel::{Filter, SearchPlan};

#[test]
fn forced_moves_finds_the_default_values_and_non_losings_nodes() {
    for name in common::NETS {
        let (g, m0) = common::game(name);
        let all = common::positions(name, &g, &m0);
        let chain = pflow_metamodel::Chain::default();
        let base = common::fresh(&g, &all);
        for with in [vec![Filter::MustWin], vec![]] {
            let mut non_losing = with.clone();
            non_losing.push(Filter::NonLosing);
            let mut forced = with.clone();
            forced.push(Filter::ForcedMoves);
            let nl = common::fresh_filtered(&g, &all, &chain, &non_losing);
            let fm = common::fresh_filtered(&g, &all, &chain, &forced);
            let values = |x: &[(f64, usize)]| x.iter().map(|y| y.0).collect::<Vec<_>>();
            assert_eq!(values(&fm), values(&base), "{name} {with:?}");
            assert_eq!(fm, nl, "{name} {with:?}: the same nodes as non_losing");
        }
    }
}

#[test]
fn forced_moves_alone_in_a_plan_and_on_games_by_counts() {
    let p = SearchPlan::from_json(r#"{"filters": ["forced_moves"]}"#).unwrap();
    p.check().unwrap();
    assert_eq!(
        p.normalized().unwrap().filters.unwrap(),
        vec!["forced_moves".to_string()]
    );
    // Games with no goals, and ones searched by counts, are asked as
    // non_losing asks.
    for (g, m0) in [
        common::prizes(&[1, 1, 2]),
        common::piling_prizes(&[1, 1, 2]),
    ] {
        for mk in common::reachable(&g, &m0) {
            let want = common::fresh(&g, std::slice::from_ref(&mk))[0].0;
            let got = p.solver(&g).unwrap().value(&mk).unwrap().unwrap();
            assert_eq!(got, want);
        }
    }
}
