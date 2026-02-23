//! ArkworksProver: Groth16 prover over BN254 for Petri net transitions.

use std::time::Instant;

use ark_bn254::{Bn254, Fr};
use ark_groth16::{Groth16, PreparedVerifyingKey, Proof as Groth16Proof, ProvingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use ark_std::rand::{rngs::StdRng, SeedableRng};

use pflow_zk::{
    IncidenceMatrix, PetriProver, Proof, ProofMetrics, TransitionWitness, ZkError,
};

use crate::circuit::PetriTransitionCircuit;
use crate::hash::poseidon_hash_native;

/// Arkworks-based Groth16 prover for Petri net transitions.
pub struct ArkworksProver {
    matrix: IncidenceMatrix,
    proving_key: Option<ProvingKey<Bn254>>,
    verifying_key: Option<PreparedVerifyingKey<Bn254>>,
}

impl ArkworksProver {
    /// Create a new prover for the given Petri net topology.
    pub fn new(matrix: IncidenceMatrix) -> Self {
        Self {
            matrix,
            proving_key: None,
            verifying_key: None,
        }
    }

    /// Build a circuit for the given witness.
    fn build_circuit(
        &self,
        witness: &TransitionWitness,
    ) -> Result<PetriTransitionCircuit, pflow_zk::ZkError> {
        if witness.pre_marking.len() != self.matrix.num_places {
            return Err(ZkError::MarkingMismatch {
                expected: self.matrix.num_places,
                got: witness.pre_marking.len(),
            });
        }

        let pre_fr: Vec<Fr> = witness
            .pre_marking
            .iter()
            .map(|&v| Fr::from(v as u64))
            .collect();
        let post_fr: Vec<Fr> = witness
            .post_marking
            .iter()
            .map(|&v| Fr::from(v as u64))
            .collect();

        let pre_root = poseidon_hash_native(&pre_fr);
        let post_root = poseidon_hash_native(&post_fr);

        let mut circuit = PetriTransitionCircuit::from_incidence(
            &self.matrix,
            &witness.pre_marking,
            &witness.post_marking,
            witness.transition_id,
        );
        circuit.pre_state_root = Some(pre_root);
        circuit.post_state_root = Some(post_root);

        Ok(circuit)
    }

    /// Export the verifying key as a deployable Solidity contract.
    ///
    /// Must call `setup()` first. Returns the Solidity source code for
    /// a Groth16 verifier with the VK baked in as constants.
    pub fn export_solidity_verifier(&self) -> Result<String, ZkError> {
        let pk = self.proving_key.as_ref().ok_or(ZkError::NotSetup)?;
        let vk = &pk.vk;
        let sol_vk = crate::solidity_export::extract_solidity_vk(vk);
        Ok(crate::solidity_export::render_groth16_verifier(&sol_vk))
    }

    /// Export proof bytes as Solidity calldata hex values.
    pub fn proof_to_calldata(
        proof_bytes: &[u8],
    ) -> Result<crate::solidity_export::SolidityProof, ZkError> {
        crate::solidity_export::proof_to_solidity_calldata(proof_bytes)
    }

    /// Create a dummy circuit for setup (uses zero markings).
    fn dummy_circuit(&self) -> PetriTransitionCircuit {
        let zero_marking = vec![0i64; self.matrix.num_places];
        let mut circuit = PetriTransitionCircuit::from_incidence(
            &self.matrix,
            &zero_marking,
            &zero_marking,
            0,
        );
        circuit.pre_state_root = Some(Fr::from(0u64));
        circuit.post_state_root = Some(Fr::from(0u64));
        circuit
    }
}

impl PetriProver for ArkworksProver {
    fn setup(&mut self) -> pflow_zk::Result<()> {
        let circuit = self.dummy_circuit();
        let mut rng = StdRng::seed_from_u64(42);

        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit, &mut rng)
            .map_err(|e| ZkError::ProofGeneration(format!("setup failed: {}", e)))?;

        let pvk = Groth16::<Bn254>::process_vk(&vk)
            .map_err(|e| ZkError::ProofGeneration(format!("vk processing failed: {}", e)))?;

        self.proving_key = Some(pk);
        self.verifying_key = Some(pvk);
        Ok(())
    }

    fn prove(&self, witness: &TransitionWitness) -> pflow_zk::Result<Proof> {
        let pk = self.proving_key.as_ref().ok_or(ZkError::NotSetup)?;

        let start = Instant::now();
        let circuit = self.build_circuit(witness)?;

        // Collect public inputs: pre_root, post_root, transition_id
        let pre_fr: Vec<Fr> = witness
            .pre_marking
            .iter()
            .map(|&v| Fr::from(v as u64))
            .collect();
        let post_fr: Vec<Fr> = witness
            .post_marking
            .iter()
            .map(|&v| Fr::from(v as u64))
            .collect();
        let pre_root = poseidon_hash_native(&pre_fr);
        let post_root = poseidon_hash_native(&post_fr);
        let tid_fr = Fr::from(witness.transition_id as u64);
        let public_inputs = vec![pre_root, post_root, tid_fr];

        let mut rng = StdRng::seed_from_u64(42);
        let proof = Groth16::<Bn254>::prove(pk, circuit, &mut rng)
            .map_err(|e| ZkError::ProofGeneration(format!("prove failed: {}", e)))?;

        let generation_time = start.elapsed().as_millis() as u64;

        // Serialize proof
        let mut proof_bytes = Vec::new();
        proof
            .serialize_compressed(&mut proof_bytes)
            .map_err(|e: ark_serialize::SerializationError| {
                ZkError::Serialization(e.to_string())
            })?;

        // Serialize public inputs
        let mut pi_bytes = Vec::new();
        for pi in &public_inputs {
            pi.serialize_compressed(&mut pi_bytes)
                .map_err(|e: ark_serialize::SerializationError| {
                    ZkError::Serialization(e.to_string())
                })?;
        }

        Ok(Proof {
            proof_bytes: proof_bytes.clone(),
            public_inputs: pi_bytes,
            system: self.system_name().to_string(),
            metrics: ProofMetrics {
                generation_time_ms: generation_time,
                proof_size_bytes: proof_bytes.len(),
                verification_time_ms: None,
                constraint_count: None,
            },
        })
    }

    fn verify(&self, proof: &Proof) -> pflow_zk::Result<bool> {
        let pvk = self.verifying_key.as_ref().ok_or(ZkError::NotSetup)?;

        let start = Instant::now();

        let groth16_proof =
            Groth16Proof::<Bn254>::deserialize_compressed(&proof.proof_bytes[..]).map_err(
                |e| ZkError::Verification(format!("proof deserialization failed: {}", e)),
            )?;

        // Deserialize public inputs
        let mut reader = &proof.public_inputs[..];
        let mut public_inputs = Vec::new();
        while !reader.is_empty() {
            let pi = Fr::deserialize_compressed(&mut reader).map_err(|e| {
                ZkError::Verification(format!("public input deserialization failed: {}", e))
            })?;
            public_inputs.push(pi);
        }

        let result =
            Groth16::<Bn254>::verify_with_processed_vk(pvk, &public_inputs, &groth16_proof)
                .map_err(|e| ZkError::Verification(format!("verification failed: {}", e)))?;

        let _verify_time = start.elapsed().as_millis() as u64;

        Ok(result)
    }

    fn verifying_key(&self) -> pflow_zk::Result<Vec<u8>> {
        let pvk = self.verifying_key.as_ref().ok_or(ZkError::NotSetup)?;

        let mut bytes = Vec::new();
        pvk.serialize_compressed(&mut bytes)
            .map_err(|e: ark_serialize::SerializationError| {
                ZkError::Serialization(e.to_string())
            })?;
        Ok(bytes)
    }

    fn system_name(&self) -> &'static str {
        "groth16-bn254"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::PetriNet;
    use pflow_zk::{fire_transition, IncidenceMatrix};

    #[test]
    fn test_arkworks_setup() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let mut prover = ArkworksProver::new(matrix);
        prover.setup().unwrap();
        assert!(prover.proving_key.is_some());
        assert!(prover.verifying_key.is_some());
    }

    #[test]
    fn test_arkworks_prove_verify_sir() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let mut prover = ArkworksProver::new(matrix.clone());
        prover.setup().unwrap();

        let pre = matrix.initial_marking(&net);
        let post = fire_transition(&matrix, &pre, 0).unwrap();

        let witness = TransitionWitness {
            pre_marking: pre,
            transition_id: 0,
            post_marking: post,
        };

        let proof = prover.prove(&witness).unwrap();
        assert!(
            proof.metrics.proof_size_bytes < 300,
            "Groth16 proofs should be small"
        );

        let valid = prover.verify(&proof).unwrap();
        assert!(valid, "Valid proof should verify");
    }

    #[test]
    fn test_arkworks_verifying_key() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let mut prover = ArkworksProver::new(matrix);
        prover.setup().unwrap();

        let vk = prover.verifying_key().unwrap();
        assert!(!vk.is_empty());
    }

    #[test]
    fn test_arkworks_all_transitions_single_key() {
        // SIR: infect has 2 input arcs, recover has 1.
        // Both must work with a single proving key (padded constraints).
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);
        let mut prover = ArkworksProver::new(matrix.clone());
        prover.setup().unwrap();

        let m0 = matrix.initial_marking(&net);

        // Prove infect (transition 0, 2 input arcs)
        let m1 = fire_transition(&matrix, &m0, 0).unwrap();
        let proof_infect = prover
            .prove(&TransitionWitness {
                pre_marking: m0.clone(),
                transition_id: 0,
                post_marking: m1.clone(),
            })
            .unwrap();
        assert!(prover.verify(&proof_infect).unwrap(), "infect proof should verify");

        // Prove recover (transition 1, 1 input arc)
        let m2 = fire_transition(&matrix, &m1, 1).unwrap();
        let proof_recover = prover
            .prove(&TransitionWitness {
                pre_marking: m1,
                transition_id: 1,
                post_marking: m2,
            })
            .unwrap();
        assert!(prover.verify(&proof_recover).unwrap(), "recover proof should verify");
    }
}
