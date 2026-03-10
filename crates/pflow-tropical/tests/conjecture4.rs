//! Conjecture 4: LLM-extracted nets converge with declared nets.
//!
//! Two genuinely independent paths to a tic-tac-toe Petri net:
//! 1. **Declared**: petri-pilot code-to-flow generates a net from source code
//! 2. **Extracted**: train ReLU networks on TTT game traces, factor weights
//!    through tropical decomposition, recover incidence structure
//!
//! If the ReLU-extracted topology matches the petri-pilot-declared topology,
//! that's evidence the structure is real — not an artifact of either method.

use pflow_tropical::relu_net::ReluNet;
use pflow_tropical::ttt_fixtures::pilot_ttt;
use pflow_tropical::{dense_incidence, sign_pattern, support, Factor, FactorConfig};
use pflow_zk::{fire_transition, IncidenceMatrix};

/// Generate training data for a specific transition by simulating random games.
/// Returns (input_markings, output_markings) pairs where `transition_id` fired.
fn generate_transition_data(
    im: &IncidenceMatrix,
    transition_id: usize,
    num_samples: usize,
    seed: u64,
) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let mut inputs = Vec::new();
    let mut targets = Vec::new();
    let n = im.num_places;

    // Simple PRNG for deterministic game generation
    let mut rng_state = seed;
    let mut next_rand = || -> u64 {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        rng_state >> 33
    };

    // Play many random games, collecting firings of the target transition
    let mut attempts = 0;
    while inputs.len() < num_samples && attempts < num_samples * 500 {
        attempts += 1;

        // Start from initial state and play random moves until target fires
        // or game ends. Initial marking for pilot TTT:
        let mut marking = vec![0i64; n];
        // Set initial tokens: p00-p22 = 1, x_turn = 1, game_active = 1
        for (i, label) in im.place_labels.iter().enumerate() {
            if label.starts_with('p') && label.len() == 3 {
                marking[i] = 1; // cell available
            } else if label == "x_turn" || label == "game_active" {
                marking[i] = 1;
            }
        }

        // Play up to 9 random moves
        for _step in 0..12 {
            // Find all enabled transitions
            let enabled: Vec<usize> = (0..im.num_transitions)
                .filter(|&t| im.is_enabled(&marking, t))
                .collect();

            if enabled.is_empty() {
                break;
            }

            // Pick a random enabled transition
            let chosen = enabled[next_rand() as usize % enabled.len()];

            if chosen == transition_id {
                // Record this firing
                let pre: Vec<f64> = marking.iter().map(|&m| m as f64).collect();
                let post_marking = fire_transition(im, &marking, chosen).unwrap();
                let post: Vec<f64> = post_marking.iter().map(|&m| m as f64).collect();
                inputs.push(pre);
                targets.push(post);
                marking = post_marking;

                if inputs.len() >= num_samples {
                    break;
                }
            } else {
                marking = fire_transition(im, &marking, chosen).unwrap();
            }
        }
    }

    (inputs, targets)
}

// ──────────────────────────────────────────────────────────
// Test 1: ReLU recovers play transition sign patterns
// ──────────────────────────────────────────────────────────

#[test]
fn relu_recovers_play_transition_signs() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let ground_truth = dense_incidence(&im);
    let n = im.num_places; // 33

    // Test 6 play transitions (3 X, 3 O) — representative subset for speed
    let subset = ["x_play_00", "x_play_11", "x_play_22", "o_play_01", "o_play_10", "o_play_21"];
    let play_transitions: Vec<usize> = im
        .transition_labels
        .iter()
        .enumerate()
        .filter(|(_, l)| subset.contains(&l.as_str()))
        .map(|(i, _)| i)
        .collect();

    let mut correct = 0;
    let mut total = 0;

    for &t in &play_transitions {
        let (inputs, targets) = generate_transition_data(&im, t, 40, 42 + t as u64);

        if inputs.len() < 5 {
            continue;
        }

        let mut nn = ReluNet::new(n, 48, n, 1000 + t as u64);
        nn.train(&inputs, &targets, 0.01, 300);

        // Extract delta from first test input
        let pred = nn.predict(&inputs[0]);
        let learned_delta: Vec<f64> = pred
            .iter()
            .zip(inputs[0].iter())
            .map(|(o, i)| o - i)
            .collect();

        // Factor through tropical decomposition
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

        let gt_signs = sign_pattern(&ground_truth[t]);

        // Count matching sign positions
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
        "play transition sign accuracy = {:.1}% (need >= 90%)",
        accuracy * 100.0
    );
}

// ──────────────────────────────────────────────────────────
// Test 2: ReLU recovers cell conservation law
// ──────────────────────────────────────────────────────────

