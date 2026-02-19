//! NxN Tic-Tac-Toe Petri Net ODE Benchmark
//!
//! Run with: cargo run -p pflow --example tictactoe --release

use pflow::core::PetriNet;
use pflow::solver::{methods, solve, Options, Problem};
use std::time::Instant;

/// Generate all N-in-a-row winning lines for an NxN board.
fn win_patterns(n: usize) -> Vec<Vec<(usize, usize)>> {
    let mut patterns = Vec::with_capacity(2 * n + 2);

    // Rows
    for i in 0..n {
        patterns.push((0..n).map(|j| (i, j)).collect());
    }
    // Columns
    for j in 0..n {
        patterns.push((0..n).map(|i| (i, j)).collect());
    }
    // Main diagonal
    patterns.push((0..n).map(|i| (i, i)).collect());
    // Anti-diagonal
    patterns.push((0..n).map(|i| (i, n - 1 - i)).collect());

    patterns
}

/// Build an NxN tic-tac-toe Petri net.
///
/// Places:
///   - P{i}_{j}  : cell availability (initial=1)
///   - _X{i}_{j} : X move history (initial=0)
///   - _O{i}_{j} : O move history (initial=0)
///   - Next       : turn marker (0=X, 1=O)
///   - win_x      : X win detector
///   - win_o      : O win detector
///
/// Transitions:
///   - PlayX{i}_{j} : X plays cell (i,j)
///   - PlayO{i}_{j} : O plays cell (i,j)
///   - WinX_{k}     : X win condition k
///   - WinO_{k}     : O win condition k
fn generate_tictactoe_net(n: usize) -> PetriNet {
    let mut net = PetriNet::new();
    let sp = 30.0;

    // Cell places
    for i in 0..n {
        for j in 0..n {
            net.add_place(
                format!("P{}_{}", i, j),
                vec![1.0],
                vec![],
                j as f64 * sp,
                i as f64 * sp,
                None,
            );
        }
    }

    // History places
    for i in 0..n {
        for j in 0..n {
            net.add_place(
                format!("_X{}_{}", i, j),
                vec![0.0],
                vec![],
                j as f64 * sp,
                (n + i) as f64 * sp,
                None,
            );
            net.add_place(
                format!("_O{}_{}", i, j),
                vec![0.0],
                vec![],
                j as f64 * sp,
                (2 * n + i) as f64 * sp,
                None,
            );
        }
    }

    // Control places
    net.add_place("Next", vec![0.0], vec![], n as f64 * sp, 0.0, None);
    net.add_place("win_x", vec![0.0], vec![], (n + 1) as f64 * sp, 0.0, None);
    net.add_place("win_o", vec![0.0], vec![], (n + 2) as f64 * sp, 0.0, None);

    // Move transitions
    for i in 0..n {
        for j in 0..n {
            let cell = format!("P{}_{}", i, j);

            // PlayX
            let tx = format!("PlayX{}_{}", i, j);
            net.add_transition(&tx, "move_x", j as f64 * sp, i as f64 * sp + 15.0, None);
            net.add_arc(&cell, &tx, vec![1.0], false);
            net.add_arc(&tx, &format!("_X{}_{}", i, j), vec![1.0], false);

            // PlayO
            let to = format!("PlayO{}_{}", i, j);
            net.add_transition(&to, "move_o", j as f64 * sp, i as f64 * sp + 15.0, None);
            net.add_arc(&cell, &to, vec![1.0], false);
            net.add_arc(&to, &format!("_O{}_{}", i, j), vec![1.0], false);
        }
    }

    // Win transitions
    let patterns = win_patterns(n);
    for (idx, pattern) in patterns.iter().enumerate() {
        // X win
        let tx = format!("WinX_{}", idx);
        net.add_transition(&tx, "win", (n + 1) as f64 * sp, idx as f64 * sp, None);
        for &(r, c) in pattern {
            net.add_arc(&format!("_X{}_{}", r, c), &tx, vec![1.0], false);
        }
        net.add_arc(&tx, "win_x", vec![1.0], false);

        // O win
        let to = format!("WinO_{}", idx);
        net.add_transition(&to, "win", (n + 2) as f64 * sp, idx as f64 * sp, None);
        for &(r, c) in pattern {
            net.add_arc(&format!("_O{}_{}", r, c), &to, vec![1.0], false);
        }
        net.add_arc(&to, "win_o", vec![1.0], false);
    }

    net
}

fn format_duration(d: std::time::Duration) -> String {
    let nanos = d.as_nanos();
    if nanos < 1_000 {
        format!("{}ns", nanos)
    } else if nanos < 1_000_000 {
        format!("{:.1}µs", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.1}ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.2}s", nanos as f64 / 1_000_000_000.0)
    }
}

fn main() {
    let sizes = [3, 5, 10, 15, 20, 25, 30, 35, 40];

    println!();
    println!("NxN Tic-Tac-Toe Petri Net ODE Benchmark (Rust)");
    println!("===============================================");
    println!(
        "{:<5} {:>8} {:>8} {:>8} {:>12} {:>12} {:>12}",
        "N", "Places", "Trans", "Arcs", "Build", "Solve", "Total"
    );
    println!("{}", "-".repeat(69));

    for &n in &sizes {
        // Build
        let build_start = Instant::now();
        let net = generate_tictactoe_net(n);
        let build_dur = build_start.elapsed();

        let places = net.places.len();
        let trans = net.transitions.len();
        let arcs = net.arcs.len();

        // Solve
        let state = net.set_state(None);
        let rates = net.set_rates(None);

        let solve_start = Instant::now();
        let prob = Problem::new(net, state, [0.0, 3.0], rates);
        let opts = Options {
            dt: 0.2,
            abstol: 1e-4,
            reltol: 1e-3,
            ..Options::default_opts()
        };
        let _sol = solve(&prob, &methods::tsit5(), &opts);
        let solve_dur = solve_start.elapsed();

        let total_dur = build_dur + solve_dur;

        println!(
            "{:<5} {:>8} {:>8} {:>8} {:>12} {:>12} {:>12}",
            n,
            places,
            trans,
            arcs,
            format_duration(build_dur),
            format_duration(solve_dur),
            format_duration(total_dur)
        );

        // Bail if a single size exceeds 10s
        if total_dur > std::time::Duration::from_secs(120) {
            println!("  (stopping: exceeded 10s for N={})", n);
            break;
        }
    }

    println!();
}
