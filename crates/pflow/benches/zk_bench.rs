//! ZK proof benchmarks for Petri net transitions.
//!
//! Run with: cargo bench -p pflow --features zk-arkworks --bench zk_bench

use criterion::{criterion_group, criterion_main, Criterion};
use pflow_core::PetriNet;
use pflow_zk::{fire_transition, IncidenceMatrix, PetriProver, TransitionWitness};
use pflow_zk_arkworks::ArkworksProver;

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

fn bench_arkworks_sir(c: &mut Criterion) {
    let net = sir_net();
    let matrix = IncidenceMatrix::from_petri_net(&net);
    let mut prover = ArkworksProver::new(matrix.clone());
    prover.setup().unwrap();

    let pre = matrix.initial_marking(&net);
    let post = fire_transition(&matrix, &pre, 0).unwrap();
    let witness = TransitionWitness {
        pre_marking: pre,
        transition_id: 0,
        post_marking: post,
    };

    let mut group = c.benchmark_group("arkworks_sir");

    group.bench_function("prove", |b| {
        b.iter(|| prover.prove(&witness).unwrap());
    });

    let proof = prover.prove(&witness).unwrap();
    group.bench_function("verify", |b| {
        b.iter(|| prover.verify(&proof).unwrap());
    });

    group.finish();
}

fn bench_arkworks_tictactoe(c: &mut Criterion) {
    let net = tictactoe_net();
    let matrix = IncidenceMatrix::from_petri_net(&net);
    let mut prover = ArkworksProver::new(matrix.clone());
    prover.setup().unwrap();

    let pre = matrix.initial_marking(&net);
    // Find first PlayX transition
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

    let mut group = c.benchmark_group("arkworks_tictactoe");

    group.bench_function("prove", |b| {
        b.iter(|| prover.prove(&witness).unwrap());
    });

    let proof = prover.prove(&witness).unwrap();
    group.bench_function("verify", |b| {
        b.iter(|| prover.verify(&proof).unwrap());
    });

    group.finish();
}

fn bench_incidence_extraction(c: &mut Criterion) {
    let sir = sir_net();
    let ttt = tictactoe_net();

    let mut group = c.benchmark_group("incidence_extraction");

    group.bench_function("sir", |b| {
        b.iter(|| IncidenceMatrix::from_petri_net(&sir));
    });

    group.bench_function("tictactoe", |b| {
        b.iter(|| IncidenceMatrix::from_petri_net(&ttt));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_arkworks_sir,
    bench_arkworks_tictactoe,
    bench_incidence_extraction
);
criterion_main!(benches);
