//! Times `net_to_jsonld` with a forced seal (the CID) on chains of N places:
//! `cargo run --release -p pflow-hosted --example cid_timing -- 50 100 200`.
use serde_json::json;
use std::time::Instant;

fn chain(n: usize) -> serde_json::Value {
    let places: Vec<_> = (0..n)
        .map(|i| json!({"id": format!("p{i}"), "initial": if i == 0 {1} else {0}}))
        .collect();
    let transitions: Vec<_> = (0..n - 1).map(|i| json!({"id": format!("t{i}")})).collect();
    let arcs: Vec<_> = (0..n - 1)
        .flat_map(|i| {
            [
                json!({"from": format!("p{i}"), "to": format!("t{i}")}),
                json!({"from": format!("t{i}"), "to": format!("p{}", i + 1)}),
            ]
        })
        .collect();
    json!({"name": format!("chain{n}"), "places": places, "transitions": transitions, "arcs": arcs})
}

fn main() {
    let sizes: Vec<usize> = std::env::args()
        .skip(1)
        .map(|s| s.parse().unwrap())
        .collect();
    for n in if sizes.is_empty() {
        vec![25, 50, 100]
    } else {
        sizes
    } {
        let t = Instant::now();
        let out =
            pflow_hosted::call("net_to_jsonld", &json!({"model": chain(n), "cid": true})).unwrap();
        println!(
            "{n:5} places: {:8.3} s  {}",
            t.elapsed().as_secs_f64(),
            out["cid"]
        );
    }
}