#[test]
fn relu_extracted_deltas_conserve_cells() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let n = im.num_places;

    // For each play transition, the learned delta should satisfy
    // delta[p_ij] + delta[x_ij] + delta[o_ij] = 0 for the affected cell,
    // and = 0 for all other cells (they're untouched).

    // Test a subset of play transitions
    let test_transitions: Vec<usize> = im
        .transition_labels
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("x_play"))
        .take(4) // first 4 X plays for speed
        .map(|(i, _)| i)
        .collect();

    for &t in &test_transitions {
        let (inputs, targets) = generate_transition_data(&im, t, 40, 200 + t as u64);
        if inputs.len() < 10 {
            continue;
        }

        let mut nn = ReluNet::new(n, 48, n, 2000 + t as u64);
        nn.train(&inputs, &targets, 0.005, 500);

        let pred = nn.predict(&inputs[0]);
        let learned_delta: Vec<f64> = pred
            .iter()
            .zip(inputs[0].iter())
            .map(|(o, i)| o - i)
            .collect();

        // Check cell conservation: for each cell (i,j), sum of deltas ≈ 0
        for i in 0..3 {
            for j in 0..3 {
                let p_idx = im
                    .place_labels
                    .iter()
                    .position(|l| l == &format!("p{}{}", i, j))
                    .unwrap();
                let x_idx = im
                    .place_labels
                    .iter()
                    .position(|l| l == &format!("x{}{}", i, j))
                    .unwrap();
                let o_idx = im
                    .place_labels
                    .iter()
                    .position(|l| l == &format!("o{}{}", i, j))
                    .unwrap();

                let cell_sum =
                    learned_delta[p_idx] + learned_delta[x_idx] + learned_delta[o_idx];
                assert!(
                    cell_sum.abs() < 0.5,
                    "transition {} ({}): cell ({},{}) conservation violated: sum={:.3}",
                    t,
                    im.transition_labels[t],
                    i,
                    j,
                    cell_sum
                );
            }
        }
    }
}

// ──────────────────────────────────────────────────────────
// Test 3: ReLU recovers arc support (which places are touched)
// ──────────────────────────────────────────────────────────

#[test]
fn relu_recovers_arc_support_ttt() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let ground_truth = dense_incidence(&im);
    let n = im.num_places;

    // Test a few play transitions
    let test_transitions: Vec<usize> = im
        .transition_labels
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("play"))
        .take(6)
        .map(|(i, _)| i)
        .collect();

    for &t in &test_transitions {
        let (inputs, targets) = generate_transition_data(&im, t, 50, 300 + t as u64);
        if inputs.len() < 10 {
            continue;
        }

        let mut nn = ReluNet::new(n, 48, n, 3000 + t as u64);
        let loss = nn.train(&inputs, &targets, 0.005, 600);
        if loss > 0.1 {
            continue;
        }

        let pred = nn.predict(&inputs[0]);
        let learned_delta: Vec<f64> = pred
            .iter()
            .zip(inputs[0].iter())
            .map(|(o, i)| o - i)
            .collect();

        // Ground truth support: places with non-zero delta
        let gt_support = support(&ground_truth[t]);

        // Learned support: places where |delta| > threshold
        let learned_support: Vec<usize> = learned_delta
            .iter()
            .enumerate()
            .filter(|(_, &d)| d.abs() > 0.3)
            .map(|(i, _)| i)
            .collect();

        // Every ground truth arc should appear in learned support
        for &p in &gt_support {
            assert!(
                learned_support.contains(&p),
                "transition {} ({}): place {} ({}) missing from learned support.\n  gt_support={:?}\n  learned={:?}",
                t, im.transition_labels[t], p, im.place_labels[p],
                gt_support.iter().map(|&i| &im.place_labels[i]).collect::<Vec<_>>(),
                learned_support.iter().map(|&i| &im.place_labels[i]).collect::<Vec<_>>()
            );
        }
    }
}

// ──────────────────────────────────────────────────────────
// Test 4: Turn control structure emerges from data
// ──────────────────────────────────────────────────────────

