//! Side-by-side comparison of ZK proof strategies.
//!
//! Run with: cargo run --example zk_compare -p pflow --features zk-arkworks,zk-risc0

use std::time::Instant;

use pflow_core::PetriNet;
use pflow_zk::{fire_transition, IncidenceMatrix, PetriProver, TransitionWitness};
use pflow_zk_arkworks::ArkworksProver;
use pflow_zk_risc0::Risc0Prover;

fn sir_net() -> PetriNet {
    PetriNet::build().sir(999.0, 1.0, 0.0).done()
}

fn tictactoe_net() -> PetriNet {
    let mut b = PetriNet::build();
    for i in 0..3 {
        for j in 0..3 {
            b = b.place(&format!("P{}_{}", i, j), 1.0);
            b = b.place(&format!("_X{}_{}", i, j), 0.0);
            b = b.place(&format!("_O{}_{}", i, j), 0.0);
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            b = b.transition(&format!("PlayX{}_{}", i, j));
            b = b
                .arc(&format!("P{}_{}", i, j), &format!("PlayX{}_{}", i, j), 1.0);
            b = b.arc(
                &format!("PlayX{}_{}", i, j),
                &format!("_X{}_{}", i, j),
                1.0,
            );

            b = b.transition(&format!("PlayO{}_{}", i, j));
            b = b
                .arc(&format!("P{}_{}", i, j), &format!("PlayO{}_{}", i, j), 1.0);
            b = b.arc(
                &format!("PlayO{}_{}", i, j),
                &format!("_O{}_{}", i, j),
                1.0,
            );
        }
    }
    b.done()
}

struct BenchResult {
    system: String,
    setup_ms: u64,
    prove_ms: u64,
    verify_ms: u64,
    proof_bytes: usize,
    vk_bytes: usize,
}

fn benchmark_prover(
    name: &str,
    prover: &mut dyn PetriProver,
    witness: &TransitionWitness,
) -> BenchResult {
    // Setup
    let start = Instant::now();
    prover.setup().unwrap();
    let setup_ms = start.elapsed().as_millis() as u64;

    // Prove
    let start = Instant::now();
    let proof = prover.prove(witness).unwrap();
    let prove_ms = start.elapsed().as_millis() as u64;

    // Verify
    let start = Instant::now();
    let valid = prover.verify(&proof).unwrap();
    let verify_ms = start.elapsed().as_millis() as u64;
    assert!(valid, "proof should verify");

    let vk = prover.verifying_key().unwrap();

    BenchResult {
        system: name.to_string(),
        setup_ms,
        prove_ms,
        verify_ms,
        proof_bytes: proof.metrics.proof_size_bytes,
        vk_bytes: vk.len(),
    }
}

fn print_table(model: &str, results: &[BenchResult]) {
    println!("\n=== {} ===", model);
    println!(
        "{:<20} {:>10} {:>10} {:>10} {:>12} {:>10}",
        "System", "Setup", "Prove", "Verify", "Proof Size", "VK Size"
    );
    println!("{}", "-".repeat(74));
    for r in results {
        println!(
            "{:<20} {:>8}ms {:>8}ms {:>8}ms {:>10}B {:>8}B",
            r.system, r.setup_ms, r.prove_ms, r.verify_ms, r.proof_bytes, r.vk_bytes,
        );
    }
}

fn main() {
    println!("ZK Proof Strategy Comparison for Petri Nets");
    println!("============================================\n");

    // --- SIR Model ---
    {
        let net = sir_net();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        println!(
            "SIR Model: {} places, {} transitions",
            matrix.num_places, matrix.num_transitions
        );

        let pre = matrix.initial_marking(&net);
        let post = fire_transition(&matrix, &pre, 0).unwrap();
        let witness = TransitionWitness {
            pre_marking: pre,
            transition_id: 0,
            post_marking: post,
        };

        let mut arkworks = ArkworksProver::new(matrix.clone());
        let mut risc0 = Risc0Prover::new(matrix);

        let results = vec![
            benchmark_prover("groth16-bn254", &mut arkworks, &witness),
            benchmark_prover("risc0-sim", &mut risc0, &witness),
        ];

        print_table("SIR (3 places, 2 transitions)", &results);
    }

    // --- Tic-Tac-Toe ---
    {
        let net = tictactoe_net();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        println!(
            "\nTic-Tac-Toe: {} places, {} transitions",
            matrix.num_places, matrix.num_transitions
        );

        let pre = matrix.initial_marking(&net);
        let tid = matrix
            .transition_labels
            .iter()
            .position(|l| l == "PlayX0_0")
            .unwrap();
        let post = fire_transition(&matrix, &pre, tid).unwrap();
        let witness = TransitionWitness {
            pre_marking: pre,
            transition_id: tid,
            post_marking: post,
        };

        let mut arkworks = ArkworksProver::new(matrix.clone());
        let mut risc0 = Risc0Prover::new(matrix);

        let results = vec![
            benchmark_prover("groth16-bn254", &mut arkworks, &witness),
            benchmark_prover("risc0-sim", &mut risc0, &witness),
        ];

        print_table("Tic-Tac-Toe (27 places, 18 transitions)", &results);
    }

    println!("\nNote: risc0-sim is simulation mode (no STARK proofs).");
    println!("      Install risc0 toolchain for real STARK benchmarks.");
}
