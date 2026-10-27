//! Positions per second of the game search on the Connect-4 sample, release
//! build: `cargo run --release -p pflow-metamodel --example search_bench`.
#[path = "../tests/common/mod.rs"]
mod common;

use std::time::Instant;

fn main() {
    let (g, m0) = common::game("connect-4");
    let all = common::positions("connect-4", &g, &m0);
    let mut runs = vec![];
    for _ in 0..3 {
        let t = Instant::now();
        let got = common::fresh(&g, &all);
        runs.push((t.elapsed().as_secs_f64(), got));
    }
    runs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (secs, got) = &runs[1];
    let nodes: usize = got.iter().map(|x| x.1).sum();
    println!(
        "baseline: {} positions, {nodes} nodes, {secs:.3}s median of 3, {:.2} positions/s",
        all.len(),
        all.len() as f64 / secs
    );
}
