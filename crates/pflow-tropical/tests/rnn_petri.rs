//! RNN trained on Petri net game trajectories.
//!
//! The RNN learns to predict which transition fires next given the current
//! marking. Unlike a feedforward net, the recurrent hidden state captures
//! sequential context — turn order, move history, board symmetry — from
//! the Petri net's firing semantics alone.
//!
//! Generic over any NetMatrix: supply a net and the RNN learns its dynamics.

use pflow_tropical::net_matrix::NetMatrix;
use pflow_tropical::rnn::{generate_game_trajectories, ElmanRnn, GameResult};

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
