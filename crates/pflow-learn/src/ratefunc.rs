//! `RateFunc`: transition rate functions with a learnable parameter vector.
//!
//! Go/JS split this into two interfaces (`RateFunc`, and the optional `GradRateFunc`
//! extension checked via a type assertion / duck typing at call sites). Rust has no
//! runtime interface-implements-check, so the optional method is folded into one trait
//! with a default (`eval_grad` returning `None`) — callers fall back to finite
//! differences (see [`crate::gradrate::rate_grad`]) exactly as Go/JS do when the
//! type-assertion fails.

use pflow_core::State;

/// Hidden-layer activation for [`MLPRateFunc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    Relu,
    Tanh,
}

/// A transition rate function: evaluates to a scalar rate given the current place
/// state and time, and exposes a flat parameter vector that fitting can adjust.
pub trait RateFunc {
    /// Evaluate the rate at the given state and time.
    fn eval(&self, state: &State, t: f64) -> f64;
    /// Current parameter values, in the same flat order `set_params` expects.
    fn get_params(&self) -> Vec<f64>;
    /// Overwrite the parameter vector. Implementations should panic on a length
    /// mismatch, mirroring the Go constructors' own eager-panic behavior.
    fn set_params(&mut self, params: &[f64]);
    /// Number of learnable parameters.
    fn num_params(&self) -> usize;

    /// Analytic gradient of `eval` at `(state, t)`: `(value, d/d(params), d/d(state))`.
    /// Default `None` means "no `GradRateFunc`" in Go terms — callers fall back to
    /// [`crate::gradrate::fd_rate_grad`].
    fn eval_grad(&self, _state: &State, _t: f64) -> Option<(f64, Vec<f64>, State)> {
        None
    }
}

/// A fixed, non-learnable rate. `num_params() == 0`.
pub struct ConstantRateFunc {
    rate: f64,
}

impl ConstantRateFunc {
    pub fn new(rate: f64) -> Self {
        Self { rate }
    }
}

impl RateFunc for ConstantRateFunc {
    fn eval(&self, _state: &State, _t: f64) -> f64 {
        self.rate
    }
    fn get_params(&self) -> Vec<f64> {
        Vec::new()
    }
    fn set_params(&mut self, params: &[f64]) {
        assert!(params.is_empty(), "ConstantRateFunc takes no parameters");
    }
    fn num_params(&self) -> usize {
        0
    }
    fn eval_grad(&self, _state: &State, _t: f64) -> Option<(f64, Vec<f64>, State)> {
        Some((self.rate, Vec::new(), State::new()))
    }
}

/// A single learnable scalar rate: `eval() == theta`. This is the one-parameter-per-
/// transition case go-pflow's `RateFuncsFromRates` builds by default (independent
/// params, not tied).
pub struct ScalarRateFunc {
    theta: f64,
}

impl ScalarRateFunc {
    pub fn new(rate: f64) -> Self {
        Self { theta: rate }
    }
}

impl RateFunc for ScalarRateFunc {
    fn eval(&self, _state: &State, _t: f64) -> f64 {
        self.theta
    }
    fn get_params(&self) -> Vec<f64> {
        vec![self.theta]
    }
    fn set_params(&mut self, params: &[f64]) {
        assert_eq!(params.len(), 1, "ScalarRateFunc takes exactly 1 parameter");
        self.theta = params[0];
    }
    fn num_params(&self) -> usize {
        1
    }
    fn eval_grad(&self, _state: &State, _t: f64) -> Option<(f64, Vec<f64>, State)> {
        Some((self.theta, vec![1.0], State::new()))
    }
}

/// A rate that is an (optionally ReLU-clamped) affine function of a fixed set of place
/// values, and optionally of time. Parameter layout: `[bias, w_1..w_n, (time_weight)]`.
pub struct LinearRateFunc {
    places: Vec<String>,
    time_dependent: bool,
    clamp_relu: bool,
    params: Vec<f64>,
}

impl LinearRateFunc {
    pub fn new(
        places: Vec<String>,
        time_dependent: bool,
        clamp_relu: bool,
        params: Vec<f64>,
    ) -> Self {
        let expected = 1 + places.len() + if time_dependent { 1 } else { 0 };
        assert_eq!(
            params.len(),
            expected,
            "LinearRateFunc: expected {expected} params (bias + {} place weights{}), got {}",
            places.len(),
            if time_dependent { " + time weight" } else { "" },
            params.len()
        );
        Self {
            places,
            time_dependent,
            clamp_relu,
            params,
        }
    }

