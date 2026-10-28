//! The `ode` root estimate: the compiled continuous net solves as the
//! pflow-core net does, a search ordered by it at the root finds the default
//! search's values, and `non_losing` after it catches the must-blocks it
//! misses.
#[path = "../../pflow-metamodel/tests/common/mod.rs"]
mod common;

use pflow_metamodel::game::Game;
use pflow_metamodel::SearchPlan;
use pflow_solver::game::{self, OdeEstimate};
use pflow_solver::{methods, solve, Problem};

#[test]
fn the_compiled_net_solves_as_the_core_net_does() {
    for name in common::NETS {
        let (g, m0) = common::game(name);
        let est = OdeEstimate::new(&g);
        for mk in common::sample(&g, &m0, 9, 4, &[0, 3, 6]) {
            let (u, steps) = est.final_state(&mk);
            let (net, state) = OdeEstimate::petri_net(&g, &mk);
            let rates = net.set_rates(None);
            let prob = Problem::new(net, state, [0.0, OdeEstimate::HORIZON], rates);
            let sol = solve(&prob, &methods::tsit5(), est.solve_options());
            assert_eq!(sol.t.len() - 1, steps, "{name}");
            let end = sol.get_final_state().unwrap();
            for (p, id) in g.ix.places.iter().enumerate() {
                assert!(
                    (u[p] - end[id]).abs() < 1e-9,
                    "{name} {id}: {} vs {}",
                    u[p],
                    end[id]
                );
            }
        }
    }
}

#[test]
fn an_ode_root_keeps_the_default_values() {
    let plans = [
        r#"{"root": {"order": ["ode"]}}"#,
        r#"{"root": {"order": ["ode", "non_losing"]}, "drivers": ["null_window", "aspiration"]}"#,
        r#"{"root": {"order": ["ode"]}, "drivers": ["aspiration"]}"#,
    ];
    for name in ["tic-tac-toe", "connect-3"] {
        let (g, m0) = common::game(name);
        let mut all = common::positions(name, &g, &m0);
        all.truncate(600);
        let want: Vec<f64> = common::fresh(&g, &all).iter().map(|x| x.0).collect();
        for json in plans {
            let p = SearchPlan::from_json(json).unwrap();
            // One solver for every position: its table carries over.
            let mut s = game::solver(&p, &g).unwrap();
            let got: Vec<f64> = all.iter().map(|mk| s.value(mk).unwrap().unwrap()).collect();
            assert_eq!(got, want, "{name} by {json}");
        }
    }
}

/// Whether after move `t` at `mk` the other side has a reply that wins at
/// once.
fn gives_a_win(g: &Game, mk: &[i64], t: usize) -> bool {
    let next = g.play(mk, t).unwrap();
    let s = g.sign[t];
    let worst = if s > 0.0 { g.bounds.0 } else { g.bounds.1 };
    g.moves(&next).into_iter().any(|u| {
        let after = g.play(&next, u).unwrap();
        g.moves(&after).is_empty() && g.score(&after).unwrap() == worst
    })
}

/// On tic-tac-toe the ODE's first choice sometimes leaves the other side a
/// win it could have blocked; with `non_losing` after it, never.
#[test]
fn non_losing_catches_the_must_blocks_the_ode_misses() {
    let (g, m0) = common::game("tic-tac-toe");
    let plan = |json: &str| SearchPlan::from_json(json).unwrap();
    let mut ode = game::solver(&plan(r#"{"root": {"order": ["ode"]}}"#), &g).unwrap();
    let mut refined =
        game::solver(&plan(r#"{"root": {"order": ["ode", "non_losing"]}}"#), &g).unwrap();
    let (mut missed, mut still) = (0, 0);
    for mk in common::reachable(&g, &m0) {
        let moves = g.moves(&mk);
        if moves.is_empty() || moves.iter().all(|&t| gives_a_win(&g, &mk, t)) {
            continue;
        }
        missed += gives_a_win(&g, &mk, ode.root_order(&mk).unwrap()[0]) as usize;
        still += gives_a_win(&g, &mk, refined.root_order(&mk).unwrap()[0]) as usize;
    }
    assert!(
        missed > 0,
        "the ODE alone blocked every win: nothing to refine"
    );
    assert_eq!(
        still, 0,
        "non_losing kept a losing first move ({missed} without it)"
    );
}
