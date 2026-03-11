//! Tic-tac-toe Petri net fixtures for conjecture testing.
//!
//! Two independent constructions of the same game:
//! 1. **Builder (hand-declared)**: from pflow-rs tictactoe.rs example
//! 2. **Petri-pilot (LLM-declared)**: from petri-pilot code-to-flow output
//!
//! Conjecture 4 claims these converge on the same structure.

use crate::net_matrix::NetMatrix;

#[cfg(feature = "pflow")]
use pflow_core::PetriNet;

/// Win line patterns: (name, [(row, col); 3])
const WIN_LINES: [(&str, [(usize, usize); 3]); 8] = [
    ("row0", [(0, 0), (0, 1), (0, 2)]),
    ("row1", [(1, 0), (1, 1), (1, 2)]),
    ("row2", [(2, 0), (2, 1), (2, 2)]),
    ("col0", [(0, 0), (1, 0), (2, 0)]),
    ("col1", [(0, 1), (1, 1), (2, 1)]),
    ("col2", [(0, 2), (1, 2), (2, 2)]),
    ("diag", [(0, 0), (1, 1), (2, 2)]),
    ("anti", [(0, 2), (1, 1), (2, 0)]),
];

/// Place indices in the alphabetically-sorted 33-place schema:
///  0: game_active, 1: move_tokens,
///  2..10: o00..o22,  11: o_turn,
/// 12..20: p00..p22,  21: win_o, 22: win_x,
/// 23..31: x00..x22,  32: x_turn
fn place_index(name: &str) -> usize {
    // Alphabetical list of all 33 places
    static LABELS: &[&str] = &[
        "game_active", "move_tokens",
        "o00", "o01", "o02", "o10", "o11", "o12", "o20", "o21", "o22",
        "o_turn",
        "p00", "p01", "p02", "p10", "p11", "p12", "p20", "p21", "p22",
        "win_o", "win_x",
        "x00", "x01", "x02", "x10", "x11", "x12", "x20", "x21", "x22",
        "x_turn",
    ];
    LABELS.iter().position(|&l| l == name)
        .unwrap_or_else(|| panic!("unknown place: {name}"))
}

fn cell_place(prefix: &str, r: usize, c: usize) -> usize {
    place_index(&format!("{}{}{}", prefix, r, c))
}

/// 33 place labels in alphabetical order.
fn place_labels() -> Vec<String> {
    vec![
        "game_active", "move_tokens",
        "o00", "o01", "o02", "o10", "o11", "o12", "o20", "o21", "o22",
        "o_turn",
        "p00", "p01", "p02", "p10", "p11", "p12", "p20", "p21", "p22",
        "win_o", "win_x",
        "x00", "x01", "x02", "x10", "x11", "x12", "x20", "x21", "x22",
        "x_turn",
    ].into_iter().map(String::from).collect()
}

/// Initial marking: p00..p22=1, x_turn=1, game_active=1, rest=0.
fn initial_marking() -> Vec<i64> {
    let mut m = vec![0i64; 33];
    m[place_index("game_active")] = 1;
    m[place_index("x_turn")] = 1;
    for i in 0..3 {
        for j in 0..3 {
            m[cell_place("p", i, j)] = 1;
        }
    }
    m
}

/// Hand-declared TTT net matching the pflow-rs example (simplified: no turn control).
/// 27 places (9 cells x 3 states), 18 transitions (9 cells x 2 players).
pub fn builder_ttt() -> NetMatrix {
    let np = 27; // 9 cells * 3 states (p, x, o)
    let nt = 18; // 9 cells * 2 players

    let mut place_labels = Vec::with_capacity(np);
    let mut initial = vec![0i64; np];

    // Places: o00..o22 (0..8), p00..p22 (9..17), x00..x22 (18..26)
    for i in 0..3 {
        for j in 0..3 {
            place_labels.push(format!("o{}{}", i, j));
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            place_labels.push(format!("p{}{}", i, j));
            initial[9 + i * 3 + j] = 1; // p cells start with token
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            place_labels.push(format!("x{}{}", i, j));
        }
    }

    let mut transition_labels = Vec::with_capacity(nt);
    let mut incidence = Vec::with_capacity(nt);

    for i in 0..3 {
        for j in 0..3 {
            let cell = i * 3 + j;
            let p_idx = 9 + cell;  // p{i}{j}
            let x_idx = 18 + cell; // x{i}{j}

            // x_play: consumes p, produces x
            transition_labels.push(format!("x_play_{}{}", i, j));
            let mut row = vec![0i64; np];
            row[p_idx] = -1;
            row[x_idx] = 1;
            incidence.push(row);
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            let cell = i * 3 + j;
            let p_idx = 9 + cell;  // p{i}{j}
            let o_idx = cell;      // o{i}{j}

            // o_play: consumes p, produces o
            transition_labels.push(format!("o_play_{}{}", i, j));
            let mut row = vec![0i64; np];
            row[p_idx] = -1;
            row[o_idx] = 1;
            incidence.push(row);
        }
    }

    NetMatrix::from_incidence(incidence, initial, place_labels, transition_labels)
}