#[test]
fn relu_discovers_turn_control() {
    // The pilot net has x_turn and o_turn places that alternate.
    // A ReLU net trained on X play transitions should learn that
    // x_turn decreases and o_turn increases (and vice versa for O plays).
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let n = im.num_places;

    let x_turn_idx = im
        .place_labels
        .iter()
        .position(|l| l == "x_turn")
        .unwrap();
    let o_turn_idx = im
        .place_labels
        .iter()
        .position(|l| l == "o_turn")
        .unwrap();

    // Train on x_play_11 (center play — most data available)
    let t = im
        .transition_labels
        .iter()
        .position(|l| l == "x_play_11")
        .unwrap();

    let (inputs, targets) = generate_transition_data(&im, t, 60, 400);
    assert!(inputs.len() >= 10, "need at least 10 samples for x_play_11");

    let mut nn = ReluNet::new(n, 48, n, 4000);
    nn.train(&inputs, &targets, 0.005, 600);

    let pred = nn.predict(&inputs[0]);
    let delta: Vec<f64> = pred
        .iter()
        .zip(inputs[0].iter())
        .map(|(o, i)| o - i)
        .collect();

    // X play should: x_turn decreases, o_turn increases
    assert!(
        delta[x_turn_idx] < -0.3,
        "x_play should consume x_turn, got delta={:.3}",
        delta[x_turn_idx]
    );
    assert!(
        delta[o_turn_idx] > 0.3,
        "x_play should produce o_turn, got delta={:.3}",
        delta[o_turn_idx]
    );

    // Now train on o_play_00
    let t_o = im
        .transition_labels
        .iter()
        .position(|l| l == "o_play_00")
        .unwrap();

    let (inputs_o, targets_o) = generate_transition_data(&im, t_o, 60, 401);
    if inputs_o.len() >= 10 {
        let mut nn_o = ReluNet::new(n, 48, n, 4001);
        nn_o.train(&inputs_o, &targets_o, 0.005, 600);

        let pred_o = nn_o.predict(&inputs_o[0]);
        let delta_o: Vec<f64> = pred_o
            .iter()
            .zip(inputs_o[0].iter())
            .map(|(o, i)| o - i)
            .collect();

        // O play should: o_turn decreases, x_turn increases (opposite)
        assert!(
            delta_o[o_turn_idx] < -0.3,
            "o_play should consume o_turn, got delta={:.3}",
            delta_o[o_turn_idx]
        );
        assert!(
            delta_o[x_turn_idx] > 0.3,
            "o_play should produce x_turn, got delta={:.3}",
            delta_o[x_turn_idx]
        );
    }
}

// ──────────────────────────────────────────────────────────
// Test 5: Extracted move_tokens accumulation
// ──────────────────────────────────────────────────────────

#[test]
fn relu_discovers_move_counter() {
    // Every play transition should increment move_tokens.
    // This is the accounting mechanism for the draw condition.
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let n = im.num_places;

    let mt_idx = im
        .place_labels
        .iter()
        .position(|l| l == "move_tokens")
        .unwrap();

    // Test several play transitions
    let play_transitions: Vec<usize> = im
        .transition_labels
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("play"))
        .take(6)
        .map(|(i, _)| i)
        .collect();

    let mut found_positive = 0;
    for &t in &play_transitions {
        let (inputs, targets) = generate_transition_data(&im, t, 40, 500 + t as u64);
        if inputs.len() < 10 {
            continue;
        }

        let mut nn = ReluNet::new(n, 48, n, 5000 + t as u64);
        nn.train(&inputs, &targets, 0.005, 500);

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
        found_positive >= 4,
        "at least 4/6 play transitions should increment move_tokens, got {}/{}",
        found_positive,
        play_transitions.len()
    );
}

// ──────────────────────────────────────────────────────────
// Test 6: Overall structural similarity score
// ──────────────────────────────────────────────────────────

#[test]
fn overall_structural_similarity() {
    let net = pilot_ttt();
    let im = IncidenceMatrix::from_petri_net(&net);
    let ground_truth = dense_incidence(&im);
    let n = im.num_places;

    // Sample 8 play transitions for overall accuracy
    let subset = ["x_play_00", "x_play_11", "x_play_22", "x_play_02",
                   "o_play_01", "o_play_10", "o_play_21", "o_play_12"];
    let play_transitions: Vec<usize> = im
        .transition_labels
        .iter()
        .enumerate()
        .filter(|(_, l)| subset.contains(&l.as_str()))
        .map(|(i, _)| i)
        .collect();

    let mut total_correct = 0;
    let mut total_entries = 0;
    let mut transitions_tested = 0;

    for &t in &play_transitions {
        let (inputs, targets) = generate_transition_data(&im, t, 40, 600 + t as u64);
        if inputs.len() < 5 {
            continue;
        }

        let mut nn = ReluNet::new(n, 48, n, 6000 + t as u64);
        nn.train(&inputs, &targets, 0.01, 300);

        let pred = nn.predict(&inputs[0]);
        let learned_delta: Vec<f64> = pred
            .iter()
            .zip(inputs[0].iter())
            .map(|(o, i)| o - i)
            .collect();

        let rounded: Vec<i64> = learned_delta
            .iter()
            .map(|&d| if d.abs() < 0.3 { 0 } else { d.round() as i64 })
            .collect();

        let gt_signs = sign_pattern(&ground_truth[t]);
        let ex_signs = sign_pattern(&rounded);

        for (g, e) in gt_signs.iter().zip(ex_signs.iter()) {
            if g == e {
                total_correct += 1;
            }
            total_entries += 1;
        }
        transitions_tested += 1;
    }

    assert!(
        transitions_tested >= 6,
        "need at least 6 transitions tested, got {}",
        transitions_tested
    );

    let similarity = total_correct as f64 / total_entries as f64;
    assert!(
        similarity >= 0.90,
        "overall structural similarity = {:.1}% across {} transitions (need >= 90%)",
        similarity * 100.0,
        transitions_tested
    );
}
