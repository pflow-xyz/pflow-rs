//! Chess Integer Reduction — Petri Net Analysis
//!
//! Phases 1–3: empty-board unweighted, piece-weighted, and FEN position analysis.
//!
//! Run:
//!   cargo run --example chess -p pflow
//!   cargo run --example chess -p pflow -- --fen "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"

use pflow::core::PetriNet;
use pflow::solver::equilibrium::{solve_until_equilibrium, EquilibriumOptions};
use pflow::solver::{methods, Options, Problem};
use shakmaty::fen::Fen;
use shakmaty::{attacks, Board, CastlingMode, Chess, Color, Position, Role, Square};

// ---------------------------------------------------------------------------
// Attack helpers (empty board, 8×8)
// ---------------------------------------------------------------------------

fn knight_attacks(r: usize, c: usize) -> Vec<(usize, usize)> {
    [(-2i32,-1),(-2,1),(-1,-2),(-1,2),(1,-2),(1,2),(2,-1),(2,1)].iter()
        .filter_map(|&(dr, dc)| {
            let (nr, nc) = (r as i32 + dr, c as i32 + dc);
            if (0..8).contains(&nr) && (0..8).contains(&nc) {
                Some((nr as usize, nc as usize))
            } else { None }
        }).collect()
}

fn sliding(r: usize, c: usize, dirs: &[(i32, i32)]) -> Vec<(usize, usize)> {
    let mut t = Vec::new();
    for &(dr, dc) in dirs {
        let (mut nr, mut nc) = (r as i32 + dr, c as i32 + dc);
        while (0..8).contains(&nr) && (0..8).contains(&nc) {
            t.push((nr as usize, nc as usize));
            nr += dr; nc += dc;
        }
    }
    t
}

fn bishop_rays(r: usize, c: usize) -> Vec<(usize, usize)> {
    sliding(r, c, &[(-1,-1),(-1,1),(1,-1),(1,1)])
}

fn rook_rays(r: usize, c: usize) -> Vec<(usize, usize)> {
    sliding(r, c, &[(-1,0),(1,0),(0,-1),(0,1)])
}

fn queen_attacks_empty(r: usize, c: usize) -> Vec<(usize, usize)> {
    sliding(r, c, &[(-1,-1),(-1,0),(-1,1),(0,-1),(0,1),(1,-1),(1,0),(1,1)])
}

fn king_attacks(r: usize, c: usize) -> Vec<(usize, usize)> {
    [(-1i32,-1),(-1,0),(-1,1),(0,-1),(0,1),(1,-1),(1,0),(1,1)].iter()
        .filter_map(|&(dr, dc)| {
            let (nr, nc) = (r as i32 + dr, c as i32 + dc);
            if (0..8).contains(&nr) && (0..8).contains(&nc) {
                Some((nr as usize, nc as usize))
            } else { None }
        }).collect()
}

fn pawn_attacks(r: usize, c: usize) -> Vec<(usize, usize)> {
    let mut t = Vec::new();
    if r > 0 {
        if c > 0 { t.push((r - 1, c - 1)); }
        if c < 7 { t.push((r - 1, c + 1)); }
    }
    if r < 7 {
        if c > 0 { t.push((r + 1, c - 1)); }
        if c < 7 { t.push((r + 1, c + 1)); }
    }
    t
}

// ---------------------------------------------------------------------------
// Net construction and ODE solve
// ---------------------------------------------------------------------------

type AttackFn = fn(usize, usize) -> Vec<(usize, usize)>;

const PIECE_TYPES: [(&str, f64); 6] = [
    ("P", 1.0), ("N", 3.0), ("B", 3.0), ("R", 5.0), ("Q", 9.0), ("K", 1.0),
];

fn attack_fn_for(piece: &str) -> AttackFn {
    match piece {
        "P" => pawn_attacks,
        "N" => knight_attacks,
        "B" => bishop_rays,
        "R" => rook_rays,
        "Q" => queen_attacks_empty,
        "K" => king_attacks,
        _ => unreachable!(),
    }
}

