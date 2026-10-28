//! Opening books and play mode on Connect-4, release build:
//! `cargo run --release -p pflow-solver --example search_play [labels.jsonl]`.
//!
//! 1. With a label cache (pflow-native's, e.g. its
//!    `docs/connect4-search/referee-labels.jsonl`): what it seeds, and the
//!    nodes of labelling its positions again (each move's value) with and
//!    without it, and of the 200-position sample.
//! 2. Play mode: on the 200-position sample, how often `choose` picks a best
//!    move under a node budget, by the default plan and with the `ode` root
//!    (then `non_losing`), and how often it proved it.
#[path = "../../pflow-metamodel/tests/common/mod.rs"]
mod common;

use pflow_metamodel::game::{Game, Solver};
use pflow_metamodel::{Book, SearchPlan};
use pflow_solver::game;
use std::time::Instant;

/// Nodes of valuing every move at each of `all`, by one solver.
fn label(g: &Game, s: &mut Solver, all: &[Vec<i64>]) -> usize {
    let mut nodes = 0;
    for mk in all {
        for t in g.moves(mk) {
            s.value(&g.play(mk, t).unwrap())
                .unwrap()
                .expect("within the budget");
            nodes += s.nodes();
        }
    }
    nodes
}

fn main() {
    let (g, m0) = common::game("connect-4");
    let mut wide = common::sample(&g, &m0, 5, 150, &common::CONNECT4_PLIES);
    wide.extend(common::sample(&g, &m0, 5, 50, &[16, 17, 18]));

    if let Some(path) = std::env::args().nth(1) {
        let text = std::fs::read_to_string(&path).unwrap();
        let t = Instant::now();
        let book = Book::from_labels(&g, &text).unwrap();
        let read = t.elapsed().as_secs_f64() * 1e3;
        let lines = text.lines().count() - 1;
        let exact = text.lines().skip(1).filter(|l| !l.contains("null")).count();
        let t = Instant::now();
        let s = Solver::new(&g, usize::MAX).with_book(&book);
        let seeded = s.memo_len();
        println!(
            "{path}: {lines} positions, {exact} exact: {} book entries ({seeded} distinct), read \
             in {read:.0} ms, seeded in {:.1} ms",
            book.len(),
            t.elapsed().as_secs_f64() * 1e3
        );
        let all: Vec<Vec<i64>> = text
            .lines()
            .skip(1)
            .filter(|l| !l.contains("null"))
            .map(|l| {
                let x: serde_json::Value = serde_json::from_str(l).unwrap();
                serde_json::from_value(x["mk"].clone()).unwrap()
            })
            .collect();
        let few = &all[..all.len().min(200)];
        let t = Instant::now();
        let plain = label(&g, &mut Solver::new(&g, usize::MAX), few);
        let pt = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let booked = label(&g, &mut Solver::new(&g, usize::MAX).with_book(&book), few);
        let bt = t.elapsed().as_secs_f64();
        println!(
            "labelling {} of its positions again: {plain} nodes ({pt:.2} s) without the book, \
             {booked} ({bt:.3} s) with it",
            few.len()
        );
        let (mut plain, mut booked) = (0, 0);
        for mk in &wide {
            let mut s = Solver::new(&g, usize::MAX);
            let v = s.value(mk).unwrap();
            plain += s.nodes();
            let mut s = Solver::new(&g, usize::MAX).with_book(&book);
            assert_eq!(s.value(mk).unwrap(), v);
            booked += s.nodes();
        }
        println!("the 200-position sample: {plain} nodes without the book, {booked} with it\n");
    }

    // 2. Play mode.
    let mut exact = Solver::new(&g, usize::MAX);
    let want: Vec<f64> = wide
        .iter()
        .map(|mk| exact.value(mk).unwrap().unwrap())
        .collect();
    println!(
        "{:<26} {:>8} {:>10} {:>8} {:>9}",
        "play mode, 200 positions", "budget", "best move", "proved", "time"
    );
    for root in [
        "",
        r#", "root": {"order": ["ode"]}"#,
        r#", "root": {"order": ["ode", "non_losing"]}"#,
    ] {
        for budget in [100, 1_000, 10_000, 100_000] {
            let p = SearchPlan::from_json(&format!(
                r#"{{"mode": "play", "drivers": ["null_window", {{"budget": {{"nodes": {budget}}}}}]{root}}}"#
            ))
            .unwrap();
            let t = Instant::now();
            let picks: Vec<_> = wide
                .iter()
                .map(|mk| game::solver(&p, &g).unwrap().choose(mk).unwrap().unwrap())
                .collect();
            let secs = t.elapsed().as_secs_f64();
            let proved = picks.iter().filter(|c| c.exact).count();
            let best = wide
                .iter()
                .zip(&want)
                .zip(&picks)
                .filter(|((mk, &v), c)| exact.value(&g.play(mk, c.t).unwrap()).unwrap() == Some(v))
                .count();
            let label = match root {
                "" => "default",
                r if r.contains("non_losing") => "ode, non_losing",
                _ => "ode",
            };
            println!("{label:<26} {budget:>8} {best:>10} {proved:>8} {secs:>8.2}s");
        }
    }
}
