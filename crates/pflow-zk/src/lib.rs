//! ZK proof infrastructure for Petri nets.
//!
//! Provides a shared `PetriProver` trait and supporting types for proving
//! Petri net transition firings in zero knowledge. Two backend implementations
//! exist:
//!
//! - **pflow-zk-arkworks**: Compiles net topology into R1CS constraints (Groth16 + Poseidon)
//! - **pflow-zk-risc0**: Wraps the simulator in a zkVM guest (STARK proofs)

pub mod incidence;
pub mod types;

pub use incidence::IncidenceMatrix;
pub use types::*;

use thiserror::Error;

/// Errors from ZK proof operations.
#[derive(Error, Debug)]
pub enum ZkError {
    #[error("setup has not been run")]
    NotSetup,
    #[error("transition {0} is not enabled at the given marking")]
    NotEnabled(TransitionId),
    #[error("transition id {0} out of range (net has {1} transitions)")]
    InvalidTransition(TransitionId, usize),
    #[error("marking length {got} does not match expected {expected}")]
    MarkingMismatch { expected: usize, got: usize },
    #[error("proof generation failed: {0}")]
    ProofGeneration(String),
    #[error("proof verification failed: {0}")]
    Verification(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

pub type Result<T> = std::result::Result<T, ZkError>;

/// Trait for ZK proof systems over Petri nets.
///
/// Each implementation proves the same statement: "given a Petri net topology,
/// a transition firing transforms marking M into marking M'."
pub trait PetriProver: Send + Sync {
    /// Run any required setup (e.g., trusted setup, key generation).
    fn setup(&mut self) -> Result<()>;

    /// Generate a proof for a transition firing.
    fn prove(&self, witness: &TransitionWitness) -> Result<Proof>;

    /// Verify a proof.
    fn verify(&self, proof: &Proof) -> Result<bool>;

    /// Return the serialized verifying key.
    fn verifying_key(&self) -> Result<Vec<u8>>;

    /// Name of the proof system (e.g., "groth16", "risc0").
    fn system_name(&self) -> &'static str;
}

/// Fire a transition on a marking, returning the new marking.
///
/// This is the reference implementation used by all provers.
/// Returns an error if the transition is not enabled.
pub fn fire_transition(
    matrix: &IncidenceMatrix,
    marking: &Marking,
    transition_id: TransitionId,
) -> Result<Marking> {
    if transition_id >= matrix.num_transitions {
        return Err(ZkError::InvalidTransition(
            transition_id,
            matrix.num_transitions,
        ));
    }
    if marking.len() != matrix.num_places {
        return Err(ZkError::MarkingMismatch {
            expected: matrix.num_places,
            got: marking.len(),
        });
    }
    if !matrix.is_enabled(marking, transition_id) {
        return Err(ZkError::NotEnabled(transition_id));
    }

    let delta = matrix.delta(transition_id);
    let post: Marking = marking
        .iter()
        .zip(delta.iter())
        .map(|(m, d)| m + d)
        .collect();
    Ok(post)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::PetriNet;

    #[test]
    fn test_fire_sir_infect() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let m0 = matrix.initial_marking(&net);

        // Fire "infect" (transition 0): S-1, I+1
        let m1 = fire_transition(&matrix, &m0, 0).unwrap();
        // Canonical order: I, R, S
        assert_eq!(m1, vec![2, 0, 998]);
    }

    #[test]
    fn test_fire_sir_recover() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let m0 = matrix.initial_marking(&net);

        // Fire "recover" (transition 1): I-1, R+1
        let m1 = fire_transition(&matrix, &m0, 1).unwrap();
        assert_eq!(m1, vec![0, 1, 999]);
    }

    #[test]
    fn test_fire_sequence() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let m0 = matrix.initial_marking(&net);

        // infect, infect, recover
        let m1 = fire_transition(&matrix, &m0, 0).unwrap();
        let m2 = fire_transition(&matrix, &m1, 0).unwrap();
        let m3 = fire_transition(&matrix, &m2, 1).unwrap();
        // After 2 infects and 1 recover: I=2, R=1, S=997
        assert_eq!(m3, vec![2, 1, 997]);
    }

    #[test]
    fn test_fire_not_enabled() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);

        // I=0 means neither transition is enabled
        let m = vec![0, 0, 999];
        let result = fire_transition(&matrix, &m, 0);
        assert!(matches!(result, Err(ZkError::NotEnabled(0))));
    }

    #[test]
    fn test_fire_invalid_transition() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let m0 = matrix.initial_marking(&net);

        let result = fire_transition(&matrix, &m0, 5);
        assert!(matches!(result, Err(ZkError::InvalidTransition(5, 2))));
    }

    #[test]
    fn test_fire_marking_mismatch() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);

        let bad_marking = vec![1, 2]; // too short
        let result = fire_transition(&matrix, &bad_marking, 0);
        assert!(matches!(
            result,
            Err(ZkError::MarkingMismatch {
                expected: 3,
                got: 2
            })
        ));
    }

    #[test]
    fn test_witness_construction() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let pre = matrix.initial_marking(&net);
        let post = fire_transition(&matrix, &pre, 0).unwrap();

        let witness = TransitionWitness {
            pre_marking: pre.clone(),
            transition_id: 0,
            post_marking: post.clone(),
        };

        assert_eq!(witness.pre_marking, vec![1, 0, 999]);
        assert_eq!(witness.post_marking, vec![2, 0, 998]);
        assert_eq!(witness.transition_id, 0);
    }
}
