//! Extract a pflow-compatible incidence matrix from sparse tropical structure.
//!
//! A sparse tropical matrix maps to Petri net topology:
//! - Rows = transitions, columns = places
//! - Negative entries = input arcs (consume tokens)
//! - Positive entries = output arcs (produce tokens)
//! - NEG_INF = no arc (structural zero)

use crate::semiring::{Matrix, NEG_INF};
use pflow_zk::IncidenceMatrix;

/// Petri net extracted from tropical structure.
#[derive(Debug, Clone)]
pub struct PflowNet {
    pub places: usize,
    pub transitions: usize,
    /// Incidence matrix [transition][place]: negative = consume, positive = produce.
    pub incidence: Vec<Vec<i64>>,
    /// Transition labels (auto-generated as "t0", "t1", ...).
    pub transition_labels: Vec<String>,
    /// Place labels (auto-generated as "p0", "p1", ...).
    pub place_labels: Vec<String>,
}

impl PflowNet {
    /// Convert to pflow-zk IncidenceMatrix for ZK proof integration.
    pub fn to_incidence_matrix(&self) -> IncidenceMatrix {
        let mut inputs: Vec<Vec<(usize, i64)>> = Vec::new();
        let mut outputs: Vec<Vec<(usize, i64)>> = Vec::new();

        for t in 0..self.transitions {
            let mut t_inputs = Vec::new();
            let mut t_outputs = Vec::new();
            for p in 0..self.places {
                let w = self.incidence[t][p];
                if w < 0 {
                    t_inputs.push((p, -w)); // input weight is positive in IncidenceMatrix
                } else if w > 0 {
                    t_outputs.push((p, w));
                }
            }
            inputs.push(t_inputs);
            outputs.push(t_outputs);
        }

        IncidenceMatrix {
            place_labels: self.place_labels.clone(),
            transition_labels: self.transition_labels.clone(),
            inputs,
            outputs,
            num_places: self.places,
            num_transitions: self.transitions,
        }
    }
}

/// Extract a PflowNet from a sparse tropical matrix.
///
/// The matrix must have integer entries (run Factor first).
/// Rows are transitions, columns are places.
/// Entry values become incidence weights directly.
pub fn extract(m: &Matrix) -> Result<PflowNet, String> {
    let transitions = m.rows;
    let places = m.cols;
    let mut incidence = vec![vec![0i64; places]; transitions];

    for t in 0..transitions {
        for p in 0..places {
            let v = m.at(t, p);
            if v == NEG_INF {
                continue; // no arc
            }
            // Verify integer
            if (v - v.round()).abs() > 1e-9 {
                return Err(format!(
                    "non-integer entry at [{t}][{p}] = {v}; run Factor with round_to_int first"
                ));
            }
            incidence[t][p] = v.round() as i64;
        }
    }

    Ok(PflowNet {
        places,
        transitions,
        incidence,
        transition_labels: (0..transitions).map(|i| format!("t{i}")).collect(),
        place_labels: (0..places).map(|i| format!("p{i}")).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_petri_net() {
        // Known net: T0 consumes P0 produces P1, T1 consumes P1 produces P0
        // Incidence: T0 = [-1, +1, 0], T1 = [+1, -1, 0]
        let mut m = Matrix::new(2, 3);
        m.set(0, 0, -1.0); // T0 consumes P0
        m.set(0, 1, 1.0);  // T0 produces P1
        // m[0][2] = NEG_INF (no arc to P2)
        m.set(1, 0, 1.0);  // T1 produces P0
        m.set(1, 1, -1.0); // T1 consumes P1
        // m[1][2] = NEG_INF

        let net = extract(&m).unwrap();
        assert_eq!(net.transitions, 2);
        assert_eq!(net.places, 3);
        assert_eq!(net.incidence[0], vec![-1, 1, 0]);
        assert_eq!(net.incidence[1], vec![1, -1, 0]);
    }

    #[test]
    fn test_non_integer_error() {
        let mut m = Matrix::new(1, 1);
        m.set(0, 0, 1.5);
        let result = extract(&m);
        assert!(result.is_err());
    }

    #[test]
    fn test_to_incidence_matrix() {
        let mut m = Matrix::new(2, 2);
        m.set(0, 0, -1.0);
        m.set(0, 1, 1.0);
        m.set(1, 0, 1.0);
        m.set(1, 1, -1.0);

        let net = extract(&m).unwrap();
        let im = net.to_incidence_matrix();

        assert_eq!(im.num_places, 2);
        assert_eq!(im.num_transitions, 2);
        // T0: consumes p0 (input), produces p1 (output)
        assert_eq!(im.inputs[0], vec![(0, 1)]);
        assert_eq!(im.outputs[0], vec![(1, 1)]);
        // T1: consumes p1 (input), produces p0 (output)
        assert_eq!(im.inputs[1], vec![(1, 1)]);
        assert_eq!(im.outputs[1], vec![(0, 1)]);
    }
}
