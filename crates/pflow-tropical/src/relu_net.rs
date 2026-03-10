//! Minimal single-hidden-layer ReLU network for conjecture testing.
//!
//! No external dependencies — just matrix ops and backprop from scratch.
//! The network computes: output = W2 · ReLU(W1 · input + b1) + b2
//!
//! ReLU(x) = max(0, x) is a tropical polynomial, so the network's
//! learned weights encode tropical structure that can be factored
//! back into a Petri net incidence matrix.

/// A single-hidden-layer ReLU network.
#[derive(Debug, Clone)]
pub struct ReluNet {
    pub w1: Vec<Vec<f64>>, // hidden_size x input_size
    pub b1: Vec<f64>,      // hidden_size
    pub w2: Vec<Vec<f64>>, // output_size x hidden_size
    pub b2: Vec<f64>,      // output_size
}

impl ReluNet {
    /// Create a new network with small random weights from a deterministic seed.
    pub fn new(input_size: usize, hidden_size: usize, output_size: usize, seed: u64) -> Self {
        let mut rng = SimpleRng::new(seed);

        let w1 = (0..hidden_size)
            .map(|_| (0..input_size).map(|_| rng.normal() * 0.1).collect())
            .collect();
        let b1 = vec![0.0; hidden_size];

        let w2 = (0..output_size)
            .map(|_| (0..hidden_size).map(|_| rng.normal() * 0.1).collect())
            .collect();
        let b2 = vec![0.0; output_size];

        Self { w1, b1, w2, b2 }
    }

    /// Forward pass. Returns (hidden_pre_relu, hidden_post_relu, output).
    pub fn forward(&self, input: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let hidden_size = self.w1.len();
        let output_size = self.w2.len();

        // hidden = W1 · input + b1
        let mut pre_relu = vec![0.0; hidden_size];
        for i in 0..hidden_size {
            let mut sum = self.b1[i];
            for j in 0..input.len() {
                sum += self.w1[i][j] * input[j];
            }
            pre_relu[i] = sum;
        }

        // ReLU
        let post_relu: Vec<f64> = pre_relu.iter().map(|&x| x.max(0.0)).collect();

        // output = W2 · hidden + b2
        let mut output = vec![0.0; output_size];
        for i in 0..output_size {
            let mut sum = self.b2[i];
            for j in 0..hidden_size {
                sum += self.w2[i][j] * post_relu[j];
            }
            output[i] = sum;
        }

        (pre_relu, post_relu, output)
    }

    /// Train on a dataset using SGD with MSE loss.
    /// Returns final average loss.
    pub fn train(
        &mut self,
        inputs: &[Vec<f64>],
        targets: &[Vec<f64>],
        learning_rate: f64,
        epochs: usize,
    ) -> f64 {
        let n = inputs.len();
        let mut loss = 0.0;

        for _epoch in 0..epochs {
            loss = 0.0;
            for (input, target) in inputs.iter().zip(targets.iter()) {
                let (pre_relu, post_relu, output) = self.forward(input);

                // MSE loss gradient: d_loss/d_output = 2 * (output - target) / output_size
                let output_size = output.len();
                let hidden_size = self.w1.len();

                let d_output: Vec<f64> = output
                    .iter()
                    .zip(target.iter())
                    .map(|(o, t)| 2.0 * (o - t) / output_size as f64)
                    .collect();

                // Accumulate loss
                for (o, t) in output.iter().zip(target.iter()) {
                    loss += (o - t).powi(2);
                }

                // Gradients for W2, b2
                // d_loss/d_W2[i][j] = d_output[i] * post_relu[j]
                // d_loss/d_b2[i] = d_output[i]
                for i in 0..output_size {
                    for j in 0..hidden_size {
                        self.w2[i][j] -= learning_rate * d_output[i] * post_relu[j];
                    }
                    self.b2[i] -= learning_rate * d_output[i];
                }

                // Backprop through W2: d_hidden = W2^T · d_output
                let mut d_hidden = vec![0.0; hidden_size];
                for j in 0..hidden_size {
                    for i in 0..output_size {
                        d_hidden[j] += self.w2[i][j] * d_output[i];
                    }
                }

                // Backprop through ReLU: zero gradient where pre_relu <= 0
                for j in 0..hidden_size {
                    if pre_relu[j] <= 0.0 {
                        d_hidden[j] = 0.0;
                    }
                }

                // Gradients for W1, b1
                let input_size = input.len();
                for i in 0..hidden_size {
                    for j in 0..input_size {
                        self.w1[i][j] -= learning_rate * d_hidden[i] * input[j];
                    }
                    self.b1[i] -= learning_rate * d_hidden[i];
                }
            }
            loss /= n as f64;
        }

        loss
    }

    /// Predict output for a given input.
    pub fn predict(&self, input: &[f64]) -> Vec<f64> {
        self.forward(input).2
    }
}

/// Simple deterministic PRNG (xoshiro256**) for reproducible weight init.
struct SimpleRng {
    state: [u64; 4],
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        // SplitMix64 to initialize state
        let mut s = seed;
        let mut state = [0u64; 4];
        for st in &mut state {
            s = s.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = s;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            *st = z ^ (z >> 31);
        }
        Self { state }
    }

    fn next_u64(&mut self) -> u64 {
        let result = self.state[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.state[1] << 17;
        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= t;
        self.state[3] = self.state[3].rotate_left(45);
        result
    }

    fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Box-Muller transform for normal distribution.
    fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1e-10); // avoid log(0)
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}
