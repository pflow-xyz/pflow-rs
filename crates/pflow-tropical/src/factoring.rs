//! Factor a dense ReLU weight matrix into sparse tropical structure.
//!
//! ReLU(x) = max(0, x) is a tropical polynomial. A trained ReLU layer's
//! weight matrix can be sparsified by thresholding and rounding to recover
//! integer arc weights suitable for Petri net interpretation.

use crate::semiring::{Matrix, NEG_INF};

/// Configuration for factoring a dense weight matrix.
pub struct FactorConfig {
    /// Weights with absolute value below this become NEG_INF (structural zeros).
    pub threshold: f64,
    /// If true, round surviving weights to the nearest integer.
    pub round_to_int: bool,
}

impl Default for FactorConfig {
    fn default() -> Self {
        Self {
            threshold: 1e-2,
            round_to_int: true,
        }
    }
}

/// Factor trait: convert dense weight data into a sparse tropical matrix.
pub trait Factor {
    fn factor(&self, cfg: &FactorConfig) -> Matrix;
}

impl Factor for Vec<Vec<f64>> {
    /// Threshold small weights to NEG_INF, optionally round survivors to integers.
    fn factor(&self, cfg: &FactorConfig) -> Matrix {
        let rows = self.len();
        let cols = if rows > 0 { self[0].len() } else { 0 };
        let mut m = Matrix::new(rows, cols);
        for i in 0..rows {
            for j in 0..cols {
                let w = self[i][j];
                if w.abs() < cfg.threshold {
                    m.set(i, j, NEG_INF);
                } else if cfg.round_to_int {
                    m.set(i, j, w.round());
                } else {
                    m.set(i, j, w);
                }
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factor_thresholds_and_rounds() {
        let weights = vec![
            vec![2.7, 0.001, -1.3],
            vec![0.005, 3.9, 0.008],
        ];
        let m = weights.factor(&FactorConfig::default());

        assert_eq!(m.at(0, 0), 3.0);  // 2.7 rounds to 3
        assert_eq!(m.at(0, 1), NEG_INF);  // 0.001 < threshold
        assert_eq!(m.at(0, 2), -1.0);  // -1.3 rounds to -1
        assert_eq!(m.at(1, 0), NEG_INF);  // 0.005 < threshold
        assert_eq!(m.at(1, 1), 4.0);  // 3.9 rounds to 4
        assert_eq!(m.at(1, 2), NEG_INF);  // 0.008 < threshold
    }

    #[test]
    fn test_factor_no_rounding() {
        let weights = vec![vec![2.7, 0.001]];
        let cfg = FactorConfig {
            threshold: 1e-2,
            round_to_int: false,
        };
        let m = weights.factor(&cfg);
        assert!((m.at(0, 0) - 2.7).abs() < 1e-10);
        assert_eq!(m.at(0, 1), NEG_INF);
    }
}
