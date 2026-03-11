//! Elman RNN for learning sequential Petri net firing patterns.
//!
//! No external dependencies — matrix ops and BPTT from scratch.
//! The network computes:
//!   h_t = tanh(W_xh · x_t + W_hh · h_{t-1} + b_h)
//!   y_t = W_hy · h_t + b_y
//!
//! Trained on game trajectories from a Petri net, the hidden state
//! captures sequential context (turn order, board history) that a
//! feedforward network cannot represent.

pub use crate::relu_net::SimpleRng;

/// An Elman recurrent neural network.
#[derive(Debug, Clone)]
pub struct ElmanRnn {
    pub w_xh: Vec<Vec<f64>>, // hidden_size x input_size
    pub w_hh: Vec<Vec<f64>>, // hidden_size x hidden_size
    pub b_h: Vec<f64>,       // hidden_size
    pub w_hy: Vec<Vec<f64>>, // output_size x hidden_size
    pub b_y: Vec<f64>,       // output_size
    pub hidden_size: usize,
    pub input_size: usize,
    pub output_size: usize,
}

impl ElmanRnn {
    /// Create a new RNN with small random weights from a deterministic seed.
    pub fn new(input_size: usize, hidden_size: usize, output_size: usize, seed: u64) -> Self {
        let mut rng = SimpleRng::new(seed);
        let scale = 0.1;

        let w_xh = (0..hidden_size)
            .map(|_| (0..input_size).map(|_| rng.normal() * scale).collect())
            .collect();
        let w_hh = (0..hidden_size)
            .map(|_| (0..hidden_size).map(|_| rng.normal() * scale).collect())
            .collect();
        let b_h = vec![0.0; hidden_size];

        let w_hy = (0..output_size)
            .map(|_| (0..hidden_size).map(|_| rng.normal() * scale).collect())
            .collect();
        let b_y = vec![0.0; output_size];

        Self { w_xh, w_hh, b_h, w_hy, b_y, hidden_size, input_size, output_size }
    }

    /// Forward pass over a single timestep given input and previous hidden state.
    /// Returns (new_hidden, output).
    pub fn step(&self, input: &[f64], h_prev: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let hs = self.hidden_size;

        // h = tanh(W_xh · x + W_hh · h_prev + b_h)
        let mut h_pre = vec![0.0; hs];
        for i in 0..hs {
            let mut sum = self.b_h[i];
            for j in 0..self.input_size {
                sum += self.w_xh[i][j] * input[j];
            }
            for j in 0..hs {
                sum += self.w_hh[i][j] * h_prev[j];
            }
            h_pre[i] = sum;
        }
        let h: Vec<f64> = h_pre.iter().map(|&x| x.tanh()).collect();

        // y = W_hy · h + b_y
        let mut y = vec![0.0; self.output_size];
        for i in 0..self.output_size {
            let mut sum = self.b_y[i];
            for j in 0..hs {
                sum += self.w_hy[i][j] * h[j];
            }
            y[i] = sum;
        }

        (h, y)
    }

    /// Forward pass over a full sequence. Returns (hidden_states, outputs).
    /// hidden_states[t] is the hidden state after processing input[t].
    pub fn forward(&self, inputs: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
        let mut hiddens = Vec::with_capacity(inputs.len());
        let mut outputs = Vec::with_capacity(inputs.len());
        let mut h = vec![0.0; self.hidden_size];

        for input in inputs {
            let (h_new, y) = self.step(input, &h);
            h = h_new;
            hiddens.push(h.clone());
            outputs.push(y);
        }

        (hiddens, outputs)
    }

    /// Softmax over a vector (for classification output).
    pub fn softmax(logits: &[f64]) -> Vec<f64> {
        let max = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let exps: Vec<f64> = logits.iter().map(|&x| (x - max).exp()).collect();
        let sum: f64 = exps.iter().sum();
        exps.iter().map(|&e| e / sum).collect()
    }