/// Petri-pilot (LLM-declared) TTT net from petri-pilot code-to-flow output.
/// 33 places, 35 transitions, 189 arcs — includes turn control, win detection, draw.
/// Win transitions consume (not catalytic) — game ends on win so tokens don't return.
pub fn pilot_ttt() -> NetMatrix {
    let np = 33;
    let labels = place_labels();
    let initial = initial_marking();

    // Transition order (alphabetical): draw, o_play_00..o_play_22,
    // o_win_anti..o_win_row2, x_play_00..x_play_22, x_win_anti..x_win_row2
    let mut transition_labels = Vec::new();
    let mut incidence = Vec::new();
    let mut input = Vec::new();

    // Helper: add a transition with given inc/inp rows
    let mut add_transition = |name: &str, inc: Vec<i64>, inp: Vec<i64>| {
        transition_labels.push(name.to_string());
        incidence.push(inc);
        input.push(inp);
    };

    // Draw transition: consumes 9 move_tokens + game_active, produces win_o
    {
        let mut inc = vec![0i64; np];
        let mut inp = vec![0i64; np];
        inp[place_index("move_tokens")] = 9;
        inc[place_index("move_tokens")] = -9;
        inp[place_index("game_active")] = 1;
        inc[place_index("game_active")] = -1;
        inc[place_index("win_o")] = 1;
        add_transition("draw", inc, inp);
    }

    // O play transitions (o_play_00 .. o_play_22)
    for i in 0..3 {
        for j in 0..3 {
            let mut inc = vec![0i64; np];
            let mut inp = vec![0i64; np];
            // consumes: p{i}{j}, o_turn
            inp[cell_place("p", i, j)] = 1;
            inc[cell_place("p", i, j)] = -1;
            inp[place_index("o_turn")] = 1;
            inc[place_index("o_turn")] = -1;
            // produces: o{i}{j}, x_turn, move_tokens
            inc[cell_place("o", i, j)] = 1;
            inc[place_index("x_turn")] = 1;
            inc[place_index("move_tokens")] = 1;
            add_transition(&format!("o_play_{}{}", i, j), inc, inp);
        }
    }

    // O win transitions (alphabetical: anti, col0..col2, diag, row0..row2)
    // No catalytic arcs: game ends on win, so consumed cells don't need to return.
    for &(name, ref cells) in &WIN_LINES {
        let mut inc = vec![0i64; np];
        let mut inp = vec![0i64; np];
        // consume o cells (no reproduce — game is over)
        for &(r, c) in cells {
            inp[cell_place("o", r, c)] = 1;
            inc[cell_place("o", r, c)] = -1;
        }
        // consume game_active + x_turn
        inp[place_index("game_active")] = 1;
        inc[place_index("game_active")] = -1;
        inp[place_index("x_turn")] = 1;
        inc[place_index("x_turn")] = -1;
        // produce win_o
        inc[place_index("win_o")] = 1;
        add_transition(&format!("o_win_{}", name), inc, inp);
    }

    // X play transitions (x_play_00 .. x_play_22)
    for i in 0..3 {
        for j in 0..3 {
            let mut inc = vec![0i64; np];
            let mut inp = vec![0i64; np];
            // consumes: p{i}{j}, x_turn
            inp[cell_place("p", i, j)] = 1;
            inc[cell_place("p", i, j)] = -1;
            inp[place_index("x_turn")] = 1;
            inc[place_index("x_turn")] = -1;
            // produces: x{i}{j}, o_turn, move_tokens
            inc[cell_place("x", i, j)] = 1;
            inc[place_index("o_turn")] = 1;
            inc[place_index("move_tokens")] = 1;
            add_transition(&format!("x_play_{}{}", i, j), inc, inp);
        }
    }

    // X win transitions (alphabetical: anti, col0..col2, diag, row0..row2)
    // No catalytic arcs: game ends on win, so consumed cells don't need to return.
    for &(name, ref cells) in &WIN_LINES {
        let mut inc = vec![0i64; np];
        let mut inp = vec![0i64; np];
        // consume x cells (no reproduce — game is over)
        for &(r, c) in cells {
            inp[cell_place("x", r, c)] = 1;
            inc[cell_place("x", r, c)] = -1;
        }
        // consume game_active + o_turn
        inp[place_index("game_active")] = 1;
        inc[place_index("game_active")] = -1;
        inp[place_index("o_turn")] = 1;
        inc[place_index("o_turn")] = -1;
        // produce win_x
        inc[place_index("win_x")] = 1;
        add_transition(&format!("x_win_{}", name), inc, inp);
    }

    NetMatrix::new(incidence, input, initial, labels, transition_labels)
}

