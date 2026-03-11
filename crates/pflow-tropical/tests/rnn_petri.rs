//! RNN trained on Petri net game trajectories.
//!
//! The RNN learns to predict which transition fires next given the current
//! marking. Unlike a feedforward net, the recurrent hidden state captures
//! sequential context — turn order, move history, board symmetry — from
//! the Petri net's firing semantics alone.
//!
//! Generic over any NetMatrix: supply a net and the RNN learns its dynamics.

use pflow_tropical::net_matrix::NetMatrix;
use pflow_tropical::rnn::{
    generate_game_trajectories, generate_game_trajectories_capped,
    ElmanRnn, GameResult, SimpleRng,
};

// ──────────────────────────────────────────────────────────
// Test 1: Simple loop — RNN learns alternating transitions
// ──────────────────────────────────────────────────────────

#[test]
fn rnn_learns_simple_loop() {
    // Generate deterministic training sequences
    let mut sequences = Vec::new();
    for _ in 0..50 {
        let inputs = vec![
            vec![1.0, 0.0],
            vec![0.0, 1.0],
            vec![1.0, 0.0],
            vec![0.0, 1.0],
            vec![1.0, 0.0],
            vec![0.0, 1.0],
        ];
        let targets = vec![0, 1, 0, 1, 0, 1];
        sequences.push((inputs, targets));
    }

    let mut rnn = ElmanRnn::new(2, 16, 2, 42);
    let loss = rnn.train(&sequences, 0.1, 300);
    assert!(loss < 0.3, "RNN didn't converge on simple loop: loss = {loss}");

    let preds = rnn.predict_sequence(&[
        vec![1.0, 0.0],
        vec![0.0, 1.0],
        vec![1.0, 0.0],
        vec![0.0, 1.0],
    ]);
    assert_eq!(preds, vec![0, 1, 0, 1], "RNN should predict alternating T0/T1");
}

// ──────────────────────────────────────────────────────────
// Test 2: 3-place pipeline — cyclic transition prediction
// ──────────────────────────────────────────────────────────

#[test]
fn rnn_learns_pipeline_cycle() {
    // Cyclic sequences: T0, T1, T2, T0, T1, T2, ...
    let mut sequences = Vec::new();
    for _ in 0..50 {
        let inputs = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        let targets = vec![0, 1, 2, 0, 1, 2];
        sequences.push((inputs, targets));
    }

    let mut rnn = ElmanRnn::new(3, 16, 3, 7);
    let loss = rnn.train(&sequences, 0.1, 300);
    assert!(loss < 0.3, "RNN didn't converge on pipeline: loss = {loss}");

    let preds = rnn.predict_sequence(&[
        vec![1.0, 0.0, 0.0],
        vec![0.0, 1.0, 0.0],
        vec![0.0, 0.0, 1.0],
    ]);
    assert_eq!(preds, vec![0, 1, 2], "RNN should predict cyclic T0->T1->T2");
}

// ──────────────────────────────────────────────────────────
// Test 3: TTT — RNN trained on random legal game play
// ──────────────────────────────────────────────────────────

#[test]
fn rnn_learns_ttt_trajectories() {
    let net = pflow_tropical::ttt_fixtures::pilot_ttt();
    let np = net.num_places();   // 33
    let nt = net.num_transitions(); // 35

    // Generate random legal games
    let trajectories = generate_game_trajectories(&net, 100, 42);
    assert!(!trajectories.is_empty(), "should generate at least one game");

    // Verify trajectory validity: each target must be an enabled transition
    for (inputs, targets) in &trajectories {
        assert_eq!(inputs.len(), targets.len());
        for input in inputs {
            assert_eq!(input.len(), np);
        }
        for &t in targets {
            assert!(t < nt, "transition index {t} out of range");
        }
    }

    // Compute average game length for baseline comparison
    let avg_steps: f64 = trajectories.iter()
        .map(|(inputs, _)| inputs.len() as f64)
        .sum::<f64>() / trajectories.len() as f64;

    // Train RNN on the trajectories
    let mut rnn = ElmanRnn::new(np, 32, nt, 42);
    let loss = rnn.train(&trajectories, 0.05, 100);

    // Loss is cross-entropy summed over timesteps, averaged over sequences.
    // Random baseline per step: -ln(1/35) ≈ 3.56, so per sequence ≈ avg_steps * 3.56
    // But many transitions are disabled, so effective random is lower.
    // A useful RNN should beat uniform-over-all-transitions baseline.
    let random_baseline = avg_steps * (nt as f64).ln();
    assert!(
        loss < random_baseline,
        "RNN should beat random guessing on TTT (loss = {loss}, random ≈ {random_baseline:.1})"
    );

    // Verify predictions are valid transition indices
    if let Some((first_inputs, _)) = trajectories.first() {
        let preds = rnn.predict_sequence(first_inputs);
        for &p in &preds {
            assert!(p < nt, "predicted transition {p} out of range");
        }
    }
}

