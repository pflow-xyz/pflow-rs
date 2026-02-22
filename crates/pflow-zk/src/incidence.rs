//! Incidence matrix extraction from Petri nets.
//!
//! Converts a `PetriNet` into a canonical integer representation suitable
//! for ZK circuit construction. Places and transitions are sorted
//! alphabetically to ensure deterministic ordering.

use pflow_core::PetriNet;

/// Incidence matrix representation of a Petri net topology.
///
/// All indices refer to the canonical (sorted) ordering of places and transitions.
#[derive(Debug, Clone)]
pub struct IncidenceMatrix {
    /// Place labels in sorted order.
    pub place_labels: Vec<String>,
    /// Transition labels in sorted order.
    pub transition_labels: Vec<String>,
    /// Per-transition input arcs: `inputs[t]` = `[(place_idx, weight), ...]`.
    pub inputs: Vec<Vec<(usize, i64)>>,
    /// Per-transition output arcs: `outputs[t]` = `[(place_idx, weight), ...]`.
    pub outputs: Vec<Vec<(usize, i64)>>,
    /// Number of places.
    pub num_places: usize,
    /// Number of transitions.
    pub num_transitions: usize,
}

impl IncidenceMatrix {
    /// Extracts an incidence matrix from a `PetriNet`.
    ///
    /// Places and transitions are sorted alphabetically for canonical ordering.
    /// Arc weights are rounded to the nearest integer (Petri nets use `f64`
    /// internally for ODE compatibility, but ZK proofs need integers).
    pub fn from_petri_net(net: &PetriNet) -> Self {
        let mut place_labels: Vec<String> = net.places.keys().cloned().collect();
        place_labels.sort();

        let mut transition_labels: Vec<String> = net.transitions.keys().cloned().collect();
        transition_labels.sort();

        let place_index: std::collections::HashMap<&str, usize> = place_labels
            .iter()
            .enumerate()
            .map(|(i, l)| (l.as_str(), i))
            .collect();

        let num_places = place_labels.len();
        let num_transitions = transition_labels.len();

        let mut inputs = vec![Vec::new(); num_transitions];
        let mut outputs = vec![Vec::new(); num_transitions];

        for arc in &net.arcs {
            if arc.inhibit_transition {
                continue; // inhibitor arcs are not part of the incidence matrix
            }

            let weight = arc.weight_sum().round() as i64;
            if weight == 0 {
                continue;
            }

            // Input arc: place → transition (source is a place, target is a transition)
            if let Some(&t_idx) = place_index.get(arc.source.as_str()) {
                if let Some(trans_pos) = transition_labels.iter().position(|l| l == &arc.target) {
                    inputs[trans_pos].push((t_idx, weight));
                }
            }

            // Output arc: transition → place (source is a transition, target is a place)
            if let Some(&p_idx) = place_index.get(arc.target.as_str()) {
                if let Some(trans_pos) = transition_labels.iter().position(|l| l == &arc.source) {
                    outputs[trans_pos].push((p_idx, weight));
                }
            }
        }

        // Sort each arc list by place index for determinism
        for arcs in &mut inputs {
            arcs.sort_by_key(|&(idx, _)| idx);
        }
        for arcs in &mut outputs {
            arcs.sort_by_key(|&(idx, _)| idx);
        }

        Self {
            place_labels,
            transition_labels,
            inputs,
            outputs,
            num_places,
            num_transitions,
        }
    }

    /// Returns the initial marking vector from the Petri net in canonical order.
    pub fn initial_marking(&self, net: &PetriNet) -> Vec<i64> {
        self.place_labels
            .iter()
            .map(|label| {
                net.places
                    .get(label)
                    .map(|p| p.token_count().round() as i64)
                    .unwrap_or(0)
            })
            .collect()
    }

    /// Computes the delta vector for a transition: `delta[p] = output[p] - input[p]`.
    pub fn delta(&self, transition_id: usize) -> Vec<i64> {
        let mut d = vec![0i64; self.num_places];
        for &(p, w) in &self.inputs[transition_id] {
            d[p] -= w;
        }
        for &(p, w) in &self.outputs[transition_id] {
            d[p] += w;
        }
        d
    }