    fn raw(&self, state: &State, t: f64) -> f64 {
        let mut v = self.params[0];
        for (i, p) in self.places.iter().enumerate() {
            v += self.params[1 + i] * state.get(p).copied().unwrap_or(0.0);
        }
        if self.time_dependent {
            v += self.params[1 + self.places.len()] * t;
        }
        v
    }
}

impl RateFunc for LinearRateFunc {
    fn eval(&self, state: &State, t: f64) -> f64 {
        let v = self.raw(state, t);
        if self.clamp_relu {
            v.max(0.0)
        } else {
            v
        }
    }
    fn get_params(&self) -> Vec<f64> {
        self.params.clone()
    }
    fn set_params(&mut self, params: &[f64]) {
        assert_eq!(
            params.len(),
            self.params.len(),
            "LinearRateFunc: parameter length mismatch"
        );
        self.params = params.to_vec();
    }
    fn num_params(&self) -> usize {
        self.params.len()
    }
    fn eval_grad(&self, state: &State, t: f64) -> Option<(f64, Vec<f64>, State)> {
        let raw = self.raw(state, t);
        // ReLU subgradient at the clamp is 0, matching Go/JS.
        let active = !self.clamp_relu || raw > 0.0;
        let value = if self.clamp_relu { raw.max(0.0) } else { raw };

        let mut dparams = vec![0.0; self.params.len()];
        let mut dstate = State::new();
        if active {
            dparams[0] = 1.0;
            for (i, p) in self.places.iter().enumerate() {
                dparams[1 + i] = state.get(p).copied().unwrap_or(0.0);
                dstate.insert(p.clone(), self.params[1 + i]);
            }
            if self.time_dependent {
                dparams[1 + self.places.len()] = t;
            }
        } else {
            for p in &self.places {
                dstate.insert(p.clone(), 0.0);
            }
        }
        Some((value, dparams, dstate))
    }
}

/// A small MLP with one hidden layer, the hybrid mechanistic/neural rate:
/// `k(state, t) = W2 . sigma(W1 . [state; t] + b1) + b2`, optionally ReLU-clamped at the
/// output. Parameter layout (matching go-pflow's `MLPRateFunc` exactly, so the flat
/// packing here is the same one `EvalGrad`'s gradient uses): `[W1 (hidden*input,
/// row-major, W1[i*input+j]) | b1 (hidden) | W2 (hidden) | b2 (1)]`.
pub struct MLPRateFunc {
    places: Vec<String>,
    hidden_size: usize,
    time_dependent: bool,
    activation: Activation,
    clamp_output_relu: bool,
    params: Vec<f64>,
}

impl MLPRateFunc {
    /// Deterministic Xavier-like init, byte-for-byte go-pflow's
    /// `learn/ratefunc.go::NewMLPRateFunc` formula (re-verified against the live Go
    /// source this pass): `scale = sqrt(2 / input_size)`,
    /// `W1[i] = scale * ((i*7 + 13) % 100) / 100.0 - 0.5)` for `i` a flat index into `W1`
    /// only, `b1 = 0`, `W2 = 0`, `b2 = 0.1`. Pure integer/float arithmetic — no RNG
    /// dependency needed.
    pub fn new(
        places: &[String],
        hidden_size: usize,
        time_dependent: bool,
        activation: Activation,
        clamp_output_relu: bool,
    ) -> Self {
        let input_size = places.len() + if time_dependent { 1 } else { 0 };
        let w1_len = hidden_size * input_size;
        let num_params = w1_len + hidden_size + hidden_size + 1;
        let mut params = vec![0.0; num_params];

        let scale = (2.0 / input_size as f64).sqrt();
        for (i, p) in params.iter_mut().take(w1_len).enumerate() {
            *p = scale * (((i * 7 + 13) % 100) as f64 / 100.0 - 0.5);
        }
        // b1 (params[w1_len..w1_len+hidden]) and W2 (next `hidden`) stay 0.0.
        params[num_params - 1] = 0.1; // b2

        Self {
            places: places.to_vec(),
            hidden_size,
            time_dependent,
            activation,
            clamp_output_relu,
            params,
        }
    }

    fn input_size(&self) -> usize {
        self.places.len() + if self.time_dependent { 1 } else { 0 }
    }

    fn build_input(&self, state: &State, t: f64) -> Vec<f64> {
        let mut input: Vec<f64> = self
            .places
            .iter()
            .map(|p| state.get(p).copied().unwrap_or(0.0))
            .collect();
        if self.time_dependent {
            input.push(t);
        }
        input
    }

