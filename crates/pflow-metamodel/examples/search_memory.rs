//! Peak table size on hard Connect-4 positions: early positions of seeded
//! play, each searched under a node budget, one fresh solver each. Run it
//! under a memory cap; prints `ply nodes value` per position.
//! `cargo run --release -p pflow-metamodel --example search_memory [ply] [budget] [entries] [count]`.
#[path = "../tests/common/mod.rs"]
mod common;

use pflow_metamodel::game::Solver;
use std::time::Instant;

fn main() {
    let args: Vec<usize> = std::env::args()
        .skip(1)
        .map(|a| a.parse().unwrap())
        .collect();
    let ply = args.first().copied().unwrap_or(8);
    let budget = args.get(1).copied().unwrap_or(30_000_000);
    let entries = args.get(2).copied().unwrap_or(Solver::MEMO);
    let count = args.get(3).copied().unwrap_or(3);
    let (g, m0) = common::game("connect-4");
    println!(
        "places {} transitions {}",
        g.ix.places.len(),
        g.ix.transitions.len()
    );
    let (entry, most) = Solver::new(&g, 1).with_memo(entries).memo_bytes();
    println!(
        "memo {entries} entries, {entry} bytes each, {} MiB at most",
        most >> 20
    );
    for mk in common::sample(&g, &m0, 7, count, &[ply]) {
        let t = Instant::now();
        let mut s = Solver::new(&g, budget).with_memo(entries);
        let v = s.value(&mk).unwrap();
        println!(
            "ply {ply} nodes {} value {v:?} {:.1}s",
            s.nodes(),
            t.elapsed().as_secs_f64()
        );
    }
}