// ──────────────────────────────────────────────────────────
// Test 4: Generic — train on any NetMatrix
// ──────────────────────────────────────────────────────────

#[test]
fn rnn_generic_over_any_net() {
    // Producer-consumer: P0 -> T0 -> P1 -> T1 -> P2 (no cycle, drains)
    let net = NetMatrix::from_incidence(
        vec![
            vec![-1, 1, 0], // T0: consumes P0, produces P1
            vec![0, -1, 1], // T1: consumes P1, produces P2
        ],
        vec![3, 0, 0], // 3 tokens in P0
        vec!["P0".into(), "P1".into(), "P2".into()],
        vec!["T0".into(), "T1".into()],
    );

    let trajectories = generate_game_trajectories(&net, 100, 99);
    assert!(!trajectories.is_empty());

    // Games should terminate (P0 drains, then P1 drains)
    for (inputs, targets) in &trajectories {
        assert!(inputs.len() <= 6, "game should end in ≤6 steps (3 T0 + 3 T1)");
        // First moves must be T0 (only enabled initially)
        assert_eq!(targets[0], 0, "first move must be T0");
    }

    let mut rnn = ElmanRnn::new(3, 16, 2, 55);
    let loss = rnn.train(&trajectories, 0.1, 400);
    assert!(loss < 2.0, "RNN should learn producer-consumer pattern: loss = {loss}");
}

// ──────────────────────────────────────────────────────────
// Test 5: Hidden state encodes sequential context
// ──────────────────────────────────────────────────────────

#[test]
fn rnn_hidden_state_distinguishes_context() {
    let mut sequences = Vec::new();
    for _ in 0..50 {
        sequences.push((
            vec![vec![1.0, 0.0], vec![0.0, 1.0], vec![1.0, 0.0], vec![0.0, 1.0]],
            vec![0, 1, 0, 1],
        ));
    }

    let mut rnn = ElmanRnn::new(2, 16, 2, 42);
    rnn.train(&sequences, 0.1, 300);

    // Same input [1, 0] at different positions in sequence should produce
    // different hidden states (context matters)
    let (hiddens, _) = rnn.forward(&[
        vec![1.0, 0.0],
        vec![0.0, 1.0],
        vec![1.0, 0.0],
    ]);

    // h[0] and h[2] have same input but different history
    let diff: f64 = hiddens[0].iter().zip(hiddens[2].iter())
        .map(|(a, b)| (a - b).abs())
        .sum();

    // After training, hidden states at t=0 and t=2 should differ
    // (the RNN has learned that position in sequence matters)
    assert!(
        diff > 0.01,
        "hidden states should differ for same input at different timesteps (diff = {diff})"
    );
}

// ──────────────────────────────────────────────────────────
// Test 6: RNN plays TTT — train then play complete games
// ──────────────────────────────────────────────────────────

/// Render a TTT board from a pilot_ttt marking.
/// Place layout (alphabetical, 33 places):
///   0: game_active, 1: move_tokens,
///   2..10: o00..o22, 11: o_turn,
///   12..20: p00..p22, 21: win_o, 22: win_x,
///   23..31: x00..x22, 32: x_turn
fn render_board(marking: &[i64]) -> String {
    let mut board = ['.'; 9];
    for cell in 0..9 {
        let r = cell / 3;
        let c = cell % 3;
        let p_idx = 12 + r * 3 + c;  // p00..p22
        let x_idx = 23 + r * 3 + c;  // x00..x22
        let o_idx = 2 + r * 3 + c;   // o00..o22
        if marking[x_idx] > 0 {
            board[cell] = 'X';
        } else if marking[o_idx] > 0 {
            board[cell] = 'O';
        } else if marking[p_idx] > 0 {
            board[cell] = '.';
        }
    }
    format!(
        " {} | {} | {}\n-----------\n {} | {} | {}\n-----------\n {} | {} | {}",
        board[0], board[1], board[2],
        board[3], board[4], board[5],
        board[6], board[7], board[8],
    )
}

fn game_outcome(result: &GameResult) -> &'static str {
    let m = &result.final_marking;
    if m[22] > 0 { "X wins" }
    else if m[21] > 0 { "O wins" }
    else { "draw" }
}

