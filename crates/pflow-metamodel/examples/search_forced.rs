//! `forced_moves` against `non_losing` and the default: nodes and time.
//! `cargo run --release -p pflow-metamodel --example search_forced`.
#[path = "../tests/common/mod.rs"]
mod common;

use pflow_metamodel::{Chain, Filter};
use std::time::Instant;

fn main() {
    let sets: [(&str, Vec<Filter>); 4] = [
        ("default (must_win)", vec![Filter::MustWin]),
        (
            "must_win, non_losing",
            vec![Filter::MustWin, Filter::NonLosing],
        ),
        (
            "must_win, forced_moves",
            vec![Filter::MustWin, Filter::ForcedMoves],
        ),
        ("no filters", vec![]),
    ];
    for name in common::NETS {
        let (g, m0) = common::game(name);
        let all = common::positions(name, &g, &m0);
        println!("{name}: {} positions", all.len());
        for (label, f) in &sets {
            let best = (0..3)
                .map(|_| {
                    let t = Instant::now();
                    let r = common::fresh_filtered(&g, &all, &Chain::default(), f);
                    (
                        t.elapsed().as_secs_f64(),
                        r.iter().map(|x| x.1).sum::<usize>(),
                    )
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap();
            println!(
                "  {label:24} nodes {:>9}, {:.3}s (best of 3)",
                best.1, best.0
            );
        }
    }
}
