//! Conjecture 4: LLM-extracted nets converge with declared nets.
//!
//! Two genuinely independent paths to tic-tac-toe structure:
//!
//! **Path A (declaration)**: petri-pilot analyzes Go source code and produces
//! a Petri net with 33 places and 35 transitions. This net is constructed
//! from the `ttt_fixtures::pilot_ttt()` fixture.
//!
//! **Path B (extraction)**: an independent TTT game engine (`ttt_game`) that
//! knows NOTHING about Petri nets generates raw game traces. ReLU networks
//! train on these traces, and tropical factoring extracts topology from the
//! learned weights.
//!
//! The game engine and the Petri net were written independently. If the
//! ReLU-extracted structure matches the petri-pilot-declared structure,
//! that's evidence the structure is intrinsic to tic-tac-toe — not an
//! artifact of either construction method.

use pflow_tropical::relu_net::ReluNet;
use pflow_tropical::ttt_fixtures::pilot_ttt;
use pflow_tropical::ttt_game::{generate_game_traces, Turn};
use pflow_tropical::{dense_incidence, sign_pattern, support, Factor, FactorConfig};
use pflow_zk::IncidenceMatrix;

/// Verify the game engine's vector encoding matches the Petri net's place ordering.
/// This is a structural sanity check, NOT part of the convergence proof.
#[test]
fn vector_encoding_matches_place_labels() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);

    // Expected alphabetical order of 33 places:
    let expected: Vec<&str> = vec![
        "game_active", "move_tokens",
        "o00", "o01", "o02", "o10", "o11", "o12", "o20", "o21", "o22",
        "o_turn",
        "p00", "p01", "p02", "p10", "p11", "p12", "p20", "p21", "p22",
        "win_o", "win_x",
        "x00", "x01", "x02", "x10", "x11", "x12", "x20", "x21", "x22",
        "x_turn",
    ];

    assert_eq!(im.place_labels.len(), 33);
    for (i, label) in im.place_labels.iter().enumerate() {
        assert_eq!(
            label.as_str(),
            expected[i],
            "place {i}: expected '{}', got '{}'",
            expected[i],
            label
        );
    }
}

/// Helper: collect game traces for a specific transition (cell + player).
/// Returns (pre_vectors, post_vectors) from the independent game engine.
fn traces_for_transition(
    cell: usize,
    player: Turn,
    num_games: usize,
    seed: u64,
) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let all = generate_game_traces(num_games, seed);
    let mut inputs = Vec::new();
    let mut targets = Vec::new();

    for (c, p, pre, post) in &all {
        if *c == cell && *p == player {
            inputs.push(pre.clone());
            targets.push(post.clone());
        }
    }

    (inputs, targets)
}

/// Map (cell, player) to petri-pilot transition index.
fn transition_index(im: &IncidenceMatrix, cell: usize, player: Turn) -> usize {
    let row = cell / 3;
    let col = cell % 3;
    let label = match player {
        Turn::X => format!("x_play_{}{}", row, col),
        Turn::O => format!("o_play_{}{}", row, col),
    };
    im.transition_labels
        .iter()
        .position(|l| l == &label)
        .unwrap_or_else(|| panic!("transition '{}' not found", label))
}

// ──────────────────────────────────────────────────────────
// Test 1: ReLU trained on raw game data recovers sign patterns
// ──────────────────────────────────────────────────────────

#[test]
fn relu_from_game_data_recovers_signs() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let ground_truth = dense_incidence(&im);
    let n = im.num_places; // 33

    // Test representative transitions from raw game data
    let test_cases: Vec<(usize, Turn)> = vec![
        (0, Turn::X), // x_play_00
        (4, Turn::X), // x_play_11 (center)
        (8, Turn::X), // x_play_22
        (1, Turn::O), // o_play_01
        (3, Turn::O), // o_play_10
        (7, Turn::O), // o_play_21
    ];

    let mut correct = 0;
    let mut total = 0;

    for (cell, player) in &test_cases {
        let (inputs, targets) = traces_for_transition(*cell, *player, 500, 42 + *cell as u64);

        if inputs.len() < 10 {
            continue;
        }

        let mut nn = ReluNet::new(n, 48, n, 1000 + *cell as u64);
        nn.train(&inputs, &targets, 0.01, 300);

        // Extract delta from first test input
        let pred = nn.predict(&inputs[0]);
        let learned_delta: Vec<f64> = pred
            .iter()
            .zip(inputs[0].iter())
            .map(|(o, i)| o - i)
            .collect();

        let factored = vec![learned_delta].factor(&FactorConfig {
            threshold: 0.3,
            round_to_int: true,
        });

        let extracted_row = &factored.data[..n];
        let extracted_signs: Vec<i8> = extracted_row
            .iter()
            .map(|&v| {
                if v == pflow_tropical::NEG_INF || v == 0.0 {
                    0
                } else if v > 0.0 {
                    1
                } else {
                    -1
                }
            })
            .collect();

        let t_idx = transition_index(&im, *cell, *player);
        let gt_signs = sign_pattern(&ground_truth[t_idx]);

        for (g, e) in gt_signs.iter().zip(extracted_signs.iter()) {
            if g == e {
                correct += 1;
            }
            total += 1;
        }
    }

    let accuracy = correct as f64 / total as f64;
    assert!(
        accuracy >= 0.90,
        "sign accuracy from game data = {:.1}% (need >= 90%)",
        accuracy * 100.0
    );
}