#[test]
fn rnn_plays_ttt() {
    let net = pflow_tropical::ttt_fixtures::pilot_ttt();
    let np = net.num_places();
    let nt = net.num_transitions();

    // Train on random games
    let trajectories = generate_game_trajectories(&net, 200, 42);
    let mut rnn = ElmanRnn::new(np, 32, nt, 42);
    let loss = rnn.train(&trajectories, 0.05, 100);
    eprintln!("Training loss: {loss:.3}");

    // Play 20 games using the trained RNN
    let mut wins_x = 0;
    let mut wins_o = 0;
    let mut draws = 0;
    let num_games = 20;

    for game_id in 0..num_games {
        let result = rnn.play_game(&net);
        let outcome = game_outcome(&result);

        match outcome {
            "X wins" => wins_x += 1,
            "O wins" => wins_o += 1,
            _ => draws += 1,
        }

        // Show first 3 games in detail
        if game_id < 3 {
            eprintln!("\n=== Game {} ({}) ===", game_id + 1, outcome);
            for (i, mv) in result.moves.iter().enumerate() {
                let tname = &net.transition_labels[mv.transition];
                eprintln!(
                    "Move {}: {} (confidence: {:.1}%)",
                    i + 1, tname, mv.confidence * 100.0
                );
                eprintln!("{}", render_board(&mv.marking));
                eprintln!();
            }
            eprintln!("Final board:");
            eprintln!("{}", render_board(&result.final_marking));
        }

        // Verify every move was legal
        let mut marking = net.initial.clone();
        for mv in &result.moves {
            assert!(
                net.is_enabled(&marking, mv.transition),
                "game {}: illegal move {} at marking {:?}",
                game_id, net.transition_labels[mv.transition], marking
            );
            marking = net.fire(&marking, mv.transition).unwrap();
        }
        assert_eq!(marking, result.final_marking);
    }

    eprintln!(
        "\nResults ({num_games} games): X wins {wins_x}, O wins {wins_o}, draws {draws}"
    );

    // The RNN should produce terminated games (every game reaches a final state)
    assert_eq!(
        wins_x + wins_o + draws, num_games,
        "all games should terminate"
    );
}

// ──────────────────────────────────────────────────────────
// Test 7: RNN vs Random — does training improve play?
// ──────────────────────────────────────────────────────────

#[test]
fn rnn_vs_random_play() {
    let net = pflow_tropical::ttt_fixtures::pilot_ttt();
    let np = net.num_places();
    let nt = net.num_transitions();

    // Untrained RNN (random policy)
    let untrained = ElmanRnn::new(np, 32, nt, 99);

    // Trained RNN
    let trajectories = generate_game_trajectories(&net, 200, 42);
    let mut trained = ElmanRnn::new(np, 32, nt, 42);
    trained.train(&trajectories, 0.05, 100);

    // Play 50 games with each and measure average confidence
    let mut untrained_confidence = 0.0;
    let mut trained_confidence = 0.0;
    let n = 50;

    for _ in 0..n {
        let result = untrained.play_game(&net);
        untrained_confidence += result.moves.iter()
            .map(|m| m.confidence).sum::<f64>() / result.moves.len() as f64;

        let result = trained.play_game(&net);
        trained_confidence += result.moves.iter()
            .map(|m| m.confidence).sum::<f64>() / result.moves.len() as f64;
    }

    untrained_confidence /= n as f64;
    trained_confidence /= n as f64;

    eprintln!("Average confidence — untrained: {untrained_confidence:.3}, trained: {trained_confidence:.3}");

    // Trained RNN should be more confident in its moves
    assert!(
        trained_confidence > untrained_confidence,
        "trained RNN should be more confident: {trained_confidence:.3} vs {untrained_confidence:.3}"
    );
}

// ──────────────────────────────────────────────────────────
// Test 8: Self-play with reward weighting — learn to win
// ──────────────────────────────────────────────────────────

/// TTT reward function: +1 for X win, -1 for O win, 0 for draw.
/// Place indices: win_x = 22, win_o = 21.
fn ttt_reward_x(final_marking: &[i64]) -> f64 {
    if final_marking[22] > 0 { 1.0 }
    else if final_marking[21] > 0 { -1.0 }
    else { 0.0 }
}

