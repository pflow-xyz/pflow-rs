//! Extract a Petri net from sparse tropical structure.
//!
//! A sparse tropical matrix maps to Petri net topology:
//! - Rows = transitions, columns = places
//! - Negative entries = input arcs (consume tokens)
//! - Positive entries = output arcs (produce tokens)
//! - NEG_INF = no arc (structural zero)

use crate::net_matrix::NetMatrix;
use crate::semiring::{Matrix, NEG_INF};

#[cfg(feature = "pflow")]
use pflow_zk::IncidenceMatrix;

/// Extract a `NetMatrix` from a sparse tropical matrix.
///
/// The matrix must have integer entries (run Factor first).
/// Rows are transitions, columns are places.
/// Entry values become incidence weights directly.
/// Initial marking is set to zero (caller can override).
pub fn extract(m: &Matrix) -> Result<NetMatrix, String> {
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

    let initial = vec![0i64; places];
    let place_labels: Vec<String> = (0..places).map(|i| format!("p{i}")).collect();
    let transition_labels: Vec<String> = (0..transitions).map(|i| format!("t{i}")).collect();

    Ok(NetMatrix::from_incidence(incidence, initial, place_labels, transition_labels))
}

/// Convert a `NetMatrix` to a pflow-zk `IncidenceMatrix`.
#[cfg(feature = "pflow")]
pub fn to_incidence_matrix(net: &NetMatrix) -> IncidenceMatrix {
    let mut inputs: Vec<Vec<(usize, i64)>> = Vec::new();
    let mut outputs: Vec<Vec<(usize, i64)>> = Vec::new();

    for t in 0..net.num_transitions() {
        let mut t_inputs = Vec::new();
        let mut t_outputs = Vec::new();
        for p in 0..net.num_places() {
            let in_w = net.input[t][p];
            let out_w = net.incidence[t][p] + in_w; // output = incidence + input
            if in_w > 0 {
                t_inputs.push((p, in_w));
            }
            if out_w > 0 {
                t_outputs.push((p, out_w));
            }
        }
        inputs.push(t_inputs);
        outputs.push(t_outputs);
    }

    IncidenceMatrix {
        place_labels: net.place_labels.clone(),
        transition_labels: net.transition_labels.clone(),
        inputs,
        outputs,
        num_places: net.num_places(),
        num_transitions: net.num_transitions(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_petri_net() {
        let mut m = Matrix::new(2, 3);
        m.set(0, 0, -1.0);
        m.set(0, 1, 1.0);
        m.set(1, 0, 1.0);
        m.set(1, 1, -1.0);

        let net = extract(&m).unwrap();
        assert_eq!(net.num_transitions(), 2);
        assert_eq!(net.num_places(), 3);
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

    #[cfg(feature = "pflow")]
    #[test]
    fn test_to_incidence_matrix() {
        let mut m = Matrix::new(2, 2);
        m.set(0, 0, -1.0);
        m.set(0, 1, 1.0);
        m.set(1, 0, 1.0);
        m.set(1, 1, -1.0);

        let net = extract(&m).unwrap();
        let im = to_incidence_matrix(&net);

        assert_eq!(im.num_places, 2);
        assert_eq!(im.num_transitions, 2);
        assert_eq!(im.inputs[0], vec![(0, 1)]);
        assert_eq!(im.outputs[0], vec![(1, 1)]);
        assert_eq!(im.inputs[1], vec![(1, 1)]);
        assert_eq!(im.outputs[1], vec![(0, 1)]);
    }
}