    /// Parameter slice offsets: `(w1_start, b1_start, w2_start, b2_index)`.
    fn offsets(&self) -> (usize, usize, usize, usize) {
        let input_size = self.input_size();
        let w1_len = self.hidden_size * input_size;
        let w1_start = 0;
        let b1_start = w1_len;
        let w2_start = b1_start + self.hidden_size;
        let b2_idx = w2_start + self.hidden_size;
        (w1_start, b1_start, w2_start, b2_idx)
    }

    /// Forward pass, keeping pre-activations (`z`) for the backward pass in `eval_grad`.
    fn forward(&self, state: &State, t: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>, f64) {
        let input = self.build_input(state, t);
        let input_size = input.len();
        let (w1_start, b1_start, w2_start, b2_idx) = self.offsets();
        let w1 = &self.params[w1_start..b1_start];
        let b1 = &self.params[b1_start..w2_start];
        let w2 = &self.params[w2_start..b2_idx];
        let b2 = self.params[b2_idx];

        let mut z = vec![0.0; self.hidden_size];
        let mut hidden = vec![0.0; self.hidden_size];
        for i in 0..self.hidden_size {
            let mut sum = b1[i];
            for j in 0..input_size {
                sum += w1[i * input_size + j] * input[j];
            }
            z[i] = sum;
            hidden[i] = match self.activation {
                Activation::Relu => sum.max(0.0),
                Activation::Tanh => sum.tanh(),
            };
        }
        let mut output = b2;
        for i in 0..self.hidden_size {
            output += w2[i] * hidden[i];
        }
        (input, z, hidden, output)
    }
}

impl RateFunc for MLPRateFunc {
    fn eval(&self, state: &State, t: f64) -> f64 {
        let (_, _, _, output) = self.forward(state, t);
        if self.clamp_output_relu {
            output.max(0.0)
        } else {
            output
        }
    }

    fn get_params(&self) -> Vec<f64> {
        self.params.clone()
    }

    fn set_params(&mut self, params: &[f64]) {
        assert_eq!(
            params.len(),
            self.params.len(),
            "MLPRateFunc: parameter length mismatch"
        );
        self.params = params.to_vec();
    }

    fn num_params(&self) -> usize {
        self.params.len()
    }

