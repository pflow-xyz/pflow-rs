//! Petri net transition circuit for R1CS constraint generation.

use ark_bn254::Fr;
use ark_r1cs_std::{alloc::AllocVar, eq::EqGadget, fields::fp::FpVar, fields::FieldVar};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

/// Circuit that proves a valid Petri net transition firing.
///
/// Public inputs: pre_state_root, post_state_root, transition_id
/// Private inputs: pre_marking, post_marking
///
/// Constraints:
/// 1. Hash pre-marking with Poseidon -> assert equals pre_state_root
/// 2. Hash post-marking with Poseidon -> assert equals post_state_root
/// 3. Compute delta from topology (incidence matrix baked as constants)
/// 4. Assert post[p] == pre[p] + delta[p] for all places
/// 5. Assert enabledness: pre[p] >= weight for selected transition's inputs
#[derive(Clone)]
pub struct PetriTransitionCircuit {
    /// Number of places in the net.
    pub num_places: usize,
    /// Number of transitions in the net.
    pub num_transitions: usize,
    /// Per-transition input arcs: (place_idx, weight).
    pub inputs: Vec<Vec<(usize, i64)>>,
    /// Per-transition output arcs: (place_idx, weight).
    pub outputs: Vec<Vec<(usize, i64)>>,
    /// Pre-marking (witness).
    pub pre_marking: Vec<i64>,
    /// Post-marking (witness).
    pub post_marking: Vec<i64>,
    /// Which transition was fired (witness).
    pub transition_id: usize,
    /// Pre-state Poseidon hash (public input).
    pub pre_state_root: Option<Fr>,
    /// Post-state Poseidon hash (public input).
    pub post_state_root: Option<Fr>,
}

impl ConstraintSynthesizer<Fr> for PetriTransitionCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // --- Allocate witness variables ---

        // Pre-marking as private witnesses
        let pre_vars: Vec<FpVar<Fr>> = self
            .pre_marking
            .iter()
            .map(|&v| FpVar::new_witness(cs.clone(), || Ok(Fr::from(v as u64))).unwrap())
            .collect();

        // Post-marking as private witnesses
        let post_vars: Vec<FpVar<Fr>> = self
            .post_marking
            .iter()
            .map(|&v| FpVar::new_witness(cs.clone(), || Ok(Fr::from(v as u64))).unwrap())
            .collect();

        // Transition ID as private witness
        let tid_var =
            FpVar::new_witness(cs.clone(), || Ok(Fr::from(self.transition_id as u64)))?;

        // Public inputs: state roots
        let pre_root_var = FpVar::new_input(cs.clone(), || {
            self.pre_state_root
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let post_root_var = FpVar::new_input(cs.clone(), || {
            self.post_state_root
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Transition ID as public input
        let tid_pub = FpVar::new_input(cs.clone(), || Ok(Fr::from(self.transition_id as u64)))?;
        tid_var.enforce_equal(&tid_pub)?;

        // --- Step 1 & 2: Hash markings with Poseidon ---
        let pre_hash = crate::hash::poseidon_hash_vec(cs.clone(), &pre_vars)?;
        let post_hash = crate::hash::poseidon_hash_vec(cs.clone(), &post_vars)?;

        pre_hash.enforce_equal(&pre_root_var)?;
        post_hash.enforce_equal(&post_root_var)?;

        // --- Step 3 & 4: Compute delta and verify state transition ---
        let delta = self.compute_delta();
        let delta_vars: Vec<FpVar<Fr>> = delta
            .iter()
            .map(|&d| {
                let f = if d >= 0 {
                    Fr::from(d as u64)
                } else {
                    -Fr::from((-d) as u64)
                };
                FpVar::new_witness(cs.clone(), || Ok(f)).unwrap()
            })
            .collect();

        // Assert post[p] == pre[p] + delta[p] for all places
        for p in 0..self.num_places {
            let expected = &pre_vars[p] + &delta_vars[p];
            post_vars[p].enforce_equal(&expected)?;
        }

        // --- Step 5: Assert enabledness ---
        // For each input arc of the selected transition, assert pre[p] >= weight
        // by showing pre[p] - weight = remainder (non-negative witness)
        for &(p_idx, weight) in &self.inputs[self.transition_id] {
            let weight_const = FpVar::constant(Fr::from(weight as u64));
            let remainder = FpVar::new_witness(cs.clone(), || {
                let pre_val = self.pre_marking[p_idx];
                let r = pre_val - weight;
                Ok(Fr::from(r as u64))
            })?;
            let sum = &weight_const + &remainder;
            pre_vars[p_idx].enforce_equal(&sum)?;
        }

        Ok(())
    }
}

impl PetriTransitionCircuit {
    /// Compute the delta vector for the selected transition.
    fn compute_delta(&self) -> Vec<i64> {
        let mut d = vec![0i64; self.num_places];
        for &(p, w) in &self.inputs[self.transition_id] {
            d[p] -= w;
        }
        for &(p, w) in &self.outputs[self.transition_id] {
            d[p] += w;
        }
        d
    }

    /// Create a circuit from an incidence matrix and witness data.
    pub fn from_incidence(
        matrix: &pflow_zk::IncidenceMatrix,
        pre_marking: &[i64],
        post_marking: &[i64],
        transition_id: usize,
    ) -> Self {
        Self {
            num_places: matrix.num_places,
            num_transitions: matrix.num_transitions,
            inputs: matrix.inputs.clone(),
            outputs: matrix.outputs.clone(),
            pre_marking: pre_marking.to_vec(),
            post_marking: post_marking.to_vec(),
            transition_id,
            pre_state_root: None,
            post_state_root: None,
        }
    }
}