    /// Predict the most likely class at each timestep (argmax of output).
    pub fn predict_sequence(&self, inputs: &[Vec<f64>]) -> Vec<usize> {
        let (_, outputs) = self.forward(inputs);
        outputs.iter().map(|y| {
            y.iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                .map(|(i, _)| i)
                .unwrap()
        }).collect()
    }

    /// Train on sequences using BPTT with cross-entropy loss (softmax output).
    ///
    /// Each training example is a sequence of (input, target_class) pairs.
    /// Returns final average loss across all sequences.
    pub fn train(
        &mut self,
        sequences: &[(Vec<Vec<f64>>, Vec<usize>)], // (inputs, target_classes) per sequence
        learning_rate: f64,
        epochs: usize,
    ) -> f64 {
        let mut loss = 0.0;

        for _epoch in 0..epochs {
            loss = 0.0;

            for (inputs, targets) in sequences {
                let seq_len = inputs.len();
                let hs = self.hidden_size;

                // Forward pass — store all intermediate values for BPTT
                let mut h_prevs = Vec::with_capacity(seq_len); // h_{t-1}
                let mut h_posts = Vec::with_capacity(seq_len); // h_t (post-tanh)
                let mut logits_all = Vec::with_capacity(seq_len);
                let mut h = vec![0.0; hs];

                for input in inputs.iter() {
                    h_prevs.push(h.clone());
                    let mut h_pre = vec![0.0; hs];
                    for i in 0..hs {
                        let mut sum = self.b_h[i];
                        for j in 0..self.input_size {
                            sum += self.w_xh[i][j] * input[j];
                        }
                        for j in 0..hs {
                            sum += self.w_hh[i][j] * h[j];
                        }
                        h_pre[i] = sum;
                    }
                    h = h_pre.iter().map(|&x| x.tanh()).collect();
                    h_posts.push(h.clone());

                    let mut y = vec![0.0; self.output_size];
                    for i in 0..self.output_size {
                        let mut sum = self.b_y[i];
                        for j in 0..hs {
                            sum += self.w_hy[i][j] * h[j];
                        }
                        y[i] = sum;
                    }
                    logits_all.push(y);
                }

                // Backward pass (BPTT)
                let mut dw_xh = vec![vec![0.0; self.input_size]; hs];
                let mut dw_hh = vec![vec![0.0; hs]; hs];
                let mut db_h = vec![0.0; hs];
                let mut dw_hy = vec![vec![0.0; hs]; self.output_size];
                let mut db_y = vec![0.0; self.output_size];

                let mut dh_next = vec![0.0; hs]; // gradient flowing back through time

                for t in (0..seq_len).rev() {
                    let probs = Self::softmax(&logits_all[t]);
                    let target = targets[t];

                    // Cross-entropy loss
                    loss -= probs[target].max(1e-10).ln();

                    // d_logits = probs - one_hot(target)
                    let mut d_logits = probs;
                    d_logits[target] -= 1.0;

                    // Gradients for W_hy, b_y
                    for i in 0..self.output_size {
                        for j in 0..hs {
                            dw_hy[i][j] += d_logits[i] * h_posts[t][j];
                        }
                        db_y[i] += d_logits[i];
                    }

                    // Gradient into h_t: from output + from future timestep
                    let mut dh = vec![0.0; hs];
                    for j in 0..hs {
                        for i in 0..self.output_size {
                            dh[j] += self.w_hy[i][j] * d_logits[i];
                        }
                        dh[j] += dh_next[j];
                    }

                    // Through tanh: dh_pre = dh * (1 - h^2)
                    let mut dh_pre = vec![0.0; hs];
                    for i in 0..hs {
                        dh_pre[i] = dh[i] * (1.0 - h_posts[t][i] * h_posts[t][i]);
                    }

                    // Gradients for W_xh, W_hh, b_h
                    for i in 0..hs {
                        for j in 0..self.input_size {
                            dw_xh[i][j] += dh_pre[i] * inputs[t][j];
                        }
                        for j in 0..hs {
                            dw_hh[i][j] += dh_pre[i] * h_prevs[t][j];
                        }
                        db_h[i] += dh_pre[i];
                    }

                    // Propagate gradient to previous hidden state
                    dh_next = vec![0.0; hs];
                    for j in 0..hs {
                        for i in 0..hs {
                            dh_next[j] += self.w_hh[i][j] * dh_pre[i];
                        }
                    }
                }

                // Gradient clipping (prevent exploding gradients)
                let clip = 5.0;
                clip_vec2d(&mut dw_xh, clip);
                clip_vec2d(&mut dw_hh, clip);
                clip_vec1d(&mut db_h, clip);
                clip_vec2d(&mut dw_hy, clip);
                clip_vec1d(&mut db_y, clip);

                // SGD update
                let lr = learning_rate / seq_len as f64;
                for i in 0..hs {
                    for j in 0..self.input_size {
                        self.w_xh[i][j] -= lr * dw_xh[i][j];
                    }
                    for j in 0..hs {
                        self.w_hh[i][j] -= lr * dw_hh[i][j];
                    }
                    self.b_h[i] -= lr * db_h[i];
                }
                for i in 0..self.output_size {
                    for j in 0..hs {
                        self.w_hy[i][j] -= lr * dw_hy[i][j];
                    }
                    self.b_y[i] -= lr * db_y[i];
                }
            }

            loss /= sequences.len() as f64;
        }

        loss
    }
}

