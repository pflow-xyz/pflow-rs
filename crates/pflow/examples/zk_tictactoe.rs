//! ZK Tic-Tac-Toe: integer reduction + heatmap AI + Groth16 proofs.
//!
//! Demonstrates the "integer reduction" insight: running ODE simulation with
//! uniform rate constants on a Petri net encoding of tic-tac-toe recovers
//! strategic position values (center=4, corners=3, edges=2) purely from
//! network topology. Adds tactical heatmap scoring and proves each move
//! with Groth16 ZK proofs.
//!
//! ```bash
//! cargo run --example zk_tictactoe -p pflow --features zk-arkworks --release
//! cargo run --example zk_tictactoe -p pflow --features zk-arkworks --release -- --demo
//! ```

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

use pflow_core::PetriNet;
use pflow_solver::{
    equilibrium::{solve_until_equilibrium, EquilibriumOptions},
    methods, Options, Problem,
};
use pflow_zk::{fire_transition, IncidenceMatrix, PetriProver, TransitionWitness};
use pflow_zk_arkworks::ArkworksProver;
use pflow_zk_arkworks::solidity_export;

// ---------------------------------------------------------------------------
// Board representation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Empty,
    X,
    O,
}

impl std::fmt::Display for Cell {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Cell::Empty => write!(f, "."),
            Cell::X => write!(f, "X"),
            Cell::O => write!(f, "O"),
        }
    }
}

#[derive(Clone)]
struct Board {
    cells: [Cell; 9],
    move_count: usize,
}

impl Board {
    fn new() -> Self {
        Self {
            cells: [Cell::Empty; 9],
            move_count: 0,
        }
    }

    fn play(&mut self, cell: usize, piece: Cell) {
        self.cells[cell] = piece;
        self.move_count += 1;
    }

    fn is_empty(&self, cell: usize) -> bool {
        self.cells[cell] == Cell::Empty
    }
}

// Win patterns: 8 lines (3 rows, 3 cols, 2 diags)
const WIN_PATTERNS: [[usize; 3]; 8] = [
    [0, 1, 2], [3, 4, 5], [6, 7, 8], // rows
    [0, 3, 6], [1, 4, 7], [2, 5, 8], // cols
    [0, 4, 8], [2, 4, 6],             // diags
];

const WIN_PATTERN_NAMES: [&str; 8] = [
    "row0", "row1", "row2", "col0", "col1", "col2", "diag", "anti",
];

fn check_winner(board: &Board) -> Option<Cell> {
    for pat in &WIN_PATTERNS {
        let a = board.cells[pat[0]];
        if a != Cell::Empty && a == board.cells[pat[1]] && a == board.cells[pat[2]] {
            return Some(a);
        }
    }
    None
}

fn is_draw(board: &Board) -> bool {
    check_winner(board).is_none() && board.move_count == 9
}

fn print_board(board: &Board) {
    println!();
    for r in 0..3 {
        print!("  ");
        for c in 0..3 {
            let idx = r * 3 + c;
            if board.cells[idx] == Cell::Empty {
                print!(" {} ", idx);
            } else {
                print!(" {} ", board.cells[idx]);
            }
            if c < 2 {
                print!("|");
            }
        }
        println!();
        if r < 2 {
            println!("  ---+---+---");
        }
    }
    println!();
}

// ---------------------------------------------------------------------------
// Petri net construction
// ---------------------------------------------------------------------------

/// Build the analysis net for integer reduction via ODE.
///
/// Each cell has: a source place (P), a piece place (_X), a play transition
/// (P -> _X), and one *linear* drain transition per win-line the cell
/// participates in (_X -> sink). Under mass-action kinetics with uniform
/// rates, equilibrium _X concentration is inversely proportional to the
/// number of drain paths: center (4 lines) drains fastest, edges (2) slowest.
/// Inverting gives the strategic hierarchy: center=4, corner=3, edge=2.
fn build_analysis_net() -> PetriNet {
    let mut net = PetriNet::new();

    // Source and piece places for each cell
    for i in 0..3 {
        for j in 0..3 {
            net.add_place(format!("P{}_{}", i, j), vec![1.0], vec![], 0.0, 0.0, None);
            net.add_place(format!("_X{}_{}", i, j), vec![0.0], vec![], 0.0, 0.0, None);
        }
    }
    // Play transitions: P -> _X + P (P is catalyst, net-zero; constant inflow rate)
    for i in 0..3 {
        for j in 0..3 {
            let t = format!("Play{}_{}", i, j);
            net.add_transition(&t, "play", 0.0, 0.0, None);
            net.add_arc(format!("P{}_{}", i, j), &t, vec![1.0], false);
            net.add_arc(&t, format!("P{}_{}", i, j), vec![1.0], false); // return P
            net.add_arc(&t, format!("_X{}_{}", i, j), vec![1.0], false);
        }
    }

    // Per-cell-per-winline drain transitions: _X -> (consumed)
    // Each cell gets one drain transition per win line it belongs to.
    // Center (cell 4) belongs to 4 lines, corners to 3, edges to 2.
    // No output arc: tokens are simply consumed (absorbed by win-line evaluation).
    for (line_idx, pat) in WIN_PATTERNS.iter().enumerate() {
        for &cell in pat {
            let r = cell / 3;
            let c = cell % 3;
            let t = format!("drain_{}_{}_{}", r, c, line_idx);
            net.add_transition(&t, "drain", 0.0, 0.0, None);
            net.add_arc(format!("_X{}_{}", r, c), &t, vec![1.0], false);
        }
    }

    net
}