/// Build empty-board analysis net (Phase 1 or 2) and solve to equilibrium.
/// Uses consolidated drains: one drain per square with total weighted sum.
/// This is equivalent to individual drains (additive mass-action) but much faster.
fn run_empty_board(weighted: bool) -> [[f64; 8]; 8] {
    // Compute drain sums per square
    let mut drain_sums = [[0.0f64; 8]; 8];
    for r in 0..8usize {
        for c in 0..8usize {
            for &(piece, weight) in &PIECE_TYPES {
                let count = attack_fn_for(piece)(r, c).len() as f64;
                let w = if weighted { weight } else { 1.0 };
                drain_sums[r][c] += count * w;
            }
        }
    }
    let max_drain = drain_sums.iter().flat_map(|r| r.iter()).copied()
        .fold(f64::MIN, f64::max);

    // Build analysis net with normalized consolidated drains
    let mut net = PetriNet::new();
    for r in 0..8usize {
        for c in 0..8usize {
            net.add_place(format!("P{}_{}", r, c), vec![1.0], vec![], 0.0, 0.0, None);
            net.add_place(format!("_X{}_{}", r, c), vec![0.0], vec![], 0.0, 0.0, None);
        }
    }
    for r in 0..8usize {
        for c in 0..8usize {
            let t = format!("Play{}_{}", r, c);
            net.add_transition(&t, "play", 0.0, 0.0, None);
            net.add_arc(format!("P{}_{}", r, c), &t, vec![1.0], false);
            net.add_arc(&t, format!("P{}_{}", r, c), vec![1.0], false);
            net.add_arc(&t, format!("_X{}_{}", r, c), vec![1.0], false);

            let d = format!("drain_{}_{}", r, c);
            net.add_transition(&d, "drain", 0.0, 0.0, None);
            net.add_arc(
                format!("_X{}_{}", r, c), &d,
                vec![drain_sums[r][c] / max_drain], false,
            );
        }
    }

    solve_and_extract(&net)
}

/// Compute position control values (Phase 3) using shakmaty attacks.
/// For each piece, compute actual attacks (with blocking) and accumulate
/// weighted pressure on target squares. Values are analytical (drain_sum / min).
fn run_fen_analysis(fen_str: &str) -> [[f64; 8]; 8] {
    let fen: Fen = fen_str.parse().expect("invalid FEN");
    let pos: Chess = fen.into_position(CastlingMode::Standard).expect("illegal position");
    let board: &Board = pos.board();
    let occupied = board.occupied();

    // Accumulate total weighted attack pressure per target square
    let mut drain_sums = [[0.0f64; 8]; 8];
    for sq in Square::ALL {
        if let Some(piece) = board.piece_at(sq) {
            let weight = match piece.role {
                Role::Pawn => 1.0,
                Role::Knight => 3.0,
                Role::Bishop => 3.0,
                Role::Rook => 5.0,
                Role::Queen => 9.0,
                Role::King => 1.0,
            };
            let attack_bb = attacks::attacks(sq, piece, occupied);
            for target in attack_bb {
                let tr = 7 - usize::from(target.rank());
                let tc = usize::from(target.file());
                drain_sums[tr][tc] += weight;
            }
        }
    }

    // Analytical values: at equilibrium, value ∝ drain_sum.
    // Normalize by the minimum positive drain sum.
    let min_drain = drain_sums.iter().flat_map(|r| r.iter()).copied()
        .filter(|&v| v > 0.0).fold(f64::MAX, f64::min);
    let mut values = [[0.0f64; 8]; 8];
    for r in 0..8 {
        for c in 0..8 {
            values[r][c] = if drain_sums[r][c] > 0.0 {
                drain_sums[r][c] / min_drain
            } else {
                0.0
            };
        }
    }
    values
}