// ──────────────────────────────────────────────────────────
// Test 2: Cell conservation emerges from game data
// ──────────────────────────────────────────────────────────

#[test]
fn conservation_emerges_from_game_data() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let n = im.num_places;

    // Train on X center play from raw game data
    let (inputs, targets) = traces_for_transition(4, Turn::X, 500, 200);
    assert!(inputs.len() >= 10);

    let mut nn = ReluNet::new(n, 48, n, 2000);
    nn.train(&inputs, &targets, 0.01, 400);

    let pred = nn.predict(&inputs[0]);
    let delta: Vec<f64> = pred
        .iter()
        .zip(inputs[0].iter())
        .map(|(o, i)| o - i)
        .collect();

    // Cell conservation: for each cell, delta[p] + delta[x] + delta[o] ≈ 0
    // The ReLU learns this from game rules, not from Petri net structure
    for i in 0..3 {
        for j in 0..3 {
            let cell = i * 3 + j;
            let p_idx = 12 + cell; // p00..p22
            let x_idx = 23 + cell; // x00..x22
            let o_idx = 2 + cell;  // o00..o22

            let sum = delta[p_idx] + delta[x_idx] + delta[o_idx];
            assert!(
                sum.abs() < 0.5,
                "cell ({},{}): conservation violated from game data: sum={:.3}",
                i, j, sum
            );
        }
    }
}

// ──────────────────────────────────────────────────────────
// Test 3: Turn alternation emerges from game data
// ──────────────────────────────────────────────────────────