/// Record of a single move in a played game.
#[derive(Debug, Clone)]
pub struct GameMove {
    /// Marking before the move (integer).
    pub marking: Vec<i64>,
    /// Transition index chosen by the policy.
    pub transition: usize,
    /// Softmax probabilities over all transitions.
    pub probs: Vec<f64>,
    /// Confidence: probability assigned to the chosen transition.
    pub confidence: f64,
}

/// Result of a complete game played by the RNN.
#[derive(Debug, Clone)]
pub struct GameResult {
    pub moves: Vec<GameMove>,
    /// Final marking after the last move.
    pub final_marking: Vec<i64>,
}

impl ElmanRnn {
    /// Play a complete game on a `NetMatrix` using the RNN as the move policy.
    ///
    /// At each step, the RNN produces logits over all transitions. Disabled
    /// transitions are masked to `-inf` before softmax, so the policy only
    /// picks among legal moves. The game ends when no transitions are enabled.
    pub fn play_game(&self, net: &crate::net_matrix::NetMatrix) -> GameResult {
        self.play_game_inner(net, false, None)
    }

    /// Play with stochastic sampling (for exploration during self-play training).
    /// Moves are sampled proportional to softmax probabilities instead of argmax.
    pub fn play_game_stochastic(
        &self,
        net: &crate::net_matrix::NetMatrix,
        rng: &mut SimpleRng,
    ) -> GameResult {
        self.play_game_inner(net, true, Some(rng))
    }

    fn play_game_inner(
        &self,
        net: &crate::net_matrix::NetMatrix,
        stochastic: bool,
        mut rng: Option<&mut SimpleRng>,
    ) -> GameResult {
        let mut marking = net.initial.clone();
        let mut h = vec![0.0; self.hidden_size];
        let mut moves = Vec::new();

        loop {
            let enabled: Vec<usize> = (0..net.num_transitions())
                .filter(|&t| net.is_enabled(&marking, t))
                .collect();

            if enabled.is_empty() {
                break;
            }

            let input: Vec<f64> = marking.iter().map(|&m| m as f64).collect();
            let (h_new, logits) = self.step(&input, &h);
            h = h_new;

            // Mask disabled transitions to -inf
            let mut masked = vec![f64::NEG_INFINITY; self.output_size];
            for &t in &enabled {
                masked[t] = logits[t];
            }

            let probs = Self::softmax(&masked);

            let chosen = if stochastic {
                // Sample from distribution
                let r = rng.as_mut().unwrap().uniform();
                let mut cumulative = 0.0;
                let mut pick = enabled[0];
                for (i, &p) in probs.iter().enumerate() {
                    cumulative += p;
                    if r < cumulative {
                        pick = i;
                        break;
                    }
                }
                pick
            } else {
                // Argmax
                probs
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                    .map(|(i, _)| i)
                    .unwrap()
            };

            let confidence = probs[chosen];

            moves.push(GameMove {
                marking: marking.clone(),
                transition: chosen,
                probs,
                confidence,
            });

            marking = net.fire(&marking, chosen).unwrap();
        }

        GameResult { moves, final_marking: marking }
    }

