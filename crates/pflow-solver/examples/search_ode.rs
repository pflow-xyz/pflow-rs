//! The `ode` root estimate (`pflow_solver::game`) on the fixture nets,
//! release build: `cargo run --release -p pflow-solver --example
//! search_ode [wide]`.
//!
//! 1. What a solve costs on Connect-4, with the continuous net built for
//!    every solve (a pflow-core net and a `Problem`, as pflow-native's ODE
//!    player does) and compiled once per game, and what ranking one root's
//!    moves costs either way.
//! 2. Nodes and time of the search plans the root pieces make, on the
//!    50-position Connect-4 sample (with `wide`, the 200-position one),
//!    each position by a fresh solver, values checked against the default.
//! 3. How often the root's first move is a best one on tic-tac-toe, in the
//!    net's order, by the ODE, and by the ODE then `non_losing`.
#[path = "../../pflow-metamodel/tests/common/mod.rs"]
mod common;

use pflow_metamodel::game::{Game, Solver};
use pflow_metamodel::SearchPlan;
use pflow_solver::game::{self, OdeEstimate};
use pflow_solver::{methods, solve, Problem};
use std::time::Instant;

/// The state `OdeEstimate::HORIZON` ahead of `mk`, the net built anew.
fn uncompiled(g: &Game, mk: &[i64], est: &OdeEstimate) -> usize {
    let (net, state) = OdeEstimate::petri_net(g, mk);
    let rates = net.set_rates(None);
    let prob = Problem::new(net, state, [0.0, OdeEstimate::HORIZON], rates);
    solve(&prob, &methods::tsit5(), est.solve_options()).t.len() - 1
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let wide = std::env::args().any(|a| a == "wide");
    let (g, m0) = common::game("connect-4");
    let all = if wide {
        let mut all = common::sample(&g, &m0, 5, 150, &common::CONNECT4_PLIES);
        all.extend(common::sample(&g, &m0, 5, 50, &[16, 17, 18]));
        all
    } else {
        common::positions("connect-4", &g, &m0)
    };

    // 1. A solve, and a root's ranking.
    let est = OdeEstimate::new(&g);
    let t = Instant::now();
    let steps: usize = all.iter().map(|mk| uncompiled(&g, mk, &est)).sum();
    let before = ms(t) / all.len() as f64;
    let t = Instant::now();
    let steps2: usize = all.iter().map(|mk| est.final_state(mk).1).sum();
    let after = ms(t) / all.len() as f64;
    assert_eq!(steps, steps2);
    println!(
        "ode solve on connect-4 ({} positions, {:.1} steps each): built per solve {before:.3} ms, \
         compiled once {after:.3} ms ({:.1}x)",
        all.len(),
        steps as f64 / all.len() as f64,
        before / after
    );
    let roots: Vec<&Vec<i64>> = all.iter().filter(|mk| g.moves(mk).len() == 7).collect();
    let rank = SearchPlan::from_json(r#"{"root": {"order": ["ode"]}}"#).unwrap();
    let mut s = game::solver(&rank, &g).unwrap();
    let t = Instant::now();
    for mk in &roots {
        s.root_order(mk).unwrap();
    }
    let compiled = ms(t) / roots.len() as f64;
    let t = Instant::now();
    for mk in &roots {
        for u in g.moves(mk) {
            uncompiled(&g, &g.play(mk, u).unwrap(), &est);
        }
    }
    let rebuilt = ms(t) / roots.len() as f64;
    println!(
        "a 7-move root's ranking ({} roots): built per solve {rebuilt:.2} ms, compiled once \
         {compiled:.2} ms",
        roots.len()
    );

    // 2. Plans.
    let centre = r#""order": ["tt_best", "immediate_win", "centrality", "history"]"#;
    let plans = [
        ("default", "{}".to_string()),
        ("root non_losing", r#"{"root": {"order": ["non_losing"]}}"#.into()),
        ("root ode", r#"{"root": {"order": ["ode"]}}"#.into()),
        (
            "root ode, non_losing",
            r#"{"root": {"order": ["ode", "non_losing"]}}"#.into(),
        ),
        (
            "ode + aspiration",
            r#"{"root": {"order": ["ode"]}, "drivers": ["null_window", "aspiration"]}"#.into(),
        ),
        (
            "ode, non_losing + aspiration",
            r#"{"root": {"order": ["ode", "non_losing"]}, "drivers": ["null_window", "aspiration"]}"#
                .into(),
        ),
        ("centrality", format!("{{{centre}}}")),
        (
            "centrality + ode, non_losing",
            format!(r#"{{{centre}, "root": {{"order": ["ode", "non_losing"]}}}}"#),
        ),
        (
            "centrality + ode, non_losing + aspiration",
            format!(
                r#"{{{centre}, "root": {{"order": ["ode", "non_losing"]}}, "drivers": ["null_window", "aspiration"]}}"#
            ),
        ),
    ];
    let mut want: Vec<f64> = vec![];
    let base = std::cell::Cell::new(0usize);
    println!(
        "\n{:<44} {:>11} {:>8} {:>9}",
        format!("connect-4, {} positions", all.len()),
        "nodes",
        "vs dflt",
        "time"
    );
    for (label, json) in &plans {
        let p = SearchPlan::from_json(json).unwrap();
        let t = Instant::now();
        let mut nodes = 0;
        let mut got = vec![];
        for mk in &all {
            let mut s: Solver = game::solver(&p, &g).unwrap();
            got.push(s.value(mk).unwrap().expect("within the node budget"));
            nodes += s.nodes();
        }
        let secs = t.elapsed().as_secs_f64();
        if want.is_empty() {
            want = got.clone();
            base.set(nodes);
        }
        assert_eq!(got, want, "{label} changed a value");
        let d = 100.0 * (nodes as f64 / base.get() as f64 - 1.0);
        println!("{label:<44} {nodes:>11} {d:>+7.1}% {secs:>8.3}s");
    }

    // 3. Tic-tac-toe first moves.
    let (g, m0) = common::game("tic-tac-toe");
    let mut exact = Solver::new(&g, usize::MAX);
    let mut firsts = [0usize; 3];
    let mut count = 0;
    let order = |json: &str| SearchPlan::from_json(json).unwrap();
    let mut ode = game::solver(&order(r#"{"root": {"order": ["ode"]}}"#), &g).unwrap();
    let mut both =
        game::solver(&order(r#"{"root": {"order": ["ode", "non_losing"]}}"#), &g).unwrap();
    for mk in common::reachable(&g, &m0) {
        let moves = g.moves(&mk);
        if moves.is_empty() {
            continue;
        }
        count += 1;
        let v = exact.value(&mk).unwrap().unwrap();
        let firsts_now = [
            moves[0],
            ode.root_order(&mk).unwrap()[0],
            both.root_order(&mk)
                .unwrap()
                .first()
                .copied()
                .unwrap_or(moves[0]),
        ];
        for (n, t) in firsts.iter_mut().zip(firsts_now) {
            *n += (exact.value(&g.play(&mk, t).unwrap()).unwrap().unwrap() == v) as usize;
        }
    }
    println!("\ntic-tac-toe, {count} positions with a move: the first move is a best one");
    for (label, n) in ["net order", "ode", "ode, non_losing"].iter().zip(firsts) {
        println!(
            "  {label:<16} {n:>5} ({:.1}%)",
            100.0 * n as f64 / count as f64
        );
    }
}
