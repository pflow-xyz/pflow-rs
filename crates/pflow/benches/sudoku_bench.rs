//! Criterion benchmarks for the Sudoku ODE solver.
//!
//! Run with: cargo bench -p pflow --bench sudoku_bench

use criterion::{criterion_group, criterion_main, Criterion};
use pflow::core::PetriNet;
use pflow::solver::{methods, solve, Options, Problem};

/// 4x4 puzzle (block_size=2)
fn puzzle_4x4() -> Vec<Vec<u32>> {
    vec![
        vec![1, 0, 0, 0],
        vec![0, 0, 2, 0],
        vec![0, 3, 0, 0],
        vec![0, 0, 0, 4],
    ]
}

/// 9x9 puzzle (block_size=3)
fn puzzle_9x9() -> Vec<Vec<u32>> {
    vec![
        vec![5, 3, 0, 0, 7, 0, 0, 0, 0],
        vec![6, 0, 0, 1, 9, 5, 0, 0, 0],
        vec![0, 9, 8, 0, 0, 0, 0, 6, 0],
        vec![8, 0, 0, 0, 6, 0, 0, 0, 3],
        vec![4, 0, 0, 8, 0, 3, 0, 0, 1],
        vec![7, 0, 0, 0, 2, 0, 0, 0, 6],
        vec![0, 6, 0, 0, 0, 0, 2, 8, 0],
        vec![0, 0, 0, 4, 1, 9, 0, 0, 5],
        vec![0, 0, 0, 0, 8, 0, 0, 7, 9],
    ]
}

/// Build an ODE-compatible Sudoku Petri net (identical to examples/sudoku.rs).
fn create_ode_net(puzzle: &[Vec<u32>], block_size: usize) -> PetriNet {
    let size = puzzle.len();
    let num_blocks = size / block_size;
    let spacing = 30.0;

    let mut net = PetriNet::new();

    // Cell places
    for row in 0..size {
        for col in 0..size {
            let label = format!("P{}_{}", row, col);
            let x = (col as f64 + 1.0) * spacing;
            let y = (row as f64 + 1.0) * spacing;
            let initial = if puzzle[row][col] == 0 { 1.0 } else { 0.0 };
            net.add_place(&label, vec![initial], vec![1.0], x, y, None);
        }
    }

    // History places
    let history_y = (size as f64 + 2.0) * spacing;
    for row in 0..size {
        for col in 0..size {
            for digit in 1..=size {
                let label = format!("_D{}_{}_{}", digit, row, col);
                let x = (col * size + digit) as f64 * spacing / 3.0;
                let y = history_y + row as f64 * spacing / 2.0;
                let initial = if puzzle[row][col] == digit as u32 {
                    1.0
                } else {
                    0.0
                };
                net.add_place(&label, vec![initial], vec![], x, y, None);
            }
        }
    }

    // Digit placement transitions (empty cells only)
    let trans_y = (size as f64 + 1.0) * spacing;
    for row in 0..size {
        for col in 0..size {
            if puzzle[row][col] == 0 {
                for digit in 1..=size {
                    let trans_id = format!("D{}_{}_{}", digit, row, col);
                    let x = (col * size + digit) as f64 * spacing / 3.0;
                    let role = format!("d{}", digit);
                    net.add_transition(&trans_id, &role, x, trans_y, None);

                    let cell_id = format!("P{}_{}", row, col);
                    net.add_arc(&cell_id, &trans_id, vec![1.0], false);

                    let hist_id = format!("_D{}_{}_{}", digit, row, col);
                    net.add_arc(&trans_id, &hist_id, vec![1.0], false);
                }
            }
        }
    }

    // Constraint collectors
    let constraint_y = (size * 2 + 3) as f64 * spacing;

    // Row collectors
    for row in 0..size {
        let trans_id = format!("Row{}_Complete", row);
        let x = (size as f64 + 2.0) * spacing;
        let y = constraint_y + row as f64 * spacing / 2.0;
        net.add_transition(&trans_id, "constraint", x, y, None);

        for col in 0..size {
            for digit in 1..=size {
                let hist_id = format!("_D{}_{}_{}", digit, row, col);
                net.add_arc(&hist_id, &trans_id, vec![1.0], false);
            }
        }
    }

    // Column collectors
    for col in 0..size {
        let trans_id = format!("Col{}_Complete", col);
        let x = (size as f64 + 3.0) * spacing;
        let y = constraint_y + col as f64 * spacing / 2.0;
        net.add_transition(&trans_id, "constraint", x, y, None);

        for row in 0..size {
            for digit in 1..=size {
                let hist_id = format!("_D{}_{}_{}", digit, row, col);
                net.add_arc(&hist_id, &trans_id, vec![1.0], false);
            }
        }
    }

    // Block collectors
    for br in 0..num_blocks {
        for bc in 0..num_blocks {
            let trans_id = format!("Block{}_{}_Complete", br, bc);
            let x = (size as f64 + 4.0) * spacing;
            let y = constraint_y + (br * num_blocks + bc) as f64 * spacing / 2.0;
            net.add_transition(&trans_id, "constraint", x, y, None);

            for i in 0..block_size {
                for j in 0..block_size {
                    let row = br * block_size + i;
                    let col = bc * block_size + j;
                    for digit in 1..=size {
                        let hist_id = format!("_D{}_{}_{}", digit, row, col);
                        net.add_arc(&hist_id, &trans_id, vec![1.0], false);
                    }
                }
            }
        }
    }

    // Solved place
    let num_constraints = size + size + num_blocks * num_blocks;
    net.add_place(
        "solved",
        vec![0.0],
        vec![num_constraints as f64],
        (size as f64 + 5.0) * spacing,
        constraint_y,
        None,
    );

    // Connect collectors to solved
    for row in 0..size {
        net.add_arc(&format!("Row{}_Complete", row), "solved", vec![1.0], false);
    }
    for col in 0..size {
        net.add_arc(&format!("Col{}_Complete", col), "solved", vec![1.0], false);
    }
    for br in 0..num_blocks {
        for bc in 0..num_blocks {
            net.add_arc(
                &format!("Block{}_{}_Complete", br, bc),
                "solved",
                vec![1.0],
                false,
            );
        }
    }

    net
}

