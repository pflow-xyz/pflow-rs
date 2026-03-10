//! Generic incidence matrix type with Petri net firing semantics.
//!
//! `NetMatrix` owns the dense incidence data, input weights, initial marking,
//! and labels. It provides firing, enabledness, and tropical adjacency without
//! depending on any pflow crate.

use crate::semiring::Matrix;

/// A standalone Petri net representation as dense matrices.
///
/// - `incidence[t][p]` = net token change when transition `t` fires (output - input)
/// - `input[t][p]` = tokens consumed (needed for enabledness with catalytic arcs)
/// - `initial[p]` = initial marking
#[derive(Debug, Clone)]
pub struct NetMatrix {
    pub incidence: Vec<Vec<i64>>,
    pub input: Vec<Vec<i64>>,
    pub initial: Vec<i64>,
    pub place_labels: Vec<String>,
    pub transition_labels: Vec<String>,
}

impl NetMatrix {
    /// Create a `NetMatrix` with explicit input weights (supports catalytic arcs).
    pub fn new(
        incidence: Vec<Vec<i64>>,
        input: Vec<Vec<i64>>,
        initial: Vec<i64>,
        place_labels: Vec<String>,
        transition_labels: Vec<String>,
    ) -> Self {
        let nt = incidence.len();
        let np = if nt > 0 { incidence[0].len() } else { 0 };
        assert_eq!(input.len(), nt, "input rows must match transitions");
        assert_eq!(initial.len(), np, "initial must match places");
        for (t, row) in incidence.iter().enumerate() {
            assert_eq!(row.len(), np, "incidence row {t} length mismatch");
        }
        for (t, row) in input.iter().enumerate() {
            assert_eq!(row.len(), np, "input row {t} length mismatch");
        }
        Self { incidence, input, initial, place_labels, transition_labels }
    }

    /// Convenience: derive input weights from negative incidence entries.
    ///
    /// Correct for non-catalytic nets where consuming `k` tokens means
    /// `incidence[t][p] = -k` (no simultaneous produce to same place).
    pub fn from_incidence(
        incidence: Vec<Vec<i64>>,
        initial: Vec<i64>,
        place_labels: Vec<String>,
        transition_labels: Vec<String>,
    ) -> Self {
        let input: Vec<Vec<i64>> = incidence
            .iter()
            .map(|row| row.iter().map(|&v| if v < 0 { -v } else { 0 }).collect())
            .collect();
        Self::new(incidence, input, initial, place_labels, transition_labels)
    }

    pub fn num_places(&self) -> usize {
        self.initial.len()
    }

    pub fn num_transitions(&self) -> usize {
        self.incidence.len()
    }

    /// Delta vector for transition `t`: the net token change per place.
    pub fn delta(&self, t: usize) -> &[i64] {
        &self.incidence[t]
    }

    /// Check if transition `t` is enabled at the given marking.
    pub fn is_enabled(&self, marking: &[i64], t: usize) -> bool {
        self.input[t].iter().enumerate().all(|(p, &w)| marking[p] >= w)
    }

    /// Fire transition `t` at the given marking. Returns `None` if not enabled.
    pub fn fire(&self, marking: &[i64], t: usize) -> Option<Vec<i64>> {
        if !self.is_enabled(marking, t) {
            return None;
        }
        let next: Vec<i64> = marking
            .iter()
            .enumerate()
            .map(|(p, &m)| m + self.incidence[t][p])
            .collect();
        // Check non-negative (sanity)
        if next.iter().any(|&v| v < 0) {
            return None;
        }
        Some(next)
    }

    /// Build the tropical adjacency matrix (transition-to-transition).
    ///
    /// `A[i][j]` = weight of the path from transition `j` through an
    /// intermediate place to transition `i`. NEG_INF means no connection.
    pub fn tropical_adjacency(&self) -> Matrix {
        let nt = self.num_transitions();
        let np = self.num_places();
        let mut a = Matrix::new(nt, nt);

        // Compute output weights: output[t][p] = incidence[t][p] + input[t][p]
        // (since incidence = output - input)
        for p in 0..np {
            // Find producers: transitions with positive output to place p
            let mut producers: Vec<(usize, i64)> = Vec::new();
            for t in 0..nt {
                let out_w = self.incidence[t][p] + self.input[t][p];
                if out_w > 0 {
                    producers.push((t, out_w));
                }
            }
            // Find consumers: transitions that consume from place p
            let mut consumers: Vec<(usize, i64)> = Vec::new();
            for t in 0..nt {
                if self.input[t][p] > 0 {
                    consumers.push((t, self.input[t][p]));
                }
            }
            for &(producer, _) in &producers {
                for &(consumer, cw) in &consumers {
                    let candidate = cw as f64;
                    if candidate > a.at(consumer, producer) {
                        a.set(consumer, producer, candidate);
                    }
                }
            }
        }
        a
    }

    /// P-invariants (convenience delegation).
    pub fn p_invariants(&self) -> Vec<Vec<i64>> {
        crate::invariants::p_invariants_from_dense(&self.incidence)
    }

    /// T-invariants (convenience delegation).
    pub fn t_invariants(&self) -> Vec<Vec<i64>> {
        crate::invariants::t_invariants_from_dense(&self.incidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::NEG_INF;

    #[test]
    fn test_simple_loop() {
        let net = NetMatrix::from_incidence(
            vec![vec![-1, 1], vec![1, -1]],
            vec![1, 0],
            vec!["P0".into(), "P1".into()],
            vec!["T0".into(), "T1".into()],
        );
        assert!(net.is_enabled(&net.initial, 0));
        assert!(!net.is_enabled(&net.initial, 1));
        let m1 = net.fire(&net.initial, 0).unwrap();
        assert_eq!(m1, vec![0, 1]);
        let m2 = net.fire(&m1, 1).unwrap();
        assert_eq!(m2, vec![1, 0]);
    }

    #[test]
    fn test_catalytic_arc() {
        // Transition reads from p0 (consumes+produces) and produces to p1
        // incidence: [0, 1] but input: [1, 0]
        let net = NetMatrix::new(
            vec![vec![0, 1]],
            vec![vec![1, 0]],
            vec![1, 0],
            vec!["p0".into(), "p1".into()],
            vec!["t0".into()],
        );
        assert!(net.is_enabled(&[1, 0], 0));
        assert!(!net.is_enabled(&[0, 0], 0)); // needs token in p0
        let m1 = net.fire(&[1, 0], 0).unwrap();
        assert_eq!(m1, vec![1, 1]); // p0 unchanged (catalytic), p1 gains
    }

    #[test]
    fn test_tropical_adjacency() {
        let net = NetMatrix::from_incidence(
            vec![vec![-1, 1], vec![1, -1]],
            vec![1, 0],
            vec!["P0".into(), "P1".into()],
            vec!["T0".into(), "T1".into()],
        );
        let a = net.tropical_adjacency();
        assert_eq!(a.at(1, 0), 1.0); // T1 depends on T0 via P1
        assert_eq!(a.at(0, 1), 1.0); // T0 depends on T1 via P0
        assert_eq!(a.at(0, 0), NEG_INF);
        assert_eq!(a.at(1, 1), NEG_INF);
    }
}
