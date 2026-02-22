//! Risc0Prover: zkVM-based prover for Petri net transitions.
//!
//! When the `prove` feature is enabled, generates real STARK proofs
//! by executing the guest program in the risc0 zkVM.
//!
//! Without the `prove` feature (default), runs in simulation mode:
//! executes the guest logic natively and produces a simulated proof.

use std::time::Instant;

use sha2::{Digest, Sha256};

use pflow_zk::{
    IncidenceMatrix, PetriProver, Proof, ProofMetrics, TransitionWitness, ZkError,
};

use crate::io::{GuestInput, GuestOutput};

#[cfg(feature = "prove")]
use risc0_zkvm::{default_prover, ExecutorEnv, Receipt};

#[cfg(feature = "prove")]
use crate::{PFLOW_ZK_RISC0_GUEST_ELF, PFLOW_ZK_RISC0_GUEST_ID};

/// risc0 zkVM-based prover for Petri net transitions.
///
/// In simulation mode (default), executes the guest logic natively
/// and produces a simulated proof. Full STARK proof generation
/// requires the `prove` feature and the risc0 toolchain.
pub struct Risc0Prover {
    matrix: IncidenceMatrix,
    setup_done: bool,
}

impl Risc0Prover {
    /// Create a new prover for the given topology.
    pub fn new(matrix: IncidenceMatrix) -> Self {
        Self {
            matrix,
            setup_done: false,
        }
    }

    /// Hash a marking using SHA-256 (matches the guest program).
    fn hash_marking(marking: &[i64]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for &v in marking {
            hasher.update(v.to_le_bytes());
        }
        hasher.finalize().into()
    }

    /// Build the guest input from a witness.
    fn build_guest_input(&self, witness: &TransitionWitness) -> GuestInput {
        GuestInput {
            inputs: self.matrix.inputs.clone(),
            outputs: self.matrix.outputs.clone(),
            num_places: self.matrix.num_places,
            pre_marking: witness.pre_marking.clone(),
            transition_id: witness.transition_id,
        }
    }

    /// Execute the guest logic natively (simulation mode).
    #[cfg(not(feature = "prove"))]
    fn execute_guest(&self, input: &GuestInput) -> Result<(GuestOutput, Vec<i64>), ZkError> {
        if input.transition_id >= input.inputs.len() {
            return Err(ZkError::InvalidTransition(
                input.transition_id,
                input.inputs.len(),
            ));
        }

        for &(p_idx, weight) in &input.inputs[input.transition_id] {
            if input.pre_marking[p_idx] < weight {
                return Err(ZkError::NotEnabled(input.transition_id));
            }
        }

        let mut post_marking = input.pre_marking.clone();
        for &(p_idx, weight) in &input.inputs[input.transition_id] {
            post_marking[p_idx] -= weight;
        }
        for &(p_idx, weight) in &input.outputs[input.transition_id] {
            post_marking[p_idx] += weight;
        }

        let pre_hash = Self::hash_marking(&input.pre_marking);
        let post_hash = Self::hash_marking(&post_marking);

        Ok((
            GuestOutput {
                pre_hash,
                post_hash,
                transition_id: input.transition_id,
            },
            post_marking,
        ))
    }
}

impl PetriProver for Risc0Prover {
    fn setup(&mut self) -> pflow_zk::Result<()> {
        // risc0 doesn't need a trusted setup — just mark as ready
        self.setup_done = true;
        Ok(())
    }

    // ── Simulation mode ────────────────────────────────────────────

    #[cfg(not(feature = "prove"))]
    fn prove(&self, witness: &TransitionWitness) -> pflow_zk::Result<Proof> {
        if !self.setup_done {
            return Err(ZkError::NotSetup);
        }

        let start = Instant::now();

        let input = self.build_guest_input(witness);
        let (output, post_marking) = self.execute_guest(&input)?;

        if post_marking != witness.post_marking {
            return Err(ZkError::ProofGeneration(
                "witness post-marking doesn't match computed post-marking".into(),
            ));
        }

        let generation_time = start.elapsed().as_millis() as u64;

        let proof_bytes =
            bincode::serialize(&output).map_err(|e| ZkError::Serialization(e.to_string()))?;
        let public_inputs =
            bincode::serialize(&output).map_err(|e| ZkError::Serialization(e.to_string()))?;

        Ok(Proof {
            proof_bytes: proof_bytes.clone(),
            public_inputs,
            system: self.system_name().to_string(),
            metrics: ProofMetrics {
                generation_time_ms: generation_time,
                proof_size_bytes: proof_bytes.len(),
                verification_time_ms: None,
                constraint_count: None,
            },
        })
    }

    #[cfg(not(feature = "prove"))]
    fn verify(&self, proof: &Proof) -> pflow_zk::Result<bool> {
        if !self.setup_done {
            return Err(ZkError::NotSetup);
        }

        let _output: GuestOutput = bincode::deserialize(&proof.proof_bytes)
            .map_err(|e| ZkError::Verification(format!("deserialization failed: {}", e)))?;

        Ok(true)
    }

    #[cfg(not(feature = "prove"))]
    fn verifying_key(&self) -> pflow_zk::Result<Vec<u8>> {
        if !self.setup_done {
            return Err(ZkError::NotSetup);
        }
        Ok(b"risc0-simulation-vk".to_vec())
    }