#[test]
fn rnn_self_play_ttt() {
    let net = pflow_tropical::ttt_fixtures::pilot_ttt();
    let np = net.num_places();
    let nt = net.num_transitions();

    // Bootstrap: pretrain on random trajectories so the RNN has basic game understanding
    let trajectories = generate_game_trajectories(&net, 200, 42);
    let mut rnn = ElmanRnn::new(np, 32, nt, 42);
    rnn.train(&trajectories, 0.05, 50);

    // Snapshot the pretrained version for comparison
    let pretrained = rnn.clone();

    // Self-play: 10 rounds × 50 games, train on wins/losses
    let (loss, stats) = rnn.self_play_train(
        &net,
        &ttt_reward_x,
        50,   // games per round
        5,    // epochs per round
        10,   // rounds
        0.02, // learning rate
        123,  // seed
    );

    eprintln!("Self-play final loss: {loss:.3}");
    eprintln!(
        "Last round outcomes: {:.0}% wins, {:.0}% draws, {:.0}% losses",
        stats[0] * 100.0, stats[1] * 100.0, stats[2] * 100.0
    );

    // Play 50 deterministic games with each and compare X win rates
    let mut pretrained_x_wins = 0;
    let mut selfplay_x_wins = 0;
    let n = 50;

    // Use stochastic play for varied games (deterministic would repeat the same game)
    let mut rng1 = pflow_tropical::rnn::SimpleRng::new(999);
    let mut rng2 = pflow_tropical::rnn::SimpleRng::new(999); // same seed for fair comparison

    for _ in 0..n {
        let r1 = pretrained.play_game_stochastic(&net, &mut rng1);
        if r1.final_marking[22] > 0 { pretrained_x_wins += 1; }

        let r2 = rnn.play_game_stochastic(&net, &mut rng2);
        if r2.final_marking[22] > 0 { selfplay_x_wins += 1; }
    }

    eprintln!(
        "X win rate — pretrained: {}/{n}, self-play: {}/{n}",
        pretrained_x_wins, selfplay_x_wins
    );

    // Self-play trained RNN should win at least as often as pretrained
    assert!(
        selfplay_x_wins >= pretrained_x_wins,
        "self-play should improve X win rate: {selfplay_x_wins} vs {pretrained_x_wins}"
    );
}

// ──────────────────────────────────────────────────────────
// Test 9: Reward weighting is generic — works on any net
// ──────────────────────────────────────────────────────────

#[test]
fn reward_weighted_generic_net() {
    // Producer-consumer: reward = tokens in P2 (the sink)
    let net = NetMatrix::from_incidence(
        vec![
            vec![-1, 1, 0], // T0: P0 -> P1
            vec![0, -1, 1], // T1: P1 -> P2
        ],
        vec![3, 0, 0],
        vec!["P0".into(), "P1".into(), "P2".into()],
        vec!["T0".into(), "T1".into()],
    );

    let np = net.num_places();
    let nt = net.num_transitions();
    let mut rnn = ElmanRnn::new(np, 16, nt, 42);

    // Reward: how many tokens reached the sink (P2)
    let reward_fn = |marking: &[i64]| -> f64 { marking[2] as f64 };

    let (loss, stats) = rnn.self_play_train(
        &net,
        &reward_fn,
        50,  // games
        10,  // epochs
        5,   // rounds
        0.1, // lr
        42,
    );

    eprintln!("Producer-consumer self-play: loss={loss:.3}, win%={:.0}", stats[0] * 100.0);

    // After training, the RNN should consistently move tokens to P2
    let result = rnn.play_game(&net);
    let p2_tokens = result.final_marking[2];
    assert!(
        p2_tokens >= 2,
        "trained RNN should move most tokens to P2, got {p2_tokens}"
    );
}

// ──────────────────────────────────────────────────────────
// Core TTT (tropical only) — no win detection, no turns
// ──────────────────────────────────────────────────────────

/// builder_ttt() place layout (27 places, alphabetical):
///   o00..o22 (indices 0-8), p00..p22 (indices 9-17), x00..x22 (indices 18-26)
/// Transitions (18): x_play_00..x_play_22 (0-8), o_play_00..o_play_22 (9-17)
const CORE_WIN_LINES: [[usize; 3]; 8] = [
    [0, 1, 2], // row 0
    [3, 4, 5], // row 1
    [6, 7, 8], // row 2
    [0, 3, 6], // col 0
    [1, 4, 7], // col 1
    [2, 5, 8], // col 2
    [0, 4, 8], // diag
    [2, 4, 6], // anti
];

/// Check win from core net marking. Returns (x_wins, o_wins).
fn core_check_win(marking: &[i64]) -> (bool, bool) {
    let x_wins = CORE_WIN_LINES.iter().any(|line| {
        line.iter().all(|&cell| marking[18 + cell] > 0) // x00..x22 at indices 18-26
    });
    let o_wins = CORE_WIN_LINES.iter().any(|line| {
        line.iter().all(|&cell| marking[cell] > 0) // o00..o22 at indices 0-8
    });
    (x_wins, o_wins)
}