    /// Train with per-sequence reward weighting (REINFORCE-style policy gradient).
    ///
    /// Each sequence carries a reward scalar. Gradients are scaled by reward:
    /// positive rewards reinforce the trajectory, negative rewards discourage it.
    /// Returns final average weighted loss.
    pub fn train_weighted(
        &mut self,
        sequences: &[(Vec<Vec<f64>>, Vec<usize>, f64)], // (inputs, targets, reward)
        learning_rate: f64,
        epochs: usize,
    ) -> f64 {
        let mut loss = 0.0;

        for _epoch in 0..epochs {
            loss = 0.0;

            for (inputs, targets, reward) in sequences {
                if reward.abs() < 1e-10 {
                    continue; // skip zero-reward sequences
                }

                let seq_len = inputs.len();
                let hs = self.hidden_size;

                // Forward pass
                let mut h_prevs = Vec::with_capacity(seq_len);
                let mut h_posts = Vec::with_capacity(seq_len);
                let mut logits_all = Vec::with_capacity(seq_len);
                let mut h = vec![0.0; hs];

                for input in inputs.iter() {
                    h_prevs.push(h.clone());
                    let mut h_pre = vec![0.0; hs];
                    for i in 0..hs {
                        let mut sum = self.b_h[i];
                        for j in 0..self.input_size {
                            sum += self.w_xh[i][j] * input[j];
                        }
                        for j in 0..hs {
                            sum += self.w_hh[i][j] * h[j];
                        }
                        h_pre[i] = sum;
                    }
                    h = h_pre.iter().map(|&x| x.tanh()).collect();
                    h_posts.push(h.clone());

                    let mut y = vec![0.0; self.output_size];
                    for i in 0..self.output_size {
                        let mut sum = self.b_y[i];
                        for j in 0..hs {
                            sum += self.w_hy[i][j] * h[j];
                        }
                        y[i] = sum;
                    }
                    logits_all.push(y);
                }

                // Backward pass — gradients scaled by reward
                let mut dw_xh = vec![vec![0.0; self.input_size]; hs];
                let mut dw_hh = vec![vec![0.0; hs]; hs];
                let mut db_h = vec![0.0; hs];
                let mut dw_hy = vec![vec![0.0; hs]; self.output_size];
                let mut db_y = vec![0.0; self.output_size];
                let mut dh_next = vec![0.0; hs];

                for t in (0..seq_len).rev() {
                    let probs = Self::softmax(&logits_all[t]);
                    let target = targets[t];

                    loss -= reward * probs[target].max(1e-10).ln();

                    // d_logits = reward * (probs - one_hot(target))
                    let mut d_logits: Vec<f64> = probs.iter().map(|&p| p * reward).collect();
                    d_logits[target] -= reward;

                    for i in 0..self.output_size {
                        for j in 0..hs {
                            dw_hy[i][j] += d_logits[i] * h_posts[t][j];
                        }
                        db_y[i] += d_logits[i];
                    }

                    let mut dh = vec![0.0; hs];
                    for j in 0..hs {
                        for i in 0..self.output_size {
                            dh[j] += self.w_hy[i][j] * d_logits[i];
                        }
                        dh[j] += dh_next[j];
                    }

                    let mut dh_pre = vec![0.0; hs];
                    for i in 0..hs {
                        dh_pre[i] = dh[i] * (1.0 - h_posts[t][i] * h_posts[t][i]);
                    }

                    for i in 0..hs {
                        for j in 0..self.input_size {
                            dw_xh[i][j] += dh_pre[i] * inputs[t][j];
                        }
                        for j in 0..hs {
                            dw_hh[i][j] += dh_pre[i] * h_prevs[t][j];
                        }
                        db_h[i] += dh_pre[i];
                    }

                    dh_next = vec![0.0; hs];
                    for j in 0..hs {
                        for i in 0..hs {
                            dh_next[j] += self.w_hh[i][j] * dh_pre[i];
                        }
                    }
                }

                let clip = 5.0;
                clip_vec2d(&mut dw_xh, clip);
                clip_vec2d(&mut dw_hh, clip);
                clip_vec1d(&mut db_h, clip);
                clip_vec2d(&mut dw_hy, clip);
                clip_vec1d(&mut db_y, clip);

                let lr = learning_rate / seq_len as f64;
                for i in 0..hs {
                    for j in 0..self.input_size {
                        self.w_xh[i][j] -= lr * dw_xh[i][j];
                    }
                    for j in 0..hs {
                        self.w_hh[i][j] -= lr * dw_hh[i][j];
                    }
                    self.b_h[i] -= lr * db_h[i];
                }
                for i in 0..self.output_size {
                    for j in 0..hs {
                        self.w_hy[i][j] -= lr * dw_hy[i][j];
                    }
                    self.b_y[i] -= lr * db_y[i];
                }
            }

            let count = sequences.iter().filter(|(_, _, r)| r.abs() >= 1e-10).count().max(1);
            loss /= count as f64;
        }

        loss
    }

