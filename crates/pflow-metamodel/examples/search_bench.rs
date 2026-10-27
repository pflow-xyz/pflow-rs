//! Nodes and positions per second of the game search on the Connect-4
//! sample, one line per move-order chain, release build:
//! `cargo run --release -p pflow-metamodel --example search_bench`.
//! Run it under a memory cap (`ulimit -v`): a search that outgrows its node
//! budget is cut off, but the table behind it is not bounded by it.
#[path = "../tests/common/mod.rs"]
mod common;

use pflow_metamodel::order::{Chain, Piece};
use std::time::Instant;

fn main() {
    let (g, m0) = common::game("connect-4");
    let all = common::positions("connect-4", &g, &m0);
    let [tt, win, hist, cen, thr] = Piece::NAMES.map(|n| Piece::parse(n).unwrap());
    let variants = [
        ("baseline", vec![tt, win, hist]),
        ("+centrality", vec![tt, win, cen, hist]),
        ("+threats", vec![tt, win, thr, hist]),
        ("+threats+centrality", vec![tt, win, thr, cen, hist]),
        ("+centrality+threats", vec![tt, win, cen, thr, hist]),
    ];
    let mut values: Option<Vec<f64>> = None;
    for (name, pieces) in variants {
        let chain = Chain::new(&pieces).unwrap();
        let mut runs = vec![];
        for _ in 0..3 {
            let t = Instant::now();
            let got = common::fresh_with(&g, &all, &chain);
            runs.push((t.elapsed().as_secs_f64(), got));
        }
        runs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (secs, got) = &runs[1];
        let vals: Vec<f64> = got.iter().map(|x| x.0).collect();
        assert_eq!(*values.get_or_insert_with(|| vals.clone()), vals, "{name}");
        let nodes: usize = got.iter().map(|x| x.1).sum();
        println!(
            "{name}: {} positions, {nodes} nodes, {secs:.3}s median of 3, {:.2} positions/s",
            all.len(),
            all.len() as f64 / secs
        );
    }
}
