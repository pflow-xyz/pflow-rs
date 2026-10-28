//! Review sweep for partial_key and forced_moves: positions from seeds and
//! depths the gate tests do not use, compared with the default plan.
mod common;

use pflow_metamodel::{Chain, Filter, SearchPlan};

fn positions(name: &str) -> Vec<Vec<i64>> {
    let (g, m0) = common::game(name);
    match name {
        "tic-tac-toe" => common::sample(&g, &m0, 91, 60, &[0, 1, 2, 3, 4, 5]),
        "connect-3" => common::sample(&g, &m0, 92, 60, &[0, 2, 5, 7, 9, 11]),
        _ => common::sample(&g, &m0, 93, 40, &[18, 19, 21, 23, 25, 27]),
    }
}

#[test]
fn partial_key_and_forced_moves_keep_values_on_fresh_samples() {
    let chain = Chain::default();
    for name in common::NETS {
        let (g, _) = common::game(name);
        let all = positions(name);
        let base = common::fresh(&g, &all);
        let vals = |x: &[(f64, usize)]| x.iter().map(|y| y.0).collect::<Vec<_>>();

        let fm = common::fresh_filtered(&g, &all, &chain, &[Filter::MustWin, Filter::ForcedMoves]);
        let nl = common::fresh_filtered(&g, &all, &chain, &[Filter::MustWin, Filter::NonLosing]);
        assert_eq!(vals(&fm), vals(&base), "{name} forced_moves");
        assert_eq!(fm, nl, "{name} forced_moves nodes");

        for json in [
            r#"{"memo": {"partial_key": true}}"#,
            r#"{"memo": {"entries": 4096, "partial_key": true}}"#,
        ] {
            let p = SearchPlan::from_json(json).unwrap();
            let Ok(_) = p.solver(&g) else { continue };
            // fresh solver each, and one shared solver
            let mut shared = p.solver(&g).unwrap();
            for (mk, want) in all.iter().zip(vals(&base)) {
                assert_eq!(
                    p.solver(&g).unwrap().value(mk).unwrap().unwrap(),
                    want,
                    "{name} {json}"
                );
                assert_eq!(
                    shared.value(mk).unwrap().unwrap(),
                    want,
                    "{name} {json} shared"
                );
            }
        }
    }
}