/// Core TTT with turn enforcement via x_turn/o_turn places.
/// 29 places (9 cells × 3 states + 2 turn), 18 transitions.
///
/// Each x_play consumes x_turn, produces o_turn (and vice versa).
/// The turn token ping-pongs: x_turn → o_turn → x_turn → ...
///
/// Place layout (alphabetical):
///   o00..o22 (0-8), o_turn (9), p00..p22 (10-18), x00..x22 (19-27), x_turn (28)
pub fn builder_ttt_turns() -> NetMatrix {
    let np = 29;
    let nt = 18;

    let mut place_labels = Vec::with_capacity(np);
    let mut initial = vec![0i64; np];

    // o00..o22 (indices 0-8)
    for i in 0..3 {
        for j in 0..3 {
            place_labels.push(format!("o{}{}", i, j));
        }
    }
    // o_turn (index 9)
    place_labels.push("o_turn".to_string());
    // p00..p22 (indices 10-18)
    for i in 0..3 {
        for j in 0..3 {
            place_labels.push(format!("p{}{}", i, j));
            initial[10 + i * 3 + j] = 1;
        }
    }
    // x00..x22 (indices 19-27)
    for i in 0..3 {
        for j in 0..3 {
            place_labels.push(format!("x{}{}", i, j));
        }
    }
    // x_turn (index 28) — X goes first
    place_labels.push("x_turn".to_string());
    initial[28] = 1;

    let mut transition_labels = Vec::with_capacity(nt);
    let mut incidence = Vec::with_capacity(nt);

    // x_play_ij: consumes p_ij + x_turn, produces x_ij + o_turn
    for i in 0..3 {
        for j in 0..3 {
            let cell = i * 3 + j;
            let p_idx = 10 + cell;
            let x_idx = 19 + cell;

            transition_labels.push(format!("x_play_{}{}", i, j));
            let mut row = vec![0i64; np];
            row[p_idx] = -1;     // consume empty cell
            row[x_idx] = 1;      // produce X mark
            row[28] = -1;        // consume x_turn
            row[9] = 1;          // produce o_turn
            incidence.push(row);
        }
    }

    // o_play_ij: consumes p_ij + o_turn, produces o_ij + x_turn
    for i in 0..3 {
        for j in 0..3 {
            let cell = i * 3 + j;
            let p_idx = 10 + cell;
            let o_idx = cell;

            transition_labels.push(format!("o_play_{}{}", i, j));
            let mut row = vec![0i64; np];
            row[p_idx] = -1;     // consume empty cell
            row[o_idx] = 1;      // produce O mark
            row[9] = -1;         // consume o_turn
            row[28] = 1;         // produce x_turn
            incidence.push(row);
        }
    }

    NetMatrix::from_incidence(incidence, initial, place_labels, transition_labels)
}