    fn eval_grad(&self, state: &State, t: f64) -> Option<(f64, Vec<f64>, State)> {
        let (input, z, hidden, output) = self.forward(state, t);
        let input_size = input.len();
        let (w1_start, b1_start, w2_start, b2_idx) = self.offsets();
        let w1 = &self.params[w1_start..b1_start];
        let w2 = &self.params[w2_start..b2_idx];

        // Output-clamp fired: all gradients are the zero subgradient, matching Go/JS.
        if self.clamp_output_relu && output < 0.0 {
            return Some((0.0, vec![0.0; self.params.len()], State::new()));
        }

        // Backward pass, upstream derivative 1: d[i] = W2[i] * sigma'(z[i]).
        let mut d = vec![0.0; self.hidden_size];
        for i in 0..self.hidden_size {
            let sp = match self.activation {
                Activation::Relu => {
                    if z[i] > 0.0 {
                        1.0
                    } else {
                        0.0
                    }
                }
                Activation::Tanh => 1.0 - hidden[i] * hidden[i],
            };
            d[i] = w2[i] * sp;
        }

        let mut grad = vec![0.0; self.params.len()];
        for i in 0..self.hidden_size {
            for j in 0..input_size {
                grad[i * input_size + j] = d[i] * input[j]; // dW1
            }
            grad[b1_start + i] = d[i]; // db1
            grad[w2_start + i] = hidden[i]; // dW2
        }
        grad[self.params.len() - 1] = 1.0; // db2

        // dk/dstate[place_j] = sum_i d[i] * W1[i*input_size + j]; the time input column
        // (if any, at the end of `input`) is dropped — time isn't a state variable.
        let mut dstate = State::new();
        for (j, place) in self.places.iter().enumerate() {
            let mut sum = 0.0;
            for i in 0..self.hidden_size {
                sum += d[i] * w1[i * input_size + j];
            }
            *dstate.entry(place.clone()).or_insert(0.0) += sum;
        }

        Some((output, grad, dstate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(pairs: &[(&str, f64)]) -> State {
        pairs.iter().map(|&(k, v)| (k.to_string(), v)).collect()
    }

    /// Central-difference `eval` w.r.t. one parameter, holding state/t fixed.
    fn fd_dparam(rf: &mut MLPRateFunc, i: usize, state_: &State, t: f64) -> f64 {
        let params = rf.get_params();
        let h = 1e-6 * (1.0 + params[i].abs());
        let mut p = params.clone();
        p[i] = params[i] + h;
        rf.set_params(&p);
        let f_plus = rf.eval(state_, t);
        p[i] = params[i] - h;
        rf.set_params(&p);
        let f_minus = rf.eval(state_, t);
        rf.set_params(&params);
        (f_plus - f_minus) / (2.0 * h)
    }

    /// Central-difference `eval` w.r.t. one state place, holding params/t fixed.
    fn fd_dstate(rf: &MLPRateFunc, place: &str, state_: &State, t: f64) -> f64 {
        let v = state_.get(place).copied().unwrap_or(0.0);
        let h = 1e-6 * (1.0 + v.abs());
        let mut s_plus = state_.clone();
        s_plus.insert(place.to_string(), v + h);
        let mut s_minus = state_.clone();
        s_minus.insert(place.to_string(), v - h);
        (rf.eval(&s_plus, t) - rf.eval(&s_minus, t)) / (2.0 * h)
    }

    #[test]
    fn mlp_relu_eval_grad_matches_finite_differences() {
        let places = vec!["A".to_string(), "B".to_string()];
        let mut rf = MLPRateFunc::new(&places, 4, false, Activation::Relu, false);
        let s = state(&[("A", 2.3), ("B", 0.7)]);
        let t = 0.0;

        let (value, dparams, dstate) = rf.eval_grad(&s, t).expect("analytic grad");
        assert_eq!(value, rf.eval(&s, t));

        for i in 0..dparams.len() {
            let fd = fd_dparam(&mut rf, i, &s, t);
            let scale = 1e-6 + 1e-4 * dparams[i].abs().max(fd.abs());
            assert!(
                (dparams[i] - fd).abs() <= scale,
                "param {i}: analytic {} vs fd {fd}",
                dparams[i]
            );
        }
        for place in &places {
            let analytic = dstate.get(place).copied().unwrap_or(0.0);
            let fd = fd_dstate(&rf, place, &s, t);
            let scale = 1e-6 + 1e-4 * analytic.abs().max(fd.abs());
            assert!(
                (analytic - fd).abs() <= scale,
                "state {place}: analytic {analytic} vs fd {fd}"
            );
        }
    }

    #[test]
    fn mlp_tanh_time_dependent_eval_grad_matches_finite_differences() {
        let places = vec!["A".to_string()];
        let mut rf = MLPRateFunc::new(&places, 3, true, Activation::Tanh, false);
        let s = state(&[("A", 1.4)]);
        let t = 0.6;

        let (_, dparams, dstate) = rf.eval_grad(&s, t).expect("analytic grad");
        for i in 0..dparams.len() {
            let fd = fd_dparam(&mut rf, i, &s, t);
            let scale = 1e-6 + 1e-4 * dparams[i].abs().max(fd.abs());
            assert!(
                (dparams[i] - fd).abs() <= scale,
                "param {i}: analytic {} vs fd {fd}",
                dparams[i]
            );
        }
        let analytic = dstate.get("A").copied().unwrap_or(0.0);
        let fd = fd_dstate(&rf, "A", &s, t);
        let scale = 1e-6 + 1e-4 * analytic.abs().max(fd.abs());
        assert!(
            (analytic - fd).abs() <= scale,
            "state A: analytic {analytic} vs fd {fd}"
        );
    }

    #[test]
    fn mlp_output_clamp_zeroes_gradient() {
        // Force a negative output by driving b2 (the last param) very negative, then
        // confirm the ReLU-clamped eval_grad reports the zero subgradient everywhere.
        let places = vec!["A".to_string()];
        let mut rf = MLPRateFunc::new(&places, 2, false, Activation::Relu, true);
        let mut params = rf.get_params();
        let last = params.len() - 1;
        params[last] = -100.0;
        rf.set_params(&params);

        let s = state(&[("A", 1.0)]);
        let (value, dparams, dstate) = rf.eval_grad(&s, 0.0).expect("analytic grad");
        assert_eq!(value, 0.0);
        assert!(dparams.iter().all(|&g| g == 0.0));
        assert!(dstate.is_empty() || dstate.values().all(|&g| g == 0.0));
    }
}