fn sudoku_4x4_ode(c: &mut Criterion) {
    let puzzle = puzzle_4x4();
    let net = create_ode_net(&puzzle, 2);

    c.bench_function("sudoku_4x4_ode", |b| {
        b.iter(|| {
            let state = net.set_state(None);
            let rates = net.set_rates(None);
            let prob = Problem::new(net.clone(), state, [0.0, 3.0], rates);
            let opts = Options {
                dt: 0.2,
                abstol: 1e-4,
                reltol: 1e-3,
                ..Options::default_opts()
            };
            solve(&prob, &methods::tsit5(), &opts)
        })
    });
}

fn sudoku_9x9_ode(c: &mut Criterion) {
    let puzzle = puzzle_9x9();
    let net = create_ode_net(&puzzle, 3);

    let mut group = c.benchmark_group("sudoku_9x9");
    group.sample_size(10);
    group.bench_function("sudoku_9x9_ode", |b| {
        b.iter(|| {
            let state = net.set_state(None);
            let rates = net.set_rates(None);
            let prob = Problem::new(net.clone(), state, [0.0, 3.0], rates);
            let opts = Options {
                dt: 0.2,
                abstol: 1e-4,
                reltol: 1e-3,
                ..Options::default_opts()
            };
            solve(&prob, &methods::tsit5(), &opts)
        })
    });
    group.finish();
}

/// Empty puzzle for arbitrary sizes.
fn empty_puzzle(size: usize) -> Vec<Vec<u32>> {
    vec![vec![0; size]; size]
}

fn sudoku_16x16_ode(c: &mut Criterion) {
    let puzzle = empty_puzzle(16);
    let net = create_ode_net(&puzzle, 4);

    let mut group = c.benchmark_group("sudoku_16x16");
    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(300));
    group.bench_function("sudoku_16x16_ode", |b| {
        b.iter(|| {
            let state = net.set_state(None);
            let rates = net.set_rates(None);
            let prob = Problem::new(net.clone(), state, [0.0, 3.0], rates);
            let opts = Options {
                dt: 0.2,
                abstol: 1e-4,
                reltol: 1e-3,
                ..Options::default_opts()
            };
            solve(&prob, &methods::tsit5(), &opts)
        })
    });
    group.finish();
}

criterion_group!(benches, sudoku_4x4_ode, sudoku_9x9_ode, sudoku_16x16_ode);
criterion_main!(benches);