/// NxN Tic-Tac-Toe with turn enforcement (core net, no win detection).
///
/// Places: n² empty cells + n² x cells + n² o cells + x_turn + o_turn = 3n² + 2
/// Transitions: 2n² (n² x-plays + n² o-plays)
/// Win condition: N-in-a-row (rows, columns, both diagonals)
///
/// Place layout (alphabetical): o00..o{n-1}{n-1}, o_turn, p00..p{n-1}{n-1}, x00..x{n-1}{n-1}, x_turn
pub fn builder_ttt_nxn(n: usize) -> NetMatrix {
    assert!(n >= 2, "board size must be at least 2");
    let n2 = n * n;
    let np = 3 * n2 + 2; // o cells + o_turn + p cells + x cells + x_turn
    let nt = 2 * n2;

    let mut place_labels = Vec::with_capacity(np);
    let mut initial = vec![0i64; np];

    // o00..o{n-1}{n-1} (indices 0..n²-1)
    for i in 0..n {
        for j in 0..n {
            place_labels.push(format!("o{}{}", i, j));
        }
    }
    // o_turn (index n²)
    let o_turn = n2;
    place_labels.push("o_turn".to_string());
    // p00..p{n-1}{n-1} (indices n²+1..2n²)
    let p_base = n2 + 1;
    for i in 0..n {
        for j in 0..n {
            place_labels.push(format!("p{}{}", i, j));
            initial[p_base + i * n + j] = 1;
        }
    }
    // x00..x{n-1}{n-1} (indices 2n²+1..3n²)
    let x_base = 2 * n2 + 1;
    for i in 0..n {
        for j in 0..n {
            place_labels.push(format!("x{}{}", i, j));
        }
    }
    // x_turn (index 3n²+1) — X goes first
    let x_turn = 3 * n2 + 1;
    place_labels.push("x_turn".to_string());
    initial[x_turn] = 1;

    let mut transition_labels = Vec::with_capacity(nt);
    let mut incidence = Vec::with_capacity(nt);

    // o_play_ij: consumes p_ij + o_turn, produces o_ij + x_turn
    for i in 0..n {
        for j in 0..n {
            let cell = i * n + j;
            transition_labels.push(format!("o_play_{}{}", i, j));
            let mut row = vec![0i64; np];
            row[p_base + cell] = -1;
            row[cell] = 1;          // o cell
            row[o_turn] = -1;
            row[x_turn] = 1;
            incidence.push(row);
        }
    }

    // x_play_ij: consumes p_ij + x_turn, produces x_ij + o_turn
    for i in 0..n {
        for j in 0..n {
            let cell = i * n + j;
            transition_labels.push(format!("x_play_{}{}", i, j));
            let mut row = vec![0i64; np];
            row[p_base + cell] = -1;
            row[x_base + cell] = 1; // x cell
            row[x_turn] = -1;
            row[o_turn] = 1;
            incidence.push(row);
        }
    }

    NetMatrix::from_incidence(incidence, initial, place_labels, transition_labels)
}

/// Generate win lines for an NxN board: N-in-a-row across rows, columns, and both diagonals.
pub fn win_lines_nxn(n: usize) -> Vec<(String, Vec<(usize, usize)>)> {
    let mut lines = Vec::new();
    // Rows
    for i in 0..n {
        let cells: Vec<(usize, usize)> = (0..n).map(|j| (i, j)).collect();
        lines.push((format!("row{}", i), cells));
    }
    // Columns
    for j in 0..n {
        let cells: Vec<(usize, usize)> = (0..n).map(|i| (i, j)).collect();
        lines.push((format!("col{}", j), cells));
    }
    // Main diagonal
    let diag: Vec<(usize, usize)> = (0..n).map(|i| (i, i)).collect();
    lines.push(("diag".to_string(), diag));
    // Anti-diagonal
    let anti: Vec<(usize, usize)> = (0..n).map(|i| (i, n - 1 - i)).collect();
    lines.push(("anti".to_string(), anti));
    lines
}

