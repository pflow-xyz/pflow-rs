//! Tic-tac-toe Petri net fixtures for conjecture testing.
//!
//! Two independent constructions of the same game:
//! 1. **Builder (hand-declared)**: from pflow-rs tictactoe.rs example
//! 2. **Petri-pilot (LLM-declared)**: from petri-pilot code-to-flow output
//!
//! Conjecture 4 claims these converge on the same structure.

use pflow_core::PetriNet;

/// Hand-declared TTT net matching the pflow-rs example (simplified: no turn control).
/// 27 places (9 cells x 3 states), 18 transitions (9 cells x 2 players).
pub fn builder_ttt() -> PetriNet {
    let mut b = PetriNet::build();
    for i in 0..3 {
        for j in 0..3 {
            b = b.place(&format!("p{}{}", i, j), 1.0);
            b = b.place(&format!("x{}{}", i, j), 0.0);
            b = b.place(&format!("o{}{}", i, j), 0.0);
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            b = b
                .transition(&format!("x_play_{}{}", i, j))
                .arc(&format!("p{}{}", i, j), &format!("x_play_{}{}", i, j), 1.0)
                .arc(&format!("x_play_{}{}", i, j), &format!("x{}{}", i, j), 1.0);
            b = b
                .transition(&format!("o_play_{}{}", i, j))
                .arc(&format!("p{}{}", i, j), &format!("o_play_{}{}", i, j), 1.0)
                .arc(&format!("o_play_{}{}", i, j), &format!("o{}{}", i, j), 1.0);
        }
    }
    b.done()
}

/// Petri-pilot (LLM-declared) TTT net from petri-pilot code-to-flow output.
/// 33 places, 35 transitions, 237 arcs — includes turn control, win detection, draw.
pub fn pilot_ttt() -> PetriNet {
    let mut b = PetriNet::build();

    // Cell availability places (initial=1)
    for i in 0..3 {
        for j in 0..3 {
            b = b.place(&format!("p{}{}", i, j), 1.0);
        }
    }
    // X history places
    for i in 0..3 {
        for j in 0..3 {
            b = b.place(&format!("x{}{}", i, j), 0.0);
        }
    }
    // O history places
    for i in 0..3 {
        for j in 0..3 {
            b = b.place(&format!("o{}{}", i, j), 0.0);
        }
    }
    // Control places
    b = b
        .place("x_turn", 1.0)
        .place("o_turn", 0.0)
        .place("game_active", 1.0)
        .place("move_tokens", 0.0)
        .place("win_x", 0.0)
        .place("win_o", 0.0);

    // X play transitions
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

    // O play transitions
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

    // Win detection patterns
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
        // X win transition: catalytic read of x cells, consume game_active + o_turn
        let tx = format!("x_win_{}", name);
        b = b.transition(&tx);
        for &(r, c) in cells {
            b = b
                .arc(&format!("x{}{}", r, c), &tx, 1.0)
                .arc(&tx, &format!("x{}{}", r, c), 1.0); // catalytic: return token
        }
        b = b
            .arc("game_active", &tx, 1.0)
            .arc("o_turn", &tx, 1.0)
            .arc(&tx, "win_x", 1.0);

        // O win transition: catalytic read of o cells, consume game_active + x_turn
        let to = format!("o_win_{}", name);
        b = b.transition(&to);
        for &(r, c) in cells {
            b = b
                .arc(&format!("o{}{}", r, c), &to, 1.0)
                .arc(&to, &format!("o{}{}", r, c), 1.0); // catalytic
        }
        b = b
            .arc("game_active", &to, 1.0)
            .arc("x_turn", &to, 1.0)
            .arc(&to, "win_o", 1.0);
    }

    // Draw transition: all 9 moves made, no winner
    b = b
        .transition("draw")
        .arc("move_tokens", "draw", 9.0)
        .arc("game_active", "draw", 1.0)
        .arc("draw", "win_o", 1.0); // signal game over

    b.done()
}

/// Builder TTT with win detection (matching pilot topology).
/// Adds turn control and win transitions to the basic builder net.
pub fn builder_ttt_full() -> PetriNet {
    let mut b = PetriNet::build();

    // Same cell places as pilot
    for i in 0..3 {
        for j in 0..3 {
            b = b.place(&format!("p{}{}", i, j), 1.0);
            b = b.place(&format!("x{}{}", i, j), 0.0);
            b = b.place(&format!("o{}{}", i, j), 0.0);
        }
    }

    // Control
    b = b
        .place("x_turn", 1.0)
        .place("o_turn", 0.0)
        .place("game_active", 1.0)
        .place("move_tokens", 0.0)
        .place("win_x", 0.0)
        .place("win_o", 0.0);

    // Play transitions (same as pilot)
    for i in 0..3 {
        for j in 0..3 {
            let tx = format!("x_play_{}{}", i, j);
            b = b
                .transition(&tx)
                .arc(&format!("p{}{}", i, j), &tx, 1.0)
                .arc("x_turn", &tx, 1.0)
                .arc(&tx, &format!("x{}{}", i, j), 1.0)
                .arc(&tx, "o_turn", 1.0)
                .arc(&tx, "move_tokens", 1.0);

            let to = format!("o_play_{}{}", i, j);
            b = b
                .transition(&to)
                .arc(&format!("p{}{}", i, j), &to, 1.0)
                .arc("o_turn", &to, 1.0)
                .arc(&to, &format!("o{}{}", i, j), 1.0)
                .arc(&to, "x_turn", 1.0)
                .arc(&to, "move_tokens", 1.0);
        }
    }

    // Win patterns (same structure as pilot)
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
        let tx = format!("x_win_{}", name);
        b = b.transition(&tx);
        for &(r, c) in cells {
            b = b
                .arc(&format!("x{}{}", r, c), &tx, 1.0)
                .arc(&tx, &format!("x{}{}", r, c), 1.0);
        }
        b = b
            .arc("game_active", &tx, 1.0)
            .arc("o_turn", &tx, 1.0)
            .arc(&tx, "win_x", 1.0);

        let to = format!("o_win_{}", name);
        b = b.transition(&to);
        for &(r, c) in cells {
            b = b
                .arc(&format!("o{}{}", r, c), &to, 1.0)
                .arc(&to, &format!("o{}{}", r, c), 1.0);
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