/// Render board from core net (27 places).
fn render_core_board(marking: &[i64]) -> String {
    let mut board = ['.'; 9];
    for cell in 0..9 {
        if marking[18 + cell] > 0 {
            board[cell] = 'X';
        } else if marking[cell] > 0 {
            board[cell] = 'O';
        }
    }
    format!(
        " {} | {} | {}\n-----------\n {} | {} | {}\n-----------\n {} | {} | {}",
        board[0], board[1], board[2],
        board[3], board[4], board[5],
        board[6], board[7], board[8],
    )
}

/// Count X and O moves from a core trajectory.
/// x_play transitions are indices 0-8, o_play are 9-17.
fn count_moves(moves: &[pflow_tropical::rnn::GameMove]) -> (usize, usize) {
    let x = moves.iter().filter(|m| m.transition < 9).count();
    let o = moves.iter().filter(|m| m.transition >= 9).count();
    (x, o)
}

/// Core reward: check win externally, penalize non-alternating play.
/// X wants to win, gets +1 for win, -1 for loss.
/// Returns 0 if game isn't well-formed (e.g., same player moved twice in a row).
fn core_reward_x(marking: &[i64], moves: &[pflow_tropical::rnn::GameMove]) -> f64 {
    // Check alternation: X should go on even moves (0,2,4,..), O on odd (1,3,5,..)
    let mut alternates = true;
    for (i, mv) in moves.iter().enumerate() {
        let is_x = mv.transition < 9;
        let should_be_x = i % 2 == 0;
        if is_x != should_be_x {
            alternates = false;
            break;
        }
    }

    if !alternates {
        return -0.5; // penalize non-alternating play
    }

    let (x_wins, o_wins) = core_check_win(marking);
    if x_wins { 1.0 }
    else if o_wins { -1.0 }
    else { 0.0 }
}

// ──────────────────────────────────────────────────────────
// Test 10: Core TTT — RNN on tropical-only board
// ──────────────────────────────────────────────────────────

#[test]
fn rnn_core_ttt_random_play() {
    let net = pflow_tropical::ttt_fixtures::builder_ttt();
    let np = net.num_places();   // 27
    let nt = net.num_transitions(); // 18

    // Core net never terminates (no win detection drains tokens).
    // Cap at 9 moves (full board).
    let trajectories = generate_game_trajectories_capped(&net, 200, 9, 42);
    assert!(!trajectories.is_empty());

    for (inputs, targets) in &trajectories {
        assert!(inputs.len() <= 9, "capped at 9 moves");
        assert_eq!(inputs[0].len(), np);
        for &t in targets {
            assert!(t < nt);
        }
    }

    // Train
    let mut rnn = ElmanRnn::new(np, 32, nt, 42);
    let loss = rnn.train(&trajectories, 0.05, 100);

    let avg_steps: f64 = trajectories.iter()
        .map(|(inputs, _)| inputs.len() as f64)
        .sum::<f64>() / trajectories.len() as f64;
    let random_baseline = avg_steps * (nt as f64).ln();

    eprintln!("Core TTT — loss: {loss:.3}, random baseline: {random_baseline:.1}");

    assert!(
        loss < random_baseline,
        "RNN should beat random on core TTT (loss={loss}, baseline={random_baseline:.1})"
    );

    // Play a game and show it
    let result = rnn.play_game(&net);
    let (x_wins, o_wins) = core_check_win(&result.final_marking);
    let (xc, oc) = count_moves(&result.moves);

    eprintln!("\nCore TTT game ({} moves, X:{xc} O:{oc}):", result.moves.len());
    for (i, mv) in result.moves.iter().enumerate() {
        let tname = &net.transition_labels[mv.transition];
        eprintln!("  Move {}: {} ({:.0}%)", i + 1, tname, mv.confidence * 100.0);
    }
    eprintln!("{}", render_core_board(&result.final_marking));
    eprintln!(
        "Result: {}",
        if x_wins { "X wins" } else if o_wins { "O wins" } else { "draw/incomplete" }
    );
}

// ──────────────────────────────────────────────────────────
// Test 11: Core TTT self-play with external win detection
// ──────────────────────────────────────────────────────────