/// NxN TTT with turn enforcement AND win detection + draw (full game net).
///
/// Places: 3n² + 4 (cells + turns + game_active + move_tokens + win_x + win_o)
/// Transitions: 2n² plays + 2(n+1) win checks + 1 draw = 2n² + 2n + 3
pub fn builder_ttt_nxn_full(n: usize) -> NetMatrix {
    assert!(n >= 2, "board size must be at least 2");
    let n2 = n * n;
    let win_lines = win_lines_nxn(n);
    let num_win_lines = win_lines.len(); // 2n + 2

    // Places (alphabetical): game_active, move_tokens,
    //   o00..o{n-1}{n-1}, o_turn, p00..p{n-1}{n-1},
    //   win_o, win_x, x00..x{n-1}{n-1}, x_turn
    let np = 3 * n2 + 6; // +game_active, +move_tokens, +o_turn, +win_o, +win_x, +x_turn
    let nt = 2 * n2 + 2 * num_win_lines + 1; // plays + win transitions + draw

    let mut place_labels = Vec::with_capacity(np);
    let mut initial = vec![0i64; np];

    // game_active (0)
    place_labels.push("game_active".to_string());
    initial[0] = 1;
    // move_tokens (1)
    place_labels.push("move_tokens".to_string());
    // o00..o{n-1}{n-1} (2..n²+1)
    let o_base = 2;
    for i in 0..n {
        for j in 0..n {
            place_labels.push(format!("o{}{}", i, j));
        }
    }
    // o_turn (n²+2)
    let o_turn = n2 + 2;
    place_labels.push("o_turn".to_string());
    // p00..p{n-1}{n-1} (n²+3..2n²+2)
    let p_base = n2 + 3;
    for i in 0..n {
        for j in 0..n {
            place_labels.push(format!("p{}{}", i, j));
            initial[p_base + i * n + j] = 1;
        }
    }
    // win_o (2n²+3)
    let win_o = 2 * n2 + 3;
    place_labels.push("win_o".to_string());
    // win_x (2n²+4)
    let win_x = 2 * n2 + 4;
    place_labels.push("win_x".to_string());
    // x00..x{n-1}{n-1} (2n²+5..3n²+4)
    let x_base = 2 * n2 + 5;
    for i in 0..n {
        for j in 0..n {
            place_labels.push(format!("x{}{}", i, j));
        }
    }
    // x_turn (3n²+5)
    let x_turn = 3 * n2 + 5;
    place_labels.push("x_turn".to_string());
    initial[x_turn] = 1;

    let ga = 0usize; // game_active
    let mt = 1usize; // move_tokens

    let mut transition_labels = Vec::with_capacity(nt);
    let mut incidence = Vec::with_capacity(nt);
    let mut input = Vec::with_capacity(nt);

    // draw: consumes n² move_tokens + game_active, produces win_o (draw counts as O "win" placeholder)
    {
        let mut inc = vec![0i64; np];
        let mut inp = vec![0i64; np];
        inp[mt] = n2 as i64;
        inc[mt] = -(n2 as i64);
        inp[ga] = 1;
        inc[ga] = -1;
        inc[win_o] = 1;
        transition_labels.push("draw".to_string());
        incidence.push(inc);
        input.push(inp);
    }

    // o_play_ij
    for i in 0..n {
        for j in 0..n {
            let cell = i * n + j;
            let mut inc = vec![0i64; np];
            let mut inp = vec![0i64; np];
            inp[p_base + cell] = 1;
            inc[p_base + cell] = -1;
            inp[o_turn] = 1;
            inc[o_turn] = -1;
            inc[o_base + cell] = 1;
            inc[x_turn] = 1;
            inc[mt] = 1;
            transition_labels.push(format!("o_play_{}{}", i, j));
            incidence.push(inc);
            input.push(inp);
        }
    }

    // o_win_* (alphabetical by win line name)
    for (name, cells) in &win_lines {
        let mut inc = vec![0i64; np];
        let mut inp = vec![0i64; np];
        for &(r, c) in cells {
            inp[o_base + r * n + c] = 1;
            inc[o_base + r * n + c] = -1;
        }
        inp[ga] = 1;
        inc[ga] = -1;
        inp[x_turn] = 1;
        inc[x_turn] = -1;
        inc[win_o] = 1;
        transition_labels.push(format!("o_win_{}", name));
        incidence.push(inc);
        input.push(inp);
    }

    // x_play_ij
    for i in 0..n {
        for j in 0..n {
            let cell = i * n + j;
            let mut inc = vec![0i64; np];
            let mut inp = vec![0i64; np];
            inp[p_base + cell] = 1;
            inc[p_base + cell] = -1;
            inp[x_turn] = 1;
            inc[x_turn] = -1;
            inc[x_base + cell] = 1;
            inc[o_turn] = 1;
            inc[mt] = 1;
            transition_labels.push(format!("x_play_{}{}", i, j));
            incidence.push(inc);
            input.push(inp);
        }
    }

    // x_win_*
    for (name, cells) in &win_lines {
        let mut inc = vec![0i64; np];
        let mut inp = vec![0i64; np];
        for &(r, c) in cells {
            inp[x_base + r * n + c] = 1;
            inc[x_base + r * n + c] = -1;
        }
        inp[ga] = 1;
        inc[ga] = -1;
        inp[o_turn] = 1;
        inc[o_turn] = -1;
        inc[win_x] = 1;
        transition_labels.push(format!("x_win_{}", name));
        incidence.push(inc);
        input.push(inp);
    }

    NetMatrix::new(incidence, input, initial, place_labels, transition_labels)
}