/// Build the full game net for ZK proofs (33 places, 35 transitions).
///
/// Includes turn control, move counting, game-active flag, and win/draw
/// detection. Win transitions use net-zero arcs on pieces (guard-style)
/// so pieces remain on the board after win detection.
fn build_game_net() -> PetriNet {
    let mut net = PetriNet::new();

    // Cell availability places (9)
    for r in 0..3 {
        for c in 0..3 {
            net.add_place(format!("p{}{}", r, c), vec![1.0], vec![], 0.0, 0.0, None);
        }
    }

    // X piece history places (9)
    for r in 0..3 {
        for c in 0..3 {
            net.add_place(format!("x{}{}", r, c), vec![0.0], vec![], 0.0, 0.0, None);
        }
    }

    // O piece history places (9)
    for r in 0..3 {
        for c in 0..3 {
            net.add_place(format!("o{}{}", r, c), vec![0.0], vec![], 0.0, 0.0, None);
        }
    }

    // Control places
    net.add_place("x_turn", vec![1.0], vec![], 0.0, 0.0, None);
    net.add_place("o_turn", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("win_x", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("win_o", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("game_active", vec![1.0], vec![], 0.0, 0.0, None);
    net.add_place("move_tokens", vec![0.0], vec![], 0.0, 0.0, None);

    // X play transitions (9)
    for r in 0..3 {
        for c in 0..3 {
            let t = format!("x_play_{}{}", r, c);
            net.add_transition(&t, "play_x", 0.0, 0.0, None);
            net.add_arc(format!("p{}{}", r, c), &t, vec![1.0], false);
            net.add_arc("x_turn", &t, vec![1.0], false);
            net.add_arc(&t, format!("x{}{}", r, c), vec![1.0], false);
            net.add_arc(&t, "o_turn", vec![1.0], false);
            net.add_arc(&t, "move_tokens", vec![1.0], false);
        }
    }

    // O play transitions (9)
    for r in 0..3 {
        for c in 0..3 {
            let t = format!("o_play_{}{}", r, c);
            net.add_transition(&t, "play_o", 0.0, 0.0, None);
            net.add_arc(format!("p{}{}", r, c), &t, vec![1.0], false);
            net.add_arc("o_turn", &t, vec![1.0], false);
            net.add_arc(&t, format!("o{}{}", r, c), vec![1.0], false);
            net.add_arc(&t, "x_turn", vec![1.0], false);
            net.add_arc(&t, "move_tokens", vec![1.0], false);
        }
    }

    // X win transitions (8): net-zero piece arcs + consume o_turn + game_active
    for (idx, pat) in WIN_PATTERNS.iter().enumerate() {
        let t = format!("x_win_{}", WIN_PATTERN_NAMES[idx]);
        net.add_transition(&t, "win_x", 0.0, 0.0, None);
        for &cell in pat {
            let r = cell / 3;
            let c = cell % 3;
            let place = format!("x{}{}", r, c);
            net.add_arc(&place, &t, vec![1.0], false);
            net.add_arc(&t, &place, vec![1.0], false);
        }
        net.add_arc("o_turn", &t, vec![1.0], false);
        net.add_arc("game_active", &t, vec![1.0], false);
        net.add_arc(&t, "win_x", vec![1.0], false);
    }

    // O win transitions (8): mirror
    for (idx, pat) in WIN_PATTERNS.iter().enumerate() {
        let t = format!("o_win_{}", WIN_PATTERN_NAMES[idx]);
        net.add_transition(&t, "win_o", 0.0, 0.0, None);
        for &cell in pat {
            let r = cell / 3;
            let c = cell % 3;
            let place = format!("o{}{}", r, c);
            net.add_arc(&place, &t, vec![1.0], false);
            net.add_arc(&t, &place, vec![1.0], false);
        }
        net.add_arc("x_turn", &t, vec![1.0], false);
        net.add_arc("game_active", &t, vec![1.0], false);
        net.add_arc(&t, "win_o", vec![1.0], false);
    }

    // Draw transition
    net.add_transition("draw", "draw", 0.0, 0.0, None);
    net.add_arc("move_tokens", "draw", vec![9.0], false);
    net.add_arc("game_active", "draw", vec![1.0], false);

    net
}

// ---------------------------------------------------------------------------
// Integer reduction via ODE equilibrium
// ---------------------------------------------------------------------------

fn integer_reduction() -> [f64; 9] {
    let net = build_analysis_net();

    println!(
        "  Analysis net: {} places, {} transitions, {} arcs",
        net.places.len(),
        net.transitions.len(),
        net.arcs.len()
    );

    let state = net.set_state(None);
    let rates = net.set_rates(None); // all rates = 1.0

    let prob = Problem::new(net, state, [0.0, 200.0], rates);
    let opts = Options {
        dt: 0.5,
        ..Options::default_opts()
    };
    let eq_opts = EquilibriumOptions {
        tolerance: 1e-4,
        consecutive_steps: 3,
        min_time: 0.5,
        check_interval: 5,
    };
    let (_, result) = solve_until_equilibrium(&prob, &methods::tsit5(), &opts, &eq_opts);
    let eq_state = result.state;
    let reached = result.reached;

    if !reached {
        println!("  Warning: equilibrium not fully reached, using final state");
    }

    // Extract x-piece equilibrium concentrations. Under mass-action kinetics,
    // cells with more win-line connections drain faster (more outflow transitions),
    // producing LOWER equilibrium concentrations. So strategic value is inversely
    // proportional to accumulation: invert to get center > corner > edge.
    let mut raw = [0.0f64; 9];
    for r in 0..3 {
        for c in 0..3 {
            let key = format!("_X{}_{}", r, c);
            raw[r * 3 + c] = eq_state.get(&key).copied().unwrap_or(0.0);
        }
    }

    // Invert: value = 1/concentration (more drainage = higher strategic value)
    let mut values = [0.0f64; 9];
    for i in 0..9 {
        if raw[i] > 1e-10 {
            values[i] = 1.0 / raw[i];
        }
    }

    // Normalize by minimum non-zero value to get integer-like ratios
    let min_val = values
        .iter()
        .copied()
        .filter(|&v| v > 1e-10)
        .fold(f64::MAX, f64::min);
    if min_val > 1e-10 {
        for v in &mut values {
            *v /= min_val;
        }
    }

    values
}

fn print_reduction(values: &[f64; 9]) {
    println!("  Integer Reduction (position values from ODE topology):");
    println!();
    for r in 0..3 {
        print!("    ");
        for c in 0..3 {
            print!("{:5.2}", values[r * 3 + c]);
            if c < 2 {
                print!("  ");
            }
        }
        println!();
    }

    let center = values[4];
    let corner_avg = (values[0] + values[2] + values[6] + values[8]) / 4.0;
    let edge_avg = (values[1] + values[3] + values[5] + values[7]) / 4.0;

    println!();
    println!(
        "  Center: {:.2}, Corners: {:.2}, Edges: {:.2}",
        center, corner_avg, edge_avg
    );
    println!("  Expected ratio ~4:3:2 (center > corner > edge)");
}

// ---------------------------------------------------------------------------
// Heatmap scoring (tactical evaluation)
// ---------------------------------------------------------------------------

/// Static position weights from integer reduction (rounded).
const POSITION_WEIGHTS: [f64; 9] = [3.0, 2.0, 3.0, 2.0, 4.0, 2.0, 3.0, 2.0, 3.0];

fn wins_if_played(board: &Board, cell: usize, piece: Cell) -> bool {
    for pat in &WIN_PATTERNS {
        if !pat.contains(&cell) {
            continue;
        }
        let others: Vec<usize> = pat.iter().copied().filter(|&i| i != cell).collect();
        if others.iter().all(|&i| board.cells[i] == piece) {
            return true;
        }
    }
    false
}

fn blocks_opponent(board: &Board, cell: usize, opponent: Cell) -> bool {
    if !board.is_empty(cell) {
        return false;
    }
    wins_if_played(board, cell, opponent)
}

fn compute_heatmap(board: &Board, player: Cell) -> (HashMap<usize, f64>, Option<usize>) {
    let opponent = if player == Cell::X { Cell::O } else { Cell::X };
    let mut scores: HashMap<usize, f64> = HashMap::new();
    let mut best_cell = None;
    let mut best_score = f64::NEG_INFINITY;

    for i in 0..9 {
        if !board.is_empty(i) {
            continue;
        }

        let win_flag = if wins_if_played(board, i, player) {
            1.0
        } else {
            0.0
        };
        let block_flag = if blocks_opponent(board, i, opponent) {
            1.0
        } else {
            0.0
        };

        let score = POSITION_WEIGHTS[i] + 10.0 * win_flag - 1.5 * block_flag * (1.0 - win_flag);
        scores.insert(i, score);

        if score > best_score {
            best_score = score;
            best_cell = Some(i);
        }
    }

    (scores, best_cell)
}

fn print_heatmap(scores: &HashMap<usize, f64>, board: &Board) {
    println!("  Heatmap scores:");
    for r in 0..3 {
        print!("    ");
        for c in 0..3 {
            let idx = r * 3 + c;
            if let Some(&s) = scores.get(&idx) {
                print!("{:5.1}", s);
            } else {
                print!("  {} ", board.cells[idx]);
            }
            if c < 2 {
                print!("  ");
            }
        }
        println!();
    }
}

// ---------------------------------------------------------------------------
// Transition naming and lookup
// ---------------------------------------------------------------------------

fn transition_name_for_move(cell: usize, piece: Cell) -> String {
    let r = cell / 3;
    let c = cell % 3;
    match piece {
        Cell::X => format!("x_play_{}{}", r, c),
        Cell::O => format!("o_play_{}{}", r, c),
        Cell::Empty => unreachable!(),
    }
}

fn find_transition_id(matrix: &IncidenceMatrix, name: &str) -> usize {
    matrix
        .transition_labels
        .iter()
        .position(|l| l == name)
        .unwrap_or_else(|| panic!("transition '{}' not found in matrix", name))
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

/// Compute the Poseidon hash of the initial marking as a hex string.
fn compute_initial_state_root(matrix: &IncidenceMatrix, net: &PetriNet) -> String {
    let marking = matrix.initial_marking(net);
    pflow_zk_arkworks::solidity_export::marking_to_state_root_hex(&marking)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let demo_mode = args.iter().any(|a| a == "--demo");
    let export_proof = args.iter().any(|a| a == "--export-proof");
    let export_solidity_dir = args
        .windows(2)
        .find(|w| w[0] == "--export-solidity")
        .map(|w| PathBuf::from(&w[1]));

    println!("ZK Tic-Tac-Toe: Integer Reduction + Groth16 Proofs");
    println!("===================================================\n");

    // Phase 1: Integer reduction on analysis net
    println!("Phase 1: Integer Reduction (ODE on analysis net)");
    println!("-------------------------------------------------");

    let t0 = Instant::now();
    let values = integer_reduction();
    let reduction_ms = t0.elapsed().as_secs_f64() * 1000.0;
    print_reduction(&values);
    println!("  ODE equilibrium time: {:.1}ms\n", reduction_ms);

    // Phase 2: Build game net and setup Groth16 prover
    println!("Phase 2: Game Net & Groth16 Prover Setup");
    println!("-----------------------------------------");

    let net = build_game_net();
    println!(
        "  Game net: {} places, {} transitions, {} arcs",
        net.places.len(),
        net.transitions.len(),
        net.arcs.len()
    );

    let matrix = IncidenceMatrix::from_petri_net(&net);
    println!(
        "  Incidence matrix: {} places x {} transitions",
        matrix.num_places, matrix.num_transitions
    );

    let mut prover = ArkworksProver::new(matrix.clone());
    let t0 = Instant::now();
    prover.setup().unwrap();
    let setup_ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("  Setup time: {:.1}ms\n", setup_ms);

    // --export-solidity: generate contracts and exit
    if let Some(dir) = export_solidity_dir {
        std::fs::create_dir_all(&dir).expect("failed to create output directory");

        // Groth16Verifier.sol
        let verifier_sol = prover.export_solidity_verifier().expect("VK export failed");
        let verifier_path = dir.join("Groth16Verifier.sol");
        std::fs::write(&verifier_path, &verifier_sol).expect("failed to write verifier");
        println!("  Wrote {}", verifier_path.display());

        // ZKTicTacToe.sol
        let initial_root = compute_initial_state_root(&matrix, &net);
        let game_sol = solidity_export::render_tictactoe_contract(&initial_root);
        let game_path = dir.join("ZKTicTacToe.sol");
        std::fs::write(&game_path, &game_sol).expect("failed to write game contract");
        println!("  Wrote {}", game_path.display());

        println!("\n  Initial state root: {}", initial_root);
        println!("\n  Deployment:");
        println!("    1. Deploy Groth16Verifier");
        println!("    2. Deploy ZKTicTacToe(verifierAddress)");
        println!("    3. Call playMove() with ZK proofs for each move");
        return;
    }

    // Phase 3: Game loop
    println!("Phase 3: Game Play with ZK Proofs");
    println!("---------------------------------");
    if demo_mode {
        println!("  Mode: AI vs AI (demo)\n");
    } else {
        println!("  Mode: Human (X) vs AI (O)");
        println!("  Enter cell number (0-8) to play.\n");
    }

    let mut board = Board::new();
    let mut current = Cell::X;
    let mut total_prove_ms = 0.0;
    let mut total_verify_ms = 0.0;
    let mut move_num = 0;

    // Track canonical marking through fire_transition chain
    let mut marking = matrix.initial_marking(&net);

    loop {
        print_board(&board);

        if let Some(winner) = check_winner(&board) {
            println!("  {} wins!\n", winner);
            break;
        }
        if is_draw(&board) {
            println!("  Draw!\n");
            break;
        }

        // Pick a move
        let cell = if current == Cell::X && !demo_mode {
            // Human move
            loop {
                print!("  Your move (X), cell [0-8]: ");
                io::stdout().flush().unwrap();
                let mut input = String::new();
                io::stdin().read_line(&mut input).unwrap();
                if let Ok(n) = input.trim().parse::<usize>() {
                    if n < 9 && board.is_empty(n) {
                        break n;
                    }
                }
                println!("  Invalid move, try again.");
            }
        } else {
            // AI move via heatmap
            let (scores, best) = compute_heatmap(&board, current);
            print_heatmap(&scores, &board);
            let cell = best.expect("no valid moves");
            println!(
                "  AI ({}) plays cell {} (score {:.1})",
                current, cell, scores[&cell]
            );
            cell
        };

        // Fire transition on the canonical marking
        let t_name = transition_name_for_move(cell, current);
        let tid = find_transition_id(&matrix, &t_name);
        let post_marking = fire_transition(&matrix, &marking, tid)
            .unwrap_or_else(|e| panic!("fire_transition failed: {}", e));

        let witness = TransitionWitness {
            pre_marking: marking.clone(),
            transition_id: tid,
            post_marking: post_marking.clone(),
        };

        // Update board and marking
        board.play(cell, current);
        move_num += 1;
        marking = post_marking;

        // Prove & verify
        let t0 = Instant::now();
        let proof = prover.prove(&witness).unwrap();
        let prove_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let t0 = Instant::now();
        let ok = prover.verify(&proof).unwrap();
        let verify_ms = t0.elapsed().as_secs_f64() * 1000.0;

        total_prove_ms += prove_ms;
        total_verify_ms += verify_ms;

        println!(
            "  Move {}: {} -> cell {} | prove {:.1}ms | verify {:.1}ms | {}B proof | {}",
            move_num,
            current,
            cell,
            prove_ms,
            verify_ms,
            proof.metrics.proof_size_bytes,
            if ok { "VALID" } else { "INVALID" }
        );

        if export_proof {
            if let Ok(calldata) = ArkworksProver::proof_to_calldata(&proof.proof_bytes) {
                println!("  Solidity calldata: {}", calldata.to_calldata_string());
            }
        }

        assert!(ok, "proof verification failed!");

        // Switch turns
        current = if current == Cell::X { Cell::O } else { Cell::X };
    }

    // Summary
    println!("Summary");
    println!("-------");
    println!("  Moves played: {}", move_num);
    println!("  Prover setup: {:.1}ms", setup_ms);
    println!("  Total prove time: {:.1}ms", total_prove_ms);
    println!("  Total verify time: {:.1}ms", total_verify_ms);
    println!(
        "  Avg prove/verify per move: {:.1}ms / {:.1}ms",
        total_prove_ms / move_num as f64,
        total_verify_ms / move_num as f64
    );
    println!("  ODE reduction time: {:.1}ms", reduction_ms);
    println!("  Proof system: {}", prover.system_name());
}