fn solve_and_extract(net: &PetriNet) -> [[f64; 8]; 8] {
    let state = net.set_state(None);
    let rates = net.set_rates(None);
    let prob = Problem::new(net.clone(), state, [0.0, 500.0], rates);
    let opts = Options { dt: 0.5, ..Options::default_opts() };
    let eq_opts = EquilibriumOptions {
        tolerance: 1e-4,
        consecutive_steps: 3,
        min_time: 0.5,
        check_interval: 5,
    };
    let (_, result) = solve_until_equilibrium(&prob, &methods::tsit5(), &opts, &eq_opts);
    if !result.reached {
        eprintln!("warning: ODE did not reach equilibrium");
    }

    let mut values = [[0.0f64; 8]; 8];
    for r in 0..8 {
        for c in 0..8 {
            let key = format!("_X{}_{}", r, c);
            let conc = result.state.get(&key).copied().unwrap_or(0.0);
            values[r][c] = if conc > 1e-10 { 1.0 / conc } else { 0.0 };
        }
    }
    let min_val = values.iter().flat_map(|r| r.iter())
        .copied().filter(|&v| v > 0.0).fold(f64::MAX, f64::min);
    if min_val > 0.0 && min_val < f64::MAX {
        for row in &mut values {
            for v in row.iter_mut() {
                if *v > 0.0 { *v /= min_val; }
            }
        }
    }
    values
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

fn print_heatmap(title: &str, values: &[[f64; 8]; 8]) {
    println!("\n{}", title);
    println!("{}", "-".repeat(title.len()));
    println!("      a     b     c     d     e     f     g     h");
    for r in 0..8 {
        let rank = 8 - r;
        print!("  {} ", rank);
        for c in 0..8 {
            print!("{:5.2} ", values[r][c]);
        }
        println!();
    }
    let max_val = values.iter().flat_map(|r| r.iter()).copied().fold(f64::MIN, f64::max);
    let min_val = values.iter().flat_map(|r| r.iter()).copied()
        .filter(|&v| v > 0.0).fold(f64::MAX, f64::min);
    println!("  range: {:.2} – {:.2}  (ratio {:.3})", min_val, max_val, max_val / min_val.max(1e-10));
}

fn print_board_with_values(title: &str, fen_str: &str, values: &[[f64; 8]; 8]) {
    let fen: Fen = fen_str.parse().unwrap();
    let pos: Chess = fen.into_position(CastlingMode::Standard).unwrap();
    let board = pos.board();

    println!("\n{}", title);
    println!("{}", "-".repeat(title.len()));
    println!("       a      b      c      d      e      f      g      h");
    for r in 0..8 {
        let rank = 8 - r;
        print!("  {} ", rank);
        for c in 0..8 {
            let sq = Square::new(((7 - r) as u32) * 8 + c as u32);
            let ch = board.piece_at(sq)
                .map(|p| {
                    let rc = p.role.char();
                    if p.color == Color::White { rc.to_ascii_uppercase() } else { rc }
                })
                .unwrap_or('.');
            if values[r][c] > 0.0 {
                print!("{}{:5.1} ", ch, values[r][c]);
            } else {
                print!("{}  -   ", ch);
            }
        }
        println!();
    }
    let max_val = values.iter().flat_map(|r| r.iter()).copied().fold(f64::MIN, f64::max);
    let min_val = values.iter().flat_map(|r| r.iter()).copied()
        .filter(|&v| v > 0.0).fold(f64::MAX, f64::min);
    println!("  range: {:.1} – {:.1}  (ratio {:.2})", min_val, max_val, max_val / min_val.max(1e-10));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fen_arg = args.iter().position(|a| a == "--fen").and_then(|i| args.get(i + 1));

    println!("Chess Integer Reduction Analysis");
    println!("================================\n");

    // Phase 1: Empty board, unweighted
    println!("Phase 1: Empty Board (Unweighted)");
    println!("Each piece type contributes 1 drain per reachable square.");
    let values1 = run_empty_board(false);
    print_heatmap("Unweighted Square Values (normalized by min)", &values1);

    // Phase 2: Empty board, piece-weighted
    println!("\nPhase 2: Empty Board (Piece-Weighted)");
    println!("Drain weights: P=1, N=3, B=3, R=5, Q=9, K=1");
    let values2 = run_empty_board(true);
    print_heatmap("Weighted Square Values (normalized by min)", &values2);

    // Phase 3: FEN position analysis
    if let Some(fen) = fen_arg {
        println!("\nPhase 3: Custom Position");
        println!("FEN: {}", fen);
        let values = run_fen_analysis(fen);
        print_board_with_values("Position Control Map", fen, &values);
    } else {
        let positions = [
            (
                "Starting Position",
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            ),
            (
                "Italian Game (1.e4 e5 2.Nf3 Nc6 3.Bc4)",
                "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 3 3",
            ),
            (
                "Sicilian Najdorf (1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6)",
                "rnbqkb1r/1p2pppp/p2p1n2/8/3NP3/2N5/PPP2PPP/R1BQKB1R w KQkq - 0 6",
            ),
        ];

        println!("\nPhase 3: Position Analysis (control map — weighted attack pressure per square)");
        for (name, fen) in &positions {
            let values = run_fen_analysis(fen);
            print_board_with_values(&format!("{}", name), fen, &values);
        }
    }
}