/// Builder TTT with win detection (matching pilot topology).
pub fn builder_ttt_full() -> NetMatrix {
    // Same structure as pilot_ttt — this is the "hand-declared" equivalent
    pilot_ttt()
}

/// PetriNet-returning variant of pilot_ttt (for pflow integration tests).
#[cfg(feature = "pflow")]
pub fn pilot_ttt_petri_net() -> PetriNet {
    let mut b = PetriNet::build();

    for i in 0..3 {
        for j in 0..3 {
            b = b.place(&format!("p{}{}", i, j), 1.0);
            b = b.place(&format!("x{}{}", i, j), 0.0);
            b = b.place(&format!("o{}{}", i, j), 0.0);
        }
    }
    b = b
        .place("x_turn", 1.0)
        .place("o_turn", 0.0)
        .place("game_active", 1.0)
        .place("move_tokens", 0.0)
        .place("win_x", 0.0)
        .place("win_o", 0.0);

    for i in 0..3 {
        for j in 0..3 {
            let t = format!("x_play_{}{}", i, j);
            b = b
                .transition(&t)
                .arc(&format!("p{}{}", i, j), &t, 1.0)
                .arc("x_turn", &t, 1.0)
                .arc(&t, &format!("x{}{}", i, j), 1.0)
                .arc(&t, "o_turn", 1.0)
                .arc(&t, "move_tokens", 1.0);
        }
    }

    for i in 0..3 {
        for j in 0..3 {
            let t = format!("o_play_{}{}", i, j);
            b = b
                .transition(&t)
                .arc(&format!("p{}{}", i, j), &t, 1.0)
                .arc("o_turn", &t, 1.0)
                .arc(&t, &format!("o{}{}", i, j), 1.0)
                .arc(&t, "x_turn", 1.0)
                .arc(&t, "move_tokens", 1.0);
        }
    }

    let win_lines: Vec<(&str, Vec<(usize, usize)>)> = vec![
        ("row0", vec![(0, 0), (0, 1), (0, 2)]),
        ("row1", vec![(1, 0), (1, 1), (1, 2)]),
        ("row2", vec![(2, 0), (2, 1), (2, 2)]),
        ("col0", vec![(0, 0), (1, 0), (2, 0)]),
        ("col1", vec![(0, 1), (1, 1), (2, 1)]),
        ("col2", vec![(0, 2), (1, 2), (2, 2)]),
        ("diag", vec![(0, 0), (1, 1), (2, 2)]),
        ("anti", vec![(0, 2), (1, 1), (2, 0)]),
    ];

    for (name, cells) in &win_lines {
        // X win: consume x cells (no reproduce — game is over)
        let tx = format!("x_win_{}", name);
        b = b.transition(&tx);
        for &(r, c) in cells {
            b = b.arc(&format!("x{}{}", r, c), &tx, 1.0);
        }
        b = b
            .arc("game_active", &tx, 1.0)
            .arc("o_turn", &tx, 1.0)
            .arc(&tx, "win_x", 1.0);

        // O win: consume o cells (no reproduce — game is over)
        let to = format!("o_win_{}", name);
        b = b.transition(&to);
        for &(r, c) in cells {
            b = b.arc(&format!("o{}{}", r, c), &to, 1.0);
        }
        b = b
            .arc("game_active", &to, 1.0)
            .arc("x_turn", &to, 1.0)
            .arc(&to, "win_o", 1.0);
    }

    b = b
        .transition("draw")
        .arc("move_tokens", "draw", 9.0)
        .arc("game_active", "draw", 1.0)
        .arc("draw", "win_o", 1.0);

    b.done()
}