    #[cfg(not(feature = "prove"))]
    fn system_name(&self) -> &'static str {
        "risc0-sim"
    }

    // ── Real STARK proof mode ──────────────────────────────────────

    #[cfg(feature = "prove")]
    fn prove(&self, witness: &TransitionWitness) -> pflow_zk::Result<Proof> {
        if !self.setup_done {
            return Err(ZkError::NotSetup);
        }

        let start = Instant::now();

        let input = self.build_guest_input(witness);

        // Pre-check on host side: better error messages, avoids wasted zkVM execution
        if input.transition_id >= input.inputs.len() {
            return Err(ZkError::InvalidTransition(
                input.transition_id,
                input.inputs.len(),
            ));
        }
        for &(p_idx, weight) in &input.inputs[input.transition_id] {
            if input.pre_marking[p_idx] < weight {
                return Err(ZkError::NotEnabled(input.transition_id));
            }
        }

        // Build executor environment with serialized input
        let env = ExecutorEnv::builder()
            .write(&input)
            .map_err(|e| ZkError::ProofGeneration(e.to_string()))?
            .build()
            .map_err(|e| ZkError::ProofGeneration(e.to_string()))?;

        // Generate STARK proof by executing guest in zkVM
        let prove_info = default_prover()
            .prove(env, PFLOW_ZK_RISC0_GUEST_ELF)
            .map_err(|e| ZkError::ProofGeneration(e.to_string()))?;

        let receipt = prove_info.receipt;

        // Decode guest output from the journal
        let output: GuestOutput = receipt
            .journal
            .decode()
            .map_err(|e| ZkError::ProofGeneration(e.to_string()))?;

        // Verify the post-marking hash matches the witness
        let expected_post_hash = Self::hash_marking(&witness.post_marking);
        if output.post_hash != expected_post_hash {
            return Err(ZkError::ProofGeneration(
                "post-marking hash mismatch".into(),
            ));
        }

        let generation_time = start.elapsed().as_millis() as u64;

        // Serialize the STARK receipt as the proof
        let receipt_bytes =
            bincode::serialize(&receipt).map_err(|e| ZkError::Serialization(e.to_string()))?;
        let public_inputs =
            bincode::serialize(&output).map_err(|e| ZkError::Serialization(e.to_string()))?;

        Ok(Proof {
            proof_bytes: receipt_bytes.clone(),
            public_inputs,
            system: self.system_name().to_string(),
            metrics: ProofMetrics {
                generation_time_ms: generation_time,
                proof_size_bytes: receipt_bytes.len(),
                verification_time_ms: None,
                constraint_count: None,
            },
        })
    }

    #[cfg(feature = "prove")]
    fn verify(&self, proof: &Proof) -> pflow_zk::Result<bool> {
        if !self.setup_done {
            return Err(ZkError::NotSetup);
        }

        let start = Instant::now();

        // Deserialize the STARK receipt
        let receipt: Receipt = bincode::deserialize(&proof.proof_bytes)
            .map_err(|e| ZkError::Verification(format!("deserialization failed: {}", e)))?;

        // Verify the STARK proof against the guest image ID
        receipt
            .verify(PFLOW_ZK_RISC0_GUEST_ID)
            .map_err(|e| ZkError::Verification(e.to_string()))?;

        let _verify_time = start.elapsed().as_millis() as u64;

        Ok(true)
    }

    #[cfg(feature = "prove")]
    fn verifying_key(&self) -> pflow_zk::Result<Vec<u8>> {
        if !self.setup_done {
            return Err(ZkError::NotSetup);
        }
        // The image ID is the verifying key for risc0
        Ok(PFLOW_ZK_RISC0_GUEST_ID
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect())
    }

    #[cfg(feature = "prove")]
    fn system_name(&self) -> &'static str {
        "risc0"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::PetriNet;
    use pflow_zk::{fire_transition, IncidenceMatrix};

    #[test]
    fn test_risc0_setup() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let mut prover = Risc0Prover::new(matrix);
        prover.setup().unwrap();
        assert!(prover.setup_done);
    }

    #[test]
    fn test_risc0_prove_verify_sir() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let mut prover = Risc0Prover::new(matrix.clone());
        prover.setup().unwrap();

        let pre = matrix.initial_marking(&net);
        let post = fire_transition(&matrix, &pre, 0).unwrap();

        let witness = TransitionWitness {
            pre_marking: pre,
            transition_id: 0,
            post_marking: post,
        };

        let proof = prover.prove(&witness).unwrap();
        let valid = prover.verify(&proof).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_risc0_hash_determinism() {
        let marking = vec![1i64, 0, 999];
        let h1 = Risc0Prover::hash_marking(&marking);
        let h2 = Risc0Prover::hash_marking(&marking);
        assert_eq!(h1, h2);

        let different = vec![2i64, 0, 998];
        let h3 = Risc0Prover::hash_marking(&different);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_risc0_fire_sequence() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let mut prover = Risc0Prover::new(matrix.clone());
        prover.setup().unwrap();

        let m0 = matrix.initial_marking(&net);
        let m1 = fire_transition(&matrix, &m0, 0).unwrap();
        let m2 = fire_transition(&matrix, &m1, 1).unwrap();

        let cases: Vec<(&Vec<i64>, usize, &Vec<i64>)> =
            vec![(&m0, 0, &m1), (&m1, 1, &m2)];
        for (pre, tid, post) in cases {
            let witness = TransitionWitness {
                pre_marking: pre.clone(),
                transition_id: tid,
                post_marking: post.clone(),
            };
            let proof = prover.prove(&witness).unwrap();
            assert!(prover.verify(&proof).unwrap());
        }
    }

    #[test]
    fn test_risc0_not_enabled() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let mut prover = Risc0Prover::new(matrix.clone());
        prover.setup().unwrap();

        let witness = TransitionWitness {
            pre_marking: vec![0, 0, 999],
            transition_id: 0,
            post_marking: vec![1, 0, 998],
        };

        assert!(matches!(prover.prove(&witness), Err(ZkError::NotEnabled(0))));
    }
}
