//! ZK proof benchmarks for Petri net transitions.
//!
//! Run with: cargo bench -p pflow --features zk-arkworks --bench zk_bench

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
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

struct PreparedBench {
    label: String,
    matrix: IncidenceMatrix,
    witness: TransitionWitness,
}

fn prepare(net: &PetriNet, label: &str) -> PreparedBench {
    let matrix = IncidenceMatrix::from_petri_net(net);
    let pre = matrix.initial_marking(net);
    let tid = matrix
        .transition_labels
        .iter()
        .position(|l| l.starts_with("PlayX") || l.starts_with("infect"))
        .unwrap_or(0);
    let post = fire_transition(&matrix, &pre, tid).unwrap();
    PreparedBench {
        label: label.to_string(),
        matrix,
        witness: TransitionWitness {
            pre_marking: pre,
            transition_id: tid,
            post_marking: post,
        },
    }
}

fn bench_prove(c: &mut Criterion) {
    let sir = sir_net();
    let cases: Vec<PreparedBench> = vec![
        prepare(&sir, "SIR(3p,2t)"),
        prepare(&tictactoe_net(2), "TTT-2x2(12p,8t)"),
        prepare(&tictactoe_net(3), "TTT-3x3(27p,18t)"),
        prepare(&tictactoe_net(4), "TTT-4x4(48p,32t)"),
        prepare(&tictactoe_net(5), "TTT-5x5(75p,50t)"),
    ];

    let mut group = c.benchmark_group("prove");
    for case in &cases {
        let mut ark = ArkworksProver::new(case.matrix.clone());
        ark.setup().unwrap();
        group.bench_with_input(
            BenchmarkId::new("arkworks", &case.label),
            &case.witness,
            |b, w| {
                b.iter(|| ark.prove(w).unwrap());
            },
        );

        let mut r0 = Risc0Prover::new(case.matrix.clone());
        r0.setup().unwrap();
        group.bench_with_input(
            BenchmarkId::new(r0.system_name(), &case.label),
            &case.witness,
            |b, w| {
                b.iter(|| r0.prove(w).unwrap());
            },
        );
    }
    group.finish();
}

fn bench_verify(c: &mut Criterion) {
    let sir = sir_net();
    let cases: Vec<PreparedBench> = vec![
        prepare(&sir, "SIR(3p,2t)"),
        prepare(&tictactoe_net(2), "TTT-2x2(12p,8t)"),
        prepare(&tictactoe_net(3), "TTT-3x3(27p,18t)"),
        prepare(&tictactoe_net(4), "TTT-4x4(48p,32t)"),
        prepare(&tictactoe_net(5), "TTT-5x5(75p,50t)"),
    ];

    let mut group = c.benchmark_group("verify");
    for case in &cases {
        let mut ark = ArkworksProver::new(case.matrix.clone());
        ark.setup().unwrap();
        let proof = ark.prove(&case.witness).unwrap();
        group.bench_with_input(
            BenchmarkId::new("arkworks", &case.label),
            &proof,
            |b, p| {
                b.iter(|| ark.verify(p).unwrap());
            },
        );

        let mut r0 = Risc0Prover::new(case.matrix.clone());
        r0.setup().unwrap();
        let proof = r0.prove(&case.witness).unwrap();
        group.bench_with_input(
            BenchmarkId::new(r0.system_name(), &case.label),
            &proof,
            |b, p| {
                b.iter(|| r0.verify(p).unwrap());
            },
        );
    }
    group.finish();
}

fn bench_setup(c: &mut Criterion) {
    let sir = sir_net();
    let cases: Vec<PreparedBench> = vec![
        prepare(&sir, "SIR(3p,2t)"),
        prepare(&tictactoe_net(2), "TTT-2x2(12p,8t)"),
        prepare(&tictactoe_net(3), "TTT-3x3(27p,18t)"),
        prepare(&tictactoe_net(4), "TTT-4x4(48p,32t)"),
        prepare(&tictactoe_net(5), "TTT-5x5(75p,50t)"),
    ];

    let mut group = c.benchmark_group("setup");
    for case in &cases {
        group.bench_with_input(
            BenchmarkId::new("arkworks", &case.label),
            &case.matrix,
            |b, m| {
                b.iter(|| {
                    let mut ark = ArkworksProver::new(m.clone());
                    ark.setup().unwrap();
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new(r0.system_name(), &case.label),
            &case.matrix,
            |b, m| {
                b.iter(|| {
                    let mut r0 = Risc0Prover::new(m.clone());
                    r0.setup().unwrap();
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_prove, bench_verify, bench_setup);
criterion_main!(benches);
