//! `partial_key` against the whole-key table: bytes per entry, and the
//! nodes and time of the same searches. `cargo run --release -p
//! pflow-metamodel --example search_partial [entries]`.
//!
//! Peak memory of one big search, each in a process of its own so the
//! high-water mark is its alone (read it with `/usr/bin/time -l`):
//! `search_partial one whole|partial [ply] [budget] [entries]`.
#[path = "../tests/common/mod.rs"]
mod common;

use pflow_metamodel::game::Solver;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("one") {
        return one(&args[1..]);
    }
    let entries: usize = args.first().map_or(Solver::MEMO, |a| a.parse().unwrap());
    for name in common::NETS {
        let (g, m0) = common::game(name);
        let all = common::positions(name, &g, &m0);
        let check = Solver::partial_key_check(&g, entries);
        println!(
            "{name}: {} places, {} positions, partial_key: {}",
            g.ix.places.len(),
            all.len(),
            check
                .as_ref()
                .map_or_else(|e| e.to_string(), |_| "ok".into())
        );
        let (whole, wmost) = Solver::new(&g, 1).with_memo(entries).memo_bytes();
        print!(
            "  an entry {whole} bytes whole, {} MiB at most",
            wmost >> 20
        );
        if check.is_ok() {
            let (part, pmost) = Solver::new(&g, 1)
                .with_memo(entries)
                .with_partial_key(true)
                .memo_bytes();
            println!(
                "; {part} bytes partial ({:+.1}%), {} MiB at most",
                100.0 * (part as f64 / whole as f64 - 1.0),
                pmost >> 20
            );
        } else {
            println!();
        }
        for partial in [false, true] {
            if partial && check.is_err() {
                continue;
            }
            let t = Instant::now();
            let (mut nodes, mut held) = (0, 0);
            for mk in &all {
                let mut s = Solver::new(&g, common::BUDGET)
                    .with_memo(entries)
                    .with_partial_key(partial);
                s.value(mk).unwrap().unwrap();
                nodes += s.nodes();
                held += s.memo_len();
            }
            println!(
                "  {:7} nodes {nodes}, entries held {held}, {:.3}s",
                if partial { "partial" } else { "whole" },
                t.elapsed().as_secs_f64()
            );
        }
    }
}

/// One Connect-4 search under a node budget, for `/usr/bin/time -l`.
fn one(args: &[String]) {
    let partial = args[0] == "partial";
    let num = |i: usize, d: usize| args.get(i).map_or(d, |a| a.parse().unwrap());
    let (ply, budget, entries) = (num(1, 10), num(2, 30_000_000), num(3, Solver::MEMO));
    let (g, m0) = common::game("connect-4");
    let mk = &common::sample(&g, &m0, 7, 1, &[ply])[0];
    let t = Instant::now();
    let mut s = Solver::new(&g, budget)
        .with_memo(entries)
        .with_partial_key(partial);
    let v = s.value(mk).unwrap();
    println!(
        "{} ply {ply}: nodes {} value {v:?} entries held {} {:.1}s",
        args[0],
        s.nodes(),
        s.memo_len(),
        t.elapsed().as_secs_f64()
    );
}
