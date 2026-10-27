//! Prints the order gate's lines (`net index value nodes`) for the three
//! fixture games: `cargo run --release -p pflow-metamodel --example search_gate`.
#[path = "../tests/common/mod.rs"]
mod common;

fn main() {
    for line in common::gate_lines() {
        println!("{line}");
    }
}
