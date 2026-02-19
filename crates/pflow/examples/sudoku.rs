//! Sudoku ODE model — port of go-pflow/examples/sudoku/cmd/model.go (createODENet).
//!
//! Run with: cargo run -p pflow --example sudoku

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

/// Build an ODE-compatible Sudoku Petri net, identical to Go's createODENet.
pub fn create_ode_net(puzzle: &[Vec<u32>], block_size: usize) -> PetriNet {
    let size = puzzle.len();
    let num_blocks = size / block_size;
    let spacing = 30.0;

    let mut net = PetriNet::new();

    // Cell places (P{row}{col}): initial=1 if empty, 0 if clue. Capacity=[1].
    for row in 0..size {
        for col in 0..size {
            let label = format!("P{}_{}", row, col);
            let x = (col as f64 + 1.0) * spacing;
            let y = (row as f64 + 1.0) * spacing;
            let initial = if puzzle[row][col] == 0 { 1.0 } else { 0.0 };
            let label_text = format!("Cell({},{})", row, col);
            net.add_place(&label, vec![initial], vec![1.0], x, y, Some(label_text));
        }
    }

    // History places (_D{digit}_{row}{col}): initial=1 if clue matches. No capacity.
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
                let label_text = format!("History: {} at ({},{})", digit, row, col);
                net.add_place(&label, vec![initial], vec![], x, y, Some(label_text));
            }
        }
    }

    // Digit placement transitions (D{digit}_{row}{col}): only for empty cells.
    let trans_y = (size as f64 + 1.0) * spacing;
    for row in 0..size {
        for col in 0..size {
            if puzzle[row][col] == 0 {
                for digit in 1..=size {
                    let trans_id = format!("D{}_{}_{}", digit, row, col);
                    let x = (col * size + digit) as f64 * spacing / 3.0;
                    let role = format!("d{}", digit);
                    let label_text = format!("Place {} at ({},{})", digit, row, col);
                    net.add_transition(&trans_id, &role, x, trans_y, Some(label_text));

                    // Input arc from cell place
                    let cell_id = format!("P{}_{}", row, col);
                    net.add_arc(&cell_id, &trans_id, vec![1.0], false);

                    // Output arc to history place
                    let hist_id = format!("_D{}_{}_{}", digit, row, col);
                    net.add_arc(&trans_id, &hist_id, vec![1.0], false);
                }
            }
        }
    }

    // Constraint collector transitions
    let constraint_y = (size * 2 + 3) as f64 * spacing;

    // Row collectors
    for row in 0..size {
        let trans_id = format!("Row{}_Complete", row);
        let x = (size as f64 + 2.0) * spacing;
        let y = constraint_y + row as f64 * spacing / 2.0;
        let label_text = format!("Row {} Complete", row);
        net.add_transition(&trans_id, "constraint", x, y, Some(label_text));

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
        let label_text = format!("Column {} Complete", col);
        net.add_transition(&trans_id, "constraint", x, y, Some(label_text));

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
            let label_text = format!("Block ({},{}) Complete", br, bc);
            net.add_transition(&trans_id, "constraint", x, y, Some(label_text));

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

    // Solved place: capacity = numConstraints
    let num_constraints = size + size + num_blocks * num_blocks;
    let label_text = "Puzzle Solved".to_string();
    net.add_place(
        "solved",
        vec![0.0],
        vec![num_constraints as f64],
        (size as f64 + 5.0) * spacing,
        constraint_y,
        Some(label_text),
    );

    // Connect all constraint collectors to solved place
    for row in 0..size {
        let trans_id = format!("Row{}_Complete", row);
        net.add_arc(&trans_id, "solved", vec![1.0], false);
    }
    for col in 0..size {
        let trans_id = format!("Col{}_Complete", col);
        net.add_arc(&trans_id, "solved", vec![1.0], false);
    }
    for br in 0..num_blocks {
        for bc in 0..num_blocks {
            let trans_id = format!("Block{}_{}_Complete", br, bc);
            net.add_arc(&trans_id, "solved", vec![1.0], false);
        }
    }

    net
}

/// Solve a Sudoku ODE net with the same settings used in Go benchmarks.
pub fn solve_sudoku(net: &PetriNet) -> f64 {
    let state = net.set_state(None);
    let rates = net.set_rates(None);
    let prob = Problem::new(net.clone(), state, [0.0, 3.0], rates);
    let opts = Options {
        dt: 0.2,
        abstol: 1e-4,
        reltol: 1e-3,
        ..Options::default_opts()
    };
    let sol = solve(&prob, &methods::tsit5(), &opts);
    let final_state = sol.get_final_state().unwrap();
    *final_state.get("solved").unwrap_or(&0.0)
}

/// Empty puzzle for arbitrary sizes (benchmarking).
fn empty_puzzle(size: usize) -> Vec<Vec<u32>> {
    vec![vec![0; size]; size]
}

fn print_model(label: &str, net: &PetriNet) {
    println!(
        "{}: {} places, {} transitions, {} arcs",
        label,
        net.places.len(),
        net.transitions.len(),
        net.arcs.len()
    );
}

fn main() {
    let sizes: &[(usize, usize, Option<Vec<Vec<u32>>>)] = &[
        (4, 2, Some(puzzle_4x4())),
        (9, 3, Some(puzzle_9x9())),
        (16, 4, None),
    ];

    for (size, block_size, puzzle) in sizes {
        let puzzle = puzzle
            .clone()
            .unwrap_or_else(|| empty_puzzle(*size));
        let label = format!("{}x{}", size, size);
        let net = create_ode_net(&puzzle, *block_size);
        print_model(&label, &net);

        let start = std::time::Instant::now();
        let solved = solve_sudoku(&net);
        let elapsed = start.elapsed();
        println!("  solved={:.4}  time={:.3?}", solved, elapsed);
        println!();
    }
}