    /// Train with per-move reward weighting.
    ///
    /// Like `train_weighted`, but each timestep in a sequence has its own reward.
    /// This enables symmetric training: X moves get `+r` and O moves get `-r`
    /// (or vice versa), so the policy learns to play well as both players.
    pub fn train_weighted_per_move(
        &mut self,
        sequences: &[(Vec<Vec<f64>>, Vec<usize>, Vec<f64>)], // (inputs, targets, per-move rewards)
        learning_rate: f64,
        epochs: usize,
    ) -> f64 {
        let mut loss = 0.0;

        for _epoch in 0..epochs {
            loss = 0.0;

            for (inputs, targets, rewards) in sequences {
                let seq_len = inputs.len();
                if seq_len == 0 { continue; }
                let hs = self.hidden_size;

                // Forward pass
                let mut h_prevs = Vec::with_capacity(seq_len);
                let mut h_posts = Vec::with_capacity(seq_len);
                let mut logits_all = Vec::with_capacity(seq_len);
                let mut h = vec![0.0; hs];

                for input in inputs.iter() {
                    h_prevs.push(h.clone());
                    let mut h_pre = vec![0.0; hs];
                    for i in 0..hs {
                        let mut sum = self.b_h[i];
                        for j in 0..self.input_size {
                            sum += self.w_xh[i][j] * input[j];
                        }
                        for j in 0..hs {
                            sum += self.w_hh[i][j] * h[j];
                        }
                        h_pre[i] = sum;
                    }
                    h = h_pre.iter().map(|&x| x.tanh()).collect();
                    h_posts.push(h.clone());

                    let mut y = vec![0.0; self.output_size];
                    for i in 0..self.output_size {
                        let mut sum = self.b_y[i];
                        for j in 0..hs {
                            sum += self.w_hy[i][j] * h[j];
                        }
                        y[i] = sum;
                    }
                    logits_all.push(y);
                }

                // Backward pass — per-move reward scaling
                let mut dw_xh = vec![vec![0.0; self.input_size]; hs];
                let mut dw_hh = vec![vec![0.0; hs]; hs];
                let mut db_h = vec![0.0; hs];
                let mut dw_hy = vec![vec![0.0; hs]; self.output_size];
                let mut db_y = vec![0.0; self.output_size];
                let mut dh_next = vec![0.0; hs];

                for t in (0..seq_len).rev() {
                    let probs = Self::softmax(&logits_all[t]);
                    let target = targets[t];
                    let reward = rewards[t];

                    loss -= reward * probs[target].max(1e-10).ln();

                    let mut d_logits: Vec<f64> = probs.iter().map(|&p| p * reward).collect();
                    d_logits[target] -= reward;

                    for i in 0..self.output_size {
                        for j in 0..hs {
                            dw_hy[i][j] += d_logits[i] * h_posts[t][j];
                        }
                        db_y[i] += d_logits[i];
                    }

                    let mut dh = vec![0.0; hs];
                    for j in 0..hs {
                        for i in 0..self.output_size {
                            dh[j] += self.w_hy[i][j] * d_logits[i];
                        }
                        dh[j] += dh_next[j];
                    }

                    let mut dh_pre = vec![0.0; hs];
                    for i in 0..hs {
                        dh_pre[i] = dh[i] * (1.0 - h_posts[t][i] * h_posts[t][i]);
                    }

                    for i in 0..hs {
                        for j in 0..self.input_size {
                            dw_xh[i][j] += dh_pre[i] * inputs[t][j];
                        }
                        for j in 0..hs {
                            dw_hh[i][j] += dh_pre[i] * h_prevs[t][j];
                        }
                        db_h[i] += dh_pre[i];
                    }

                    dh_next = vec![0.0; hs];
                    for j in 0..hs {
                        for i in 0..hs {
                            dh_next[j] += self.w_hh[i][j] * dh_pre[i];
                        }
                    }
                }

                let clip = 5.0;
                clip_vec2d(&mut dw_xh, clip);
                clip_vec2d(&mut dw_hh, clip);
                clip_vec1d(&mut db_h, clip);
                clip_vec2d(&mut dw_hy, clip);
                clip_vec1d(&mut db_y, clip);

                let lr = learning_rate / seq_len as f64;
                for i in 0..hs {
                    for j in 0..self.input_size {
                        self.w_xh[i][j] -= lr * dw_xh[i][j];
                    }
                    for j in 0..hs {
                        self.w_hh[i][j] -= lr * dw_hh[i][j];
                    }
                    self.b_h[i] -= lr * db_h[i];
                }
                for i in 0..self.output_size {
                    for j in 0..hs {
                        self.w_hy[i][j] -= lr * dw_hy[i][j];
                    }
                    self.b_y[i] -= lr * db_y[i];
                }
            }

            let count = sequences.len().max(1);
            loss /= count as f64;
        }

        loss
    }