    /// Checks whether a transition is enabled at the given marking.
    pub fn is_enabled(&self, marking: &[i64], transition_id: usize) -> bool {
        for &(p, w) in &self.inputs[transition_id] {
            if marking[p] < w {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sir_net() -> PetriNet {
        PetriNet::build().sir(999.0, 1.0, 0.0).done()
    }

    #[test]
    fn test_sir_extraction() {
        let net = sir_net();
        let matrix = IncidenceMatrix::from_petri_net(&net);

        // 3 places: I, R, S (sorted)
        assert_eq!(matrix.place_labels, vec!["I", "R", "S"]);
        // 2 transitions: infect, recover (sorted)
        assert_eq!(matrix.transition_labels, vec!["infect", "recover"]);
        assert_eq!(matrix.num_places, 3);
        assert_eq!(matrix.num_transitions, 2);
    }

    #[test]
    fn test_sir_inputs_outputs() {
        let net = sir_net();
        let matrix = IncidenceMatrix::from_petri_net(&net);

        // infect (t=0): inputs from I(0) w=1, S(2) w=1; output to I(0) w=2
        let infect_inputs = &matrix.inputs[0];
        assert_eq!(infect_inputs, &[(0, 1), (2, 1)]); // I=0, S=2
        let infect_outputs = &matrix.outputs[0];
        assert_eq!(infect_outputs, &[(0, 2)]); // I=0

        // recover (t=1): input from I(0) w=1; output to R(1) w=1
        let recover_inputs = &matrix.inputs[1];
        assert_eq!(recover_inputs, &[(0, 1)]); // I=0
        let recover_outputs = &matrix.outputs[1];
        assert_eq!(recover_outputs, &[(1, 1)]); // R=1
    }

    #[test]
    fn test_sir_delta() {
        let net = sir_net();
        let matrix = IncidenceMatrix::from_petri_net(&net);

        // infect delta: I +1, R 0, S -1
        let d = matrix.delta(0);
        assert_eq!(d, vec![1, 0, -1]); // [I, R, S]

        // recover delta: I -1, R +1, S 0
        let d = matrix.delta(1);
        assert_eq!(d, vec![-1, 1, 0]); // [I, R, S]
    }

    #[test]
    fn test_sir_initial_marking() {
        let net = sir_net();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let m0 = matrix.initial_marking(&net);
        // Canonical order: I=1, R=0, S=999
        assert_eq!(m0, vec![1, 0, 999]);
    }

    #[test]
    fn test_sir_enabledness() {
        let net = sir_net();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let m0 = matrix.initial_marking(&net);

        // infect requires I>=1 and S>=1 — both satisfied
        assert!(matrix.is_enabled(&m0, 0));
        // recover requires I>=1 — satisfied
        assert!(matrix.is_enabled(&m0, 1));

        // If I=0, neither transition is enabled
        let m_no_i = vec![0, 0, 999];
        assert!(!matrix.is_enabled(&m_no_i, 0));
        assert!(!matrix.is_enabled(&m_no_i, 1));
    }

    #[test]
    fn test_tictactoe_extraction() {
        // Build a 3x3 tic-tac-toe net
        let mut b = PetriNet::build();
        for i in 0..3 {
            for j in 0..3 {
                b = b.place(&format!("P{}_{}", i, j), 1.0);
                b = b.place(&format!("_X{}_{}", i, j), 0.0);
                b = b.place(&format!("_O{}_{}", i, j), 0.0);
            }
        }
        for i in 0..3 {
            for j in 0..3 {
                b = b.transition(&format!("PlayX{}_{}", i, j));
                b = b.arc(&format!("P{}_{}", i, j), &format!("PlayX{}_{}", i, j), 1.0);
                b = b.arc(&format!("PlayX{}_{}", i, j), &format!("_X{}_{}", i, j), 1.0);

                b = b.transition(&format!("PlayO{}_{}", i, j));
                b = b.arc(&format!("P{}_{}", i, j), &format!("PlayO{}_{}", i, j), 1.0);
                b = b.arc(&format!("PlayO{}_{}", i, j), &format!("_O{}_{}", i, j), 1.0);
            }
        }
        let net = b.done();
        let matrix = IncidenceMatrix::from_petri_net(&net);

        // 27 places (9 cells * 3 states each), 18 transitions (9 cells * 2 players)
        assert_eq!(matrix.num_places, 27);
        assert_eq!(matrix.num_transitions, 18);

        // All labels should be sorted
        for i in 1..matrix.place_labels.len() {
            assert!(matrix.place_labels[i - 1] <= matrix.place_labels[i]);
        }
        for i in 1..matrix.transition_labels.len() {
            assert!(matrix.transition_labels[i - 1] <= matrix.transition_labels[i]);
        }
    }
}