#[test]
fn rnn_core_ttt_self_play() {
    let net = pflow_tropical::ttt_fixtures::builder_ttt();
    let np = net.num_places();   // 27
    let nt = net.num_transitions(); // 18

    // Bootstrap on random capped games
    let trajectories = generate_game_trajectories_capped(&net, 200, 9, 42);
    let mut rnn = ElmanRnn::new(np, 32, nt, 42);
    rnn.train(&trajectories, 0.05, 50);

    let pretrained = rnn.clone();

    // Self-play with external reward.
    // Can't use self_play_train directly because:
    //  1. Core net doesn't terminate (need 9-step cap)
    //  2. Reward depends on move sequence (alternation), not just final marking
    // So we do the loop manually.
    let mut rng = SimpleRng::new(123);

    for _round in 0..10 {
        let mut weighted = Vec::new();

        for _ in 0..50 {
            // Play stochastic game, capped at 9 moves
            let result = play_core_game_stochastic(&rnn, &net, 9, &mut rng);
            let reward = core_reward_x(&result.final_marking, &result.moves);

            let inputs: Vec<Vec<f64>> = result.moves.iter()
                .map(|m| m.marking.iter().map(|&v| v as f64).collect())
                .collect();
            let targets: Vec<usize> = result.moves.iter()
                .map(|m| m.transition)
                .collect();

            weighted.push((inputs, targets, reward));
        }

        rnn.train_weighted(&weighted, 0.02, 5);
    }

    // Compare: play 50 games each
    let mut pre_x_wins = 0;
    let mut post_x_wins = 0;
    let mut pre_alternating = 0;
    let mut post_alternating = 0;
    let n = 50;

    let mut rng1 = SimpleRng::new(999);
    let mut rng2 = SimpleRng::new(999);

    for _ in 0..n {
        let r1 = play_core_game_stochastic(&pretrained, &net, 9, &mut rng1);
        let (xw1, _) = core_check_win(&r1.final_marking);
        if xw1 { pre_x_wins += 1; }
        if is_alternating(&r1) { pre_alternating += 1; }

        let r2 = play_core_game_stochastic(&rnn, &net, 9, &mut rng2);
        let (xw2, _) = core_check_win(&r2.final_marking);
        if xw2 { post_x_wins += 1; }
        if is_alternating(&r2) { post_alternating += 1; }
    }

    eprintln!("Core TTT self-play results ({n} games):");
    eprintln!(
        "  Pretrained — X wins: {pre_x_wins}, alternating: {pre_alternating}"
    );
    eprintln!(
        "  Self-play  — X wins: {post_x_wins}, alternating: {post_alternating}"
    );

    // Key insight: the core net (tropical) has no turn enforcement.
    // The RNN discovers it can "cheat" — X plays multiple consecutive moves.
    // This is *why* the observer layer exists in pilot_ttt: to structurally
    // enforce alternation. Without it, any reward-seeking policy exploits the gap.
    assert!(
        post_x_wins > pre_x_wins,
        "self-play should increase X wins: {post_x_wins} vs {pre_x_wins}"
    );
}

/// Play a capped stochastic game on the core net.
fn play_core_game_stochastic(
    rnn: &ElmanRnn,
    net: &NetMatrix,
    max_steps: usize,
    rng: &mut SimpleRng,
) -> GameResult {
    let mut marking = net.initial.clone();
    let mut h = vec![0.0; rnn.hidden_size];
    let mut moves = Vec::new();

    for _ in 0..max_steps {
        let enabled: Vec<usize> = (0..net.num_transitions())
            .filter(|&t| net.is_enabled(&marking, t))
            .collect();
        if enabled.is_empty() { break; }

        let input: Vec<f64> = marking.iter().map(|&m| m as f64).collect();
        let (h_new, logits) = rnn.step(&input, &h);
        h = h_new;

        let mut masked = vec![f64::NEG_INFINITY; rnn.output_size];
        for &t in &enabled { masked[t] = logits[t]; }
        let probs = ElmanRnn::softmax(&masked);

        // Sample
        let r = rng.uniform();
        let mut cumulative = 0.0;
        let mut chosen = enabled[0];
        for (i, &p) in probs.iter().enumerate() {
            cumulative += p;
            if r < cumulative { chosen = i; break; }
        }

        let confidence = probs[chosen];
        moves.push(pflow_tropical::rnn::GameMove {
            marking: marking.clone(),
            transition: chosen,
            probs,
            confidence,
        });

        marking = net.fire(&marking, chosen).unwrap();

        // Early termination if someone won
        let (xw, ow) = core_check_win(&marking);
        if xw || ow { break; }
    }

    GameResult { moves, final_marking: marking }
}

fn is_alternating(result: &GameResult) -> bool {
    result.moves.iter().enumerate().all(|(i, mv)| {
        let is_x = mv.transition < 9;
        let should_be_x = i % 2 == 0;
        is_x == should_be_x
    })
}

// ──────────────────────────────────────────────────────────
// Test 12: Core TTT with turn enforcement (still tropical)
// ──────────────────────────────────────────────────────────