    /// Self-play training loop: play games, score outcomes, train on rewards.
    ///
    /// `reward_fn` maps a final marking to a reward scalar.
    /// Positive = good outcome, negative = bad, zero = neutral.
    /// Returns (final_loss, win_rates) where win_rates is (positive_reward%, zero%, negative%).
    pub fn self_play_train(
        &mut self,
        net: &crate::net_matrix::NetMatrix,
        reward_fn: &dyn Fn(&[i64]) -> f64,
        games_per_round: usize,
        epochs_per_round: usize,
        rounds: usize,
        learning_rate: f64,
        seed: u64,
    ) -> (f64, [f64; 3]) {
        let mut rng = SimpleRng::new(seed);
        let mut loss = 0.0;
        let mut last_stats = [0.0; 3];

        for _round in 0..rounds {
            // Play games with stochastic policy (exploration)
            let mut weighted_seqs = Vec::with_capacity(games_per_round);
            let mut positive = 0usize;
            let mut zero = 0usize;
            let mut negative = 0usize;

            for _ in 0..games_per_round {
                let result = self.play_game_stochastic(net, &mut rng);
                let reward = reward_fn(&result.final_marking);

                if reward > 0.0 { positive += 1; }
                else if reward < 0.0 { negative += 1; }
                else { zero += 1; }

                let inputs: Vec<Vec<f64>> = result.moves.iter()
                    .map(|m| m.marking.iter().map(|&v| v as f64).collect())
                    .collect();
                let targets: Vec<usize> = result.moves.iter()
                    .map(|m| m.transition)
                    .collect();

                weighted_seqs.push((inputs, targets, reward));
            }

            loss = self.train_weighted(&weighted_seqs, learning_rate, epochs_per_round);

            let total = games_per_round as f64;
            last_stats = [
                positive as f64 / total,
                zero as f64 / total,
                negative as f64 / total,
            ];
        }

        (loss, last_stats)
    }
}

