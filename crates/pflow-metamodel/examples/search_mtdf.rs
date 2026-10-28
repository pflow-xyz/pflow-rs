//! Node counts of the `mtd_f` driver against `null_window`, on the three
//! fixture games and on prize games with many distinct rewards (at most 16, which `Game` steps
//! through; past that `mtd_f` and `null_window` are the same search):
//! `cargo run --release -p pflow-metamodel --example search_mtdf`.
#[path = "../tests/common/mod.rs"]
mod common;

use pflow_metamodel::game::{Game, Solver};
use pflow_metamodel::SearchPlan;

/// (total nodes, values) of a fresh solver on each position.
fn run(g: &Game, all: &[Vec<i64>], json: &str) -> (usize, Vec<f64>) {
    let p = SearchPlan::from_json(json).unwrap();
    let (mut nodes, mut vals) = (0, vec![]);
    for mk in all {
        let mut s: Solver = p.solver(g).unwrap();
        vals.push(s.value(mk).unwrap().expect("within budget"));
        nodes += s.nodes();
    }
    (nodes, vals)
}

fn report(label: &str, g: &Game, all: &[Vec<i64>]) {
    let (nw, vw) = run(g, all, r#"{"drivers": ["null_window"]}"#);
    let (nm, vm) = run(g, all, r#"{"drivers": ["mtd_f"]}"#);
    assert_eq!(vw, vm, "{label}: values");
    println!(
        "{label:<22} positions {:>5}  null_window {nw:>10}  mtd_f {nm:>10}  ({:+.1}%)",
        all.len(),
        (nm as f64 / nw as f64 - 1.0) * 100.0
    );
}

fn main() {
    for name in common::NETS {
        let (g, m0) = common::game(name);
        let all = common::positions(name, &g, &m0);
        report(name, &g, &all);
    }
    for weights in [
        &[1, 2, 3][..],
        &[1, 1, 2, 3],
        &[1, 1, 1, 1, 2],
        &[1, 1, 1, 1, 1, 1, 1],
    ] {
        let (g, m0) = common::prizes(weights);
        let all = common::reachable(&g, &m0);
        report(&format!("prizes {weights:?}"), &g, &all);
    }
}
