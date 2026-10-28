//! Nodes and positions per second of the game search on the Connect-4
//! sample, one line per move-order chain and set of filters, release build:
//! `cargo run --release -p pflow-metamodel --example search_bench`. With
//! `wide`, a sample of 200 (150 late positions, 50 from plies 16 to 18)
//! and the places `threats` can take in a chain instead.
//! Run it under a memory cap (`ulimit -v`): a search that outgrows its node
//! budget is cut off, but the table behind it is not bounded by it.
#[path = "../tests/common/mod.rs"]
mod common;

use pflow_metamodel::order::{Chain, Piece};
use pflow_metamodel::Filter;
use std::time::Instant;

fn main() {
    let (g, m0) = common::game("connect-4");
    let wide = std::env::args().any(|a| a == "wide");
    let all = if wide {
        let mut all = common::sample(&g, &m0, 5, 150, &common::CONNECT4_PLIES);
        all.extend(common::sample(&g, &m0, 5, 50, &[16, 17, 18]));
        all
    } else {
        common::positions("connect-4", &g, &m0)
    };
    let [tt, win, hist, cen, thr] = Piece::NAMES.map(|n| Piece::parse(n).unwrap());
    let (must, safe) = (Filter::MustWin, Filter::NonLosing);
    // Every chain under the default filter, then the chains with none and
    // with non_losing as well.
    let variants = [
        ("baseline", vec![tt, win, hist], vec![must]),
        ("+centrality", vec![tt, win, cen, hist], vec![must]),
        ("+threats", vec![tt, win, thr, hist], vec![must]),
        (
            "+threats+centrality",
            vec![tt, win, thr, cen, hist],
            vec![must],
        ),
        (
            "+centrality+threats",
            vec![tt, win, cen, thr, hist],
            vec![must],
        ),
        ("baseline no filter", vec![tt, win, hist], vec![]),
        (
            "baseline +non_losing",
            vec![tt, win, hist],
            vec![must, safe],
        ),
        (
            "+centrality +non_losing",
            vec![tt, win, cen, hist],
            vec![must, safe],
        ),
    ];
    let placements = [
        ("baseline", vec![tt, win, hist], vec![must]),
        ("threats after win", vec![tt, win, thr, hist], vec![must]),
        ("threats last", vec![tt, win, hist, thr], vec![must]),
        ("threats first", vec![thr, tt, win, hist], vec![must]),
        ("+centrality", vec![tt, win, cen, hist], vec![must]),
        (
            "+centrality, threats after",
            vec![tt, win, cen, thr, hist],
            vec![must],
        ),
        (
            "+centrality, threats before",
            vec![tt, win, thr, cen, hist],
            vec![must],
        ),
        (
            "+centrality, threats last",
            vec![tt, win, cen, hist, thr],
            vec![must],
        ),
    ];
    let variants = if wide {
        placements.to_vec()
    } else {
        variants.to_vec()
    };
    let mut values: Option<Vec<f64>> = None;
    for (name, pieces, filters) in variants {
        let chain = Chain::new(&pieces).unwrap();
        let mut runs = vec![];
        for _ in 0..3 {
            let t = Instant::now();
            let got = common::fresh_filtered(&g, &all, &chain, &filters);
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