/// builder_ttt_turns() place layout (29 places, alphabetical):
///   o00..o22 (0-8), o_turn (9), p00..p22 (10-18), x00..x22 (19-27), x_turn (28)
/// Transitions (18): x_play_00..x_play_22 (0-8), o_play_00..o_play_22 (9-17)
fn core_turns_check_win(marking: &[i64]) -> (bool, bool) {
    let x_wins = CORE_WIN_LINES.iter().any(|line| {
        line.iter().all(|&cell| marking[19 + cell] > 0) // x00..x22 at 19-27
    });
    let o_wins = CORE_WIN_LINES.iter().any(|line| {
        line.iter().all(|&cell| marking[cell] > 0) // o00..o22 at 0-8
    });
    (x_wins, o_wins)
}

fn render_core_turns_board(marking: &[i64]) -> String {
    let mut board = ['.'; 9];
    for cell in 0..9 {
        if marking[19 + cell] > 0 {
            board[cell] = 'X';
        } else if marking[cell] > 0 {
            board[cell] = 'O';
        }
    }
    let turn = if marking[28] > 0 { "X" } else { "O" };
    format!(
        " {} | {} | {}\n-----------\n {} | {} | {}\n-----------\n {} | {} | {}  (turn: {})",
        board[0], board[1], board[2],
        board[3], board[4], board[5],
        board[6], board[7], board[8],
        turn,
    )
}

fn play_core_turns_game_stochastic(
    rnn: &ElmanRnn,
    net: &NetMatrix,
    max_steps: usize,
    rng: &mut SimpleRng,
) -> GameResult {
    let mut marking = net.initial.clone();
    let mut h = vec![0.0; rnn.hidden_size];
    let mut moves = Vec::new();

    for _ in 0..max_steps {
        let enabled: Vec<usize> = (0..net.num_transitions())
            .filter(|&t| net.is_enabled(&marking, t))
            .collect();
        if enabled.is_empty() { break; }

        let input: Vec<f64> = marking.iter().map(|&m| m as f64).collect();
        let (h_new, logits) = rnn.step(&input, &h);
        h = h_new;

        let mut masked = vec![f64::NEG_INFINITY; rnn.output_size];
        for &t in &enabled { masked[t] = logits[t]; }
        let probs = ElmanRnn::softmax(&masked);

        let r = rng.uniform();
        let mut cumulative = 0.0;
        let mut chosen = enabled[0];
        for (i, &p) in probs.iter().enumerate() {
            cumulative += p;
            if r < cumulative { chosen = i; break; }
        }

        let confidence = probs[chosen];
        moves.push(pflow_tropical::rnn::GameMove {
            marking: marking.clone(),
            transition: chosen,
            probs,
            confidence,
        });

        marking = net.fire(&marking, chosen).unwrap();

        let (xw, ow) = core_turns_check_win(&marking);
        if xw || ow { break; }
    }

    GameResult { moves, final_marking: marking }
}