#[test]
fn turn_alternation_from_game_data() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let n = im.num_places;

    let x_turn_idx = 32; // x_turn
    let o_turn_idx = 11; // o_turn

    // X plays center — trained on game data, not Petri net firings
    let (inputs_x, targets_x) = traces_for_transition(4, Turn::X, 500, 300);
    assert!(inputs_x.len() >= 10);

    let mut nn_x = ReluNet::new(n, 48, n, 3000);
    nn_x.train(&inputs_x, &targets_x, 0.01, 400);

    let pred_x = nn_x.predict(&inputs_x[0]);
    let delta_x: Vec<f64> = pred_x
        .iter()
        .zip(inputs_x[0].iter())
        .map(|(o, i)| o - i)
        .collect();

    assert!(
        delta_x[x_turn_idx] < -0.3,
        "X play should consume x_turn from game data, got {:.3}",
        delta_x[x_turn_idx]
    );
    assert!(
        delta_x[o_turn_idx] > 0.3,
        "X play should produce o_turn from game data, got {:.3}",
        delta_x[o_turn_idx]
    );

    // O plays corner — opposite turn effect
    let (inputs_o, targets_o) = traces_for_transition(0, Turn::O, 500, 301);
    if inputs_o.len() >= 10 {
        let mut nn_o = ReluNet::new(n, 48, n, 3001);
        nn_o.train(&inputs_o, &targets_o, 0.01, 400);

        let pred_o = nn_o.predict(&inputs_o[0]);
        let delta_o: Vec<f64> = pred_o
            .iter()
            .zip(inputs_o[0].iter())
            .map(|(o, i)| o - i)
            .collect();

        assert!(
            delta_o[o_turn_idx] < -0.3,
            "O play should consume o_turn from game data, got {:.3}",
            delta_o[o_turn_idx]
        );
        assert!(
            delta_o[x_turn_idx] > 0.3,
            "O play should produce x_turn from game data, got {:.3}",
            delta_o[x_turn_idx]
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 4: Move counter emerges from game data
// ──────────────────────────────────────────────────────────

#[test]
fn move_counter_from_game_data() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let n = im.num_places;

    let mt_idx = 1; // move_tokens

    let test_cases: Vec<(usize, Turn)> = vec![
        (4, Turn::X),
        (0, Turn::X),
        (1, Turn::O),
        (3, Turn::O),
    ];

    let mut found_positive = 0;
    for (cell, player) in &test_cases {
        let (inputs, targets) = traces_for_transition(*cell, *player, 500, 400 + *cell as u64);
        if inputs.len() < 10 {
            continue;
        }

        let mut nn = ReluNet::new(n, 48, n, 4000 + *cell as u64);
        nn.train(&inputs, &targets, 0.01, 300);

        let pred = nn.predict(&inputs[0]);
        let delta: Vec<f64> = pred
            .iter()
            .zip(inputs[0].iter())
            .map(|(o, i)| o - i)
            .collect();

        if delta[mt_idx] > 0.3 {
            found_positive += 1;
        }
    }

    assert!(
        found_positive >= 3,
        "at least 3/4 transitions should increment move_tokens, got {}",
        found_positive
    );
}

// ──────────────────────────────────────────────────────────
// Test 5: Arc support recovery from game data
// ──────────────────────────────────────────────────────────

#[test]
fn arc_support_from_game_data() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let ground_truth = dense_incidence(&im);
    let n = im.num_places;

    let test_cases: Vec<(usize, Turn)> = vec![
        (4, Turn::X), // center
        (0, Turn::O), // corner
        (2, Turn::X), // corner
    ];

    for (cell, player) in &test_cases {
        let (inputs, targets) = traces_for_transition(*cell, *player, 800, 500 + *cell as u64);
        if inputs.len() < 10 {
            continue;
        }

        let mut nn = ReluNet::new(n, 64, n, 5000 + *cell as u64);
        nn.train(&inputs, &targets, 0.008, 500);

        let pred = nn.predict(&inputs[0]);
        let delta: Vec<f64> = pred
            .iter()
            .zip(inputs[0].iter())
            .map(|(o, i)| o - i)
            .collect();

        let t_idx = transition_index(&im, *cell, *player);
        let gt_support = support(&ground_truth[t_idx]);

        let learned_support: Vec<usize> = delta
            .iter()
            .enumerate()
            .filter(|(_, &d)| d.abs() > 0.2)
            .map(|(i, _)| i)
            .collect();

        // Count how many ground truth arcs are recovered
        let recovered: Vec<usize> = gt_support
            .iter()
            .filter(|p| learned_support.contains(p))
            .copied()
            .collect();
        let recall = recovered.len() as f64 / gt_support.len() as f64;
        assert!(
            recall >= 0.8,
            "cell {} {:?}: arc recall = {:.0}% ({}/{}), missing: {:?}",
            cell,
            player,
            recall * 100.0,
            recovered.len(),
            gt_support.len(),
            gt_support
                .iter()
                .filter(|p| !learned_support.contains(p))
                .map(|&p| &im.place_labels[p])
                .collect::<Vec<_>>()
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 6: Overall structural similarity from game data
// ──────────────────────────────────────────────────────────

#[test]
fn overall_similarity_from_game_data() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let ground_truth = dense_incidence(&im);
    let n = im.num_places;

    let test_cases: Vec<(usize, Turn)> = vec![
        (0, Turn::X), (4, Turn::X), (8, Turn::X), (2, Turn::X),
        (1, Turn::O), (3, Turn::O), (5, Turn::O), (7, Turn::O),
    ];

    let mut total_correct = 0;
    let mut total_entries = 0;
    let mut tested = 0;

    for (cell, player) in &test_cases {
        let (inputs, targets) = traces_for_transition(*cell, *player, 500, 600 + *cell as u64);
        if inputs.len() < 10 {
            continue;
        }

        let mut nn = ReluNet::new(n, 48, n, 6000 + *cell as u64);
        nn.train(&inputs, &targets, 0.01, 300);

        let pred = nn.predict(&inputs[0]);
        let delta: Vec<f64> = pred
            .iter()
            .zip(inputs[0].iter())
            .map(|(o, i)| o - i)
            .collect();

        let rounded: Vec<i64> = delta
            .iter()
            .map(|&d| if d.abs() < 0.3 { 0 } else { d.round() as i64 })
            .collect();

        let t_idx = transition_index(&im, *cell, *player);
        let gt_signs = sign_pattern(&ground_truth[t_idx]);
        let ex_signs = sign_pattern(&rounded);

        for (g, e) in gt_signs.iter().zip(ex_signs.iter()) {
            if g == e {
                total_correct += 1;
            }
            total_entries += 1;
        }
        tested += 1;
    }

    assert!(tested >= 6, "need >= 6 transitions tested, got {}", tested);

    let similarity = total_correct as f64 / total_entries as f64;
    assert!(
        similarity >= 0.90,
        "structural similarity from game data = {:.1}% across {} transitions (need >= 90%)",
        similarity * 100.0,
        tested
    );
}
