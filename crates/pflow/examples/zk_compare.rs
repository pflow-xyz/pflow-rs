//! Side-by-side comparison of ZK proof strategies across net sizes.
//!
//! Run with: cargo run --example zk_compare -p pflow --features zk-arkworks,zk-risc0 --release

use std::time::Instant;

use pflow_core::PetriNet;
use pflow_zk::{fire_transition, IncidenceMatrix, PetriProver, TransitionWitness};
use pflow_zk_arkworks::ArkworksProver;
use pflow_zk_risc0::Risc0Prover;

fn sir_net() -> PetriNet {
    PetriNet::build().sir(999.0, 1.0, 0.0).done()
}

fn tictactoe_net(n: usize) -> PetriNet {
    let mut b = PetriNet::build();
    for i in 0..n {
        for j in 0..n {
            b = b.place(&format!("P{}_{}", i, j), 1.0);
            b = b.place(&format!("_X{}_{}", i, j), 0.0);
            b = b.place(&format!("_O{}_{}", i, j), 0.0);
        }
    }
    for i in 0..n {
        for j in 0..n {
            b = b.transition(&format!("PlayX{}_{}", i, j));
            b = b.arc(&format!("P{}_{}", i, j), &format!("PlayX{}_{}", i, j), 1.0);
            b = b.arc(&format!("PlayX{}_{}", i, j), &format!("_X{}_{}", i, j), 1.0);

            b = b.transition(&format!("PlayO{}_{}", i, j));
            b = b.arc(&format!("P{}_{}", i, j), &format!("PlayO{}_{}", i, j), 1.0);
            b = b.arc(&format!("PlayO{}_{}", i, j), &format!("_O{}_{}", i, j), 1.0);
        }
    }
    b.done()
}

struct Row {
    model: String,
    places: usize,
    transitions: usize,
    system: String,
    setup_ms: f64,
    prove_ms: f64,
    verify_ms: f64,
    proof_bytes: usize,
}

fn measure(
    model: &str,
    net: &PetriNet,
    prover: &mut dyn PetriProver,
) -> Row {
    let matrix = IncidenceMatrix::from_petri_net(net);
    let places = matrix.num_places;
    let transitions = matrix.num_transitions;

    let pre = matrix.initial_marking(net);
    let tid = matrix
        .transition_labels
        .iter()
        .position(|l| l.starts_with("PlayX") || l.starts_with("infect"))
        .unwrap_or(0);
    let post = fire_transition(&matrix, &pre, tid).unwrap();
    let witness = TransitionWitness {
        pre_marking: pre,
        transition_id: tid,
        post_marking: post,
    };

    // Setup
    let start = Instant::now();
    prover.setup().unwrap();
    let setup_ms = start.elapsed().as_secs_f64() * 1000.0;

    // Prove
    let start = Instant::now();
    let proof = prover.prove(&witness).unwrap();
    let prove_ms = start.elapsed().as_secs_f64() * 1000.0;

    // Verify
    let start = Instant::now();
    let ok = prover.verify(&proof).unwrap();
    let verify_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!(ok);

    Row {
        model: model.to_string(),
        places,
        transitions,
        system: prover.system_name().to_string(),
        setup_ms,
        prove_ms,
        verify_ms,
        proof_bytes: proof.metrics.proof_size_bytes,
    }
}

fn main() {
    println!("ZK Proof Strategy Comparison for Petri Nets");
    println!("============================================\n");

    let sizes: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();

    let models: Vec<(String, PetriNet)> = if sizes.is_empty() {
        vec![
            ("SIR".into(), sir_net()),
            ("TTT 3x3".into(), tictactoe_net(3)),
            ("TTT 5x5".into(), tictactoe_net(5)),
            ("TTT 10x10".into(), tictactoe_net(10)),
            ("TTT 20x20".into(), tictactoe_net(20)),
            ("TTT 40x40".into(), tictactoe_net(40)),
        ]
    } else {
        let mut v: Vec<(String, PetriNet)> = vec![("SIR".into(), sir_net())];
        for n in sizes {
            v.push((format!("TTT {}x{}", n, n), tictactoe_net(n)));
        }
        v
    };

    let mut rows = Vec::new();

    for (name, net) in &models {
        let matrix = IncidenceMatrix::from_petri_net(net);
        eprint!("{} ({}p, {}t) ... ", name, matrix.num_places, matrix.num_transitions);

        let mut ark = ArkworksProver::new(matrix.clone());
        rows.push(measure(name, net, &mut ark));
        eprint!("arkworks ");

        let mut r0 = Risc0Prover::new(matrix);
        rows.push(measure(name, net, &mut r0));
        eprintln!("risc0");
    }

    // Print table
    println!("\n{:<12} {:<15} {:>5} {:>5} {:>10} {:>10} {:>10} {:>8}",
        "Model", "System", "P", "T", "Setup", "Prove", "Verify", "Proof");
    println!("{}", "-".repeat(80));
    for r in &rows {
        println!(
            "{:<12} {:<15} {:>5} {:>5} {:>8.1}ms {:>8.1}ms {:>8.1}ms {:>6}B",
            r.model, r.system, r.places, r.transitions,
            r.setup_ms, r.prove_ms, r.verify_ms, r.proof_bytes,
        );
    }

    let r0_name = {
        let m = IncidenceMatrix::from_petri_net(&sir_net());
        let r0 = Risc0Prover::new(m);
        r0.system_name()
    };
    if r0_name == "risc0-sim" {
        println!("\nNote: risc0-sim is simulation mode (no STARK proofs).");
        println!("      Use --features zk-risc0-prove for real STARK proofs.");
    } else {
        println!("\nNote: risc0 is generating real STARK proofs.");
    }
    println!("      Arkworks verify time should be near-constant (Groth16 property).");
}