fn clip_vec2d(v: &mut [Vec<f64>], clip: f64) {
    for row in v.iter_mut() {
        for x in row.iter_mut() {
            *x = x.clamp(-clip, clip);
        }
    }
}

fn clip_vec1d(v: &mut [f64], clip: f64) {
    for x in v.iter_mut() {
        *x = x.clamp(-clip, clip);
    }
}

/// Generate random legal game trajectories from a NetMatrix.
///
/// Returns sequences of (marking_as_f64, transition_index) suitable for RNN training.
/// Uses a simple deterministic PRNG seeded from the given value.
/// Delegates to `generate_game_trajectories_capped` with no step limit.
pub fn generate_game_trajectories(
    net: &crate::net_matrix::NetMatrix,
    num_games: usize,
    seed: u64,
) -> Vec<(Vec<Vec<f64>>, Vec<usize>)> {
    generate_game_trajectories_capped(net, num_games, 0, seed)
}

/// Like `generate_game_trajectories` but with an explicit step cap.
pub fn generate_game_trajectories_capped(
    net: &crate::net_matrix::NetMatrix,
    num_games: usize,
    max_steps: usize,
    seed: u64,
) -> Vec<(Vec<Vec<f64>>, Vec<usize>)> {
    let mut rng = SimpleRng::new(seed);
    let mut sequences = Vec::with_capacity(num_games);

    for _ in 0..num_games {
        let mut marking = net.initial.clone();
        let mut inputs = Vec::new();
        let mut targets = Vec::new();

        loop {
            if max_steps > 0 && inputs.len() >= max_steps {
                break;
            }

            // Find all enabled transitions
            let enabled: Vec<usize> = (0..net.num_transitions())
                .filter(|&t| net.is_enabled(&marking, t))
                .collect();

            if enabled.is_empty() {
                break;
            }

            // Pick a random enabled transition
            let idx = (rng.next_u64() as usize) % enabled.len();
            let t = enabled[idx];

            inputs.push(marking.iter().map(|&m| m as f64).collect());
            targets.push(t);

            marking = net.fire(&marking, t).unwrap();
        }

        if !inputs.is_empty() {
            sequences.push((inputs, targets));
        }
    }

    sequences
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_loop_rnn() {
        // Simple loop: P0 <-> P1, alternating T0/T1
        let net = crate::NetMatrix::from_incidence(
            vec![vec![-1, 1], vec![1, -1]],
            vec![1, 0],
            vec!["P0".into(), "P1".into()],
            vec!["T0".into(), "T1".into()],
        );

        // This net loops forever so generate_game_trajectories won't terminate.
        // Use fixed sequences instead.
        let _trajectories = generate_game_trajectories(&net, 0, 42);
        let mut fixed_seqs = Vec::new();
        for _ in 0..20 {
            let inputs = vec![
                vec![1.0, 0.0],
                vec![0.0, 1.0],
                vec![1.0, 0.0],
                vec![0.0, 1.0],
            ];
            let targets = vec![0, 1, 0, 1];
            fixed_seqs.push((inputs, targets));
        }

        let mut rnn = ElmanRnn::new(2, 16, 2, 42);
        let loss = rnn.train(&fixed_seqs, 0.1, 100);
        assert!(loss < 0.5, "RNN didn't converge on simple loop: loss = {loss}");

        // Predict: from [1,0] should fire T0, from [0,1] should fire T1
        let preds = rnn.predict_sequence(&[vec![1.0, 0.0], vec![0.0, 1.0]]);
        assert_eq!(preds, vec![0, 1], "RNN should predict alternating transitions");
    }

    #[test]
    fn test_softmax() {
        let probs = ElmanRnn::softmax(&[1.0, 2.0, 3.0]);
        let sum: f64 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
        assert!(probs[2] > probs[1]);
        assert!(probs[1] > probs[0]);
    }
}