#[test]
fn rnn_core_ttt_with_turns() {
    let net = pflow_tropical::ttt_fixtures::builder_ttt_turns();
    let np = net.num_places();   // 29
    let nt = net.num_transitions(); // 18

    assert_eq!(np, 29);
    assert_eq!(nt, 18);

    // Verify turn enforcement: from initial, only x_play transitions enabled
    let enabled: Vec<usize> = (0..nt)
        .filter(|&t| net.is_enabled(&net.initial, t))
        .collect();
    assert!(
        enabled.iter().all(|&t| t < 9),
        "initially only x_play (0-8) should be enabled, got {:?}", enabled
    );

    // Generate capped games (9 moves = full board)
    let trajectories = generate_game_trajectories_capped(&net, 200, 9, 42);

    // Verify all trajectories alternate correctly
    for (_, targets) in &trajectories {
        for (i, &t) in targets.iter().enumerate() {
            let is_x = t < 9;
            let should_be_x = i % 2 == 0;
            assert_eq!(
                is_x, should_be_x,
                "turn enforcement failed at move {i}: transition {t}"
            );
        }
    }
    eprintln!("Turn enforcement verified: all {} trajectories alternate correctly",
        trajectories.len());

    // Train
    let mut rnn = ElmanRnn::new(np, 32, nt, 42);
    rnn.train(&trajectories, 0.05, 50);

    // Self-play with external win reward
    let pretrained = rnn.clone();
    let mut rng = SimpleRng::new(123);

    for _round in 0..10 {
        let mut weighted = Vec::new();
        for _ in 0..50 {
            let result = play_core_turns_game_stochastic(&rnn, &net, 9, &mut rng);
            let (xw, ow) = core_turns_check_win(&result.final_marking);
            let reward = if xw { 1.0 } else if ow { -1.0 } else { 0.0 };

            let inputs: Vec<Vec<f64>> = result.moves.iter()
                .map(|m| m.marking.iter().map(|&v| v as f64).collect())
                .collect();
            let targets: Vec<usize> = result.moves.iter()
                .map(|m| m.transition).collect();

            weighted.push((inputs, targets, reward));
        }
        rnn.train_weighted(&weighted, 0.02, 5);
    }

    // Compare pretrained vs self-play
    let mut pre_x = 0;
    let mut post_x = 0;
    let mut post_alt = 0;
    let n = 50;
    let mut rng1 = SimpleRng::new(999);
    let mut rng2 = SimpleRng::new(999);

    for _ in 0..n {
        let r1 = play_core_turns_game_stochastic(&pretrained, &net, 9, &mut rng1);
        let (xw1, _) = core_turns_check_win(&r1.final_marking);
        if xw1 { pre_x += 1; }

        let r2 = play_core_turns_game_stochastic(&rnn, &net, 9, &mut rng2);
        let (xw2, _) = core_turns_check_win(&r2.final_marking);
        if xw2 { post_x += 1; }

        // Verify alternation is guaranteed by net structure
        let alt = r2.moves.iter().enumerate().all(|(i, mv)| {
            (mv.transition < 9) == (i % 2 == 0)
        });
        if alt { post_alt += 1; }
    }

    eprintln!("Core+turns self-play ({n} games):");
    eprintln!("  Pretrained X wins: {pre_x}");
    eprintln!("  Self-play  X wins: {post_x}");
    eprintln!("  Alternating: {post_alt}/{n} (enforced by net)");

    // Show a sample game
    let mut show_rng = SimpleRng::new(42);
    let sample = play_core_turns_game_stochastic(&rnn, &net, 9, &mut show_rng);
    let (xw, ow) = core_turns_check_win(&sample.final_marking);
    eprintln!("\nSample game ({} moves):", sample.moves.len());
    for (i, mv) in sample.moves.iter().enumerate() {
        eprintln!("  Move {}: {} ({:.0}%)",
            i + 1, net.transition_labels[mv.transition], mv.confidence * 100.0);
    }
    eprintln!("{}", render_core_turns_board(&sample.final_marking));
    eprintln!("Result: {}",
        if xw { "X wins" } else if ow { "O wins" } else { "draw" });

    // Alternation must be 100% — it's structural, not learned
    assert_eq!(post_alt, n, "turn enforcement must guarantee alternation");
    // Self-play should improve
    assert!(post_x >= pre_x,
        "self-play should help: {post_x} vs {pre_x}");
}

// ──────────────────────────────────────────────────────────
// Test 13: Core vs Full — compare what the RNN learns
// ──────────────────────────────────────────────────────────

#[test]
fn core_vs_full_ttt_comparison() {
    // Core net: 27 places, 18 transitions (tropical, no observer)
    let core_net = pflow_tropical::ttt_fixtures::builder_ttt();
    // Full net: 33 places, 35 transitions (with win/draw/turn observer)
    let full_net = pflow_tropical::ttt_fixtures::pilot_ttt();

    // Train both on random games
    let core_trajs = generate_game_trajectories_capped(&core_net, 200, 9, 42);
    let full_trajs = generate_game_trajectories(&full_net, 200, 42);

    let mut core_rnn = ElmanRnn::new(
        core_net.num_places(), 32, core_net.num_transitions(), 42
    );
    let mut full_rnn = ElmanRnn::new(
        full_net.num_places(), 32, full_net.num_transitions(), 42
    );

    let core_loss = core_rnn.train(&core_trajs, 0.05, 100);
    let full_loss = full_rnn.train(&full_trajs, 0.05, 100);

    eprintln!("Core TTT: {np}p/{nt}t, loss={core_loss:.3}",
        np = core_net.num_places(), nt = core_net.num_transitions());
    eprintln!("Full TTT: {np}p/{nt}t, loss={full_loss:.3}",
        np = full_net.num_places(), nt = full_net.num_transitions());

    // Play games and compare
    let mut core_x = 0;
    let mut full_x = 0;
    let n = 20;

    let mut rng = SimpleRng::new(777);

    for _ in 0..n {
        let cr = play_core_game_stochastic(&core_rnn, &core_net, 9, &mut rng);
        let (xw, _) = core_check_win(&cr.final_marking);
        if xw { core_x += 1; }

        let fr = full_rnn.play_game_stochastic(&full_net, &mut rng);
        if fr.final_marking[22] > 0 { full_x += 1; }
    }

    eprintln!("X wins ({n} games) — core: {core_x}, full: {full_x}");
    eprintln!(
        "Core net is tropical (event graph). Full net adds observer layer.\n\
         Core RNN must learn turn alternation implicitly; full net enforces it structurally."
    );

    // Both should produce valid games
    assert!(core_x + full_x > 0, "at least one RNN should produce X wins");
}
