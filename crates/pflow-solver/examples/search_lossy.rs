//! `lmr` and `futility` in play mode on the 200-position Connect-4 sample,
//! release build: `cargo run --release -p pflow-solver --example search_lossy`.
//!
//! For each plan and node budget (the whole `choose` call's): how many
//! positions got a value within the budget, how many of those values are the
//! exact one, how often `choose` picked a best move, the nodes searched, and
//! the time. A plan with no lossy piece is the baseline.
#[path = "../../pflow-metamodel/tests/common/mod.rs"]
mod common;

use pflow_metamodel::game::Solver;
use pflow_metamodel::SearchPlan;
use pflow_solver::game;
use std::time::Instant;

fn main() {
    let (g, m0) = common::game("connect-4");
    let mut wide = common::sample(&g, &m0, 5, 150, &common::CONNECT4_PLIES);
    wide.extend(common::sample(&g, &m0, 5, 50, &[16, 17, 18]));
    let mut exact = Solver::new(&g, usize::MAX);
    let want: Vec<f64> = wide
        .iter()
        .map(|mk| exact.value(mk).unwrap().unwrap())
        .collect();
    let plans: [(&str, &str); 7] = [
        ("default", ""),
        ("lmr after 3, 64 nodes", r#", "heuristics": ["lmr"]"#),
        (
            "lmr after 2, 16 nodes",
            r#", "heuristics": ["lmr"], "tuning": {"lmr": {"after": 2, "nodes": 16}}"#,
        ),
        (
            "lmr after 4, 1000 nodes",
            r#", "heuristics": ["lmr"], "tuning": {"lmr": {"after": 4, "nodes": 1000}}"#,
        ),
        ("root ode", r#", "root": {"order": ["ode"]}"#),
        (
            "ode, futility 1/8",
            r#", "filters": ["must_win", "futility"], "root": {"order": ["ode"]}"#,
        ),
        (
            "ode, futility 0.5",
            r#", "filters": ["must_win", "futility"], "root": {"order": ["ode"]},
               "tuning": {"futility": {"margin": 0.5}}"#,
        ),
    ];
    println!(
        "{:<26} {:>8} {:>6} {:>6} {:>6} {:>11} {:>8}",
        "play mode, 200 positions", "budget", "valued", "exact", "best", "nodes", "time"
    );
    for (label, extra) in plans {
        for budget in [1_000, 10_000, 100_000] {
            let p = SearchPlan::from_json(&format!(
                r#"{{"mode": "play", "drivers": ["null_window", {{"budget": {{"nodes": {budget}}}}}]{extra}}}"#
            ))
            .unwrap();
            let t = Instant::now();
            let (mut nodes, mut valued, mut right, mut picks) = (0, 0, 0, vec![]);
            for (mk, &v) in wide.iter().zip(&want) {
                let mut s = game::solver(&p, &g).unwrap();
                let c = s.choose(mk).unwrap().unwrap();
                nodes += s.nodes();
                if let Some(x) = c.value {
                    valued += 1;
                    right += (x == v) as usize;
                }
                picks.push(c.t);
            }
            let secs = t.elapsed().as_secs_f64();
            let best = wide
                .iter()
                .zip(&want)
                .zip(&picks)
                .filter(|((mk, &v), &t)| exact.value(&g.play(mk, t).unwrap()).unwrap() == Some(v))
                .count();
            println!(
                "{label:<26} {budget:>8} {valued:>6} {right:>6} {best:>6} {nodes:>11} {secs:>7.2}s"
            );
        }
    }
}
