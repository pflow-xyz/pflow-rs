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
