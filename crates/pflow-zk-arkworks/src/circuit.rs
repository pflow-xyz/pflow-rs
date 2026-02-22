//! Petri net transition circuit for R1CS constraint generation.
//!
//! The circuit uses a transition multiplexer: all transitions' topology is
//! baked in as constants, and boolean selectors gate which one is active.
//! This produces a fixed R1CS structure regardless of which transition fires,
//! allowing a single Groth16 proving key for the entire net.

use ark_bn254::Fr;
use ark_r1cs_std::{alloc::AllocVar, eq::EqGadget, fields::fp::FpVar, fields::FieldVar};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

/// Circuit that proves a valid Petri net transition firing.
///
/// Public inputs: pre_state_root, post_state_root, transition_id
/// Private inputs: pre_marking, post_marking, selectors, delta, remainders
///
/// Constraints:
/// 1. Hash pre-marking with Poseidon -> assert equals pre_state_root
/// 2. Hash post-marking with Poseidon -> assert equals post_state_root
/// 3. Selector multiplexer: exactly one boolean selector matches transition_id
/// 4. Multiplexed delta: delta[p] = sum_t(selector_t * topology_delta_t[p])
/// 5. State update: post[p] == pre[p] + delta[p] for all places
/// 6. Enabledness: selector_t * (pre[p] - weight - remainder) == 0 for all arcs
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
        // --- Allocate marking witnesses ---
        let pre_vars: Vec<FpVar<Fr>> = self
            .pre_marking
            .iter()
            .map(|&v| FpVar::new_witness(cs.clone(), || Ok(Fr::from(v as u64))).unwrap())
            .collect();

        let post_vars: Vec<FpVar<Fr>> = self
            .post_marking
            .iter()
            .map(|&v| FpVar::new_witness(cs.clone(), || Ok(Fr::from(v as u64))).unwrap())
            .collect();

        // Transition ID as private witness
        let tid_var =
            FpVar::new_witness(cs.clone(), || Ok(Fr::from(self.transition_id as u64)))?;

        // --- Public inputs ---
        let pre_root_var = FpVar::new_input(cs.clone(), || {
            self.pre_state_root
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let post_root_var = FpVar::new_input(cs.clone(), || {
            self.post_state_root
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let tid_pub = FpVar::new_input(cs.clone(), || Ok(Fr::from(self.transition_id as u64)))?;
        tid_var.enforce_equal(&tid_pub)?;

        // --- Step 1 & 2: Poseidon hash commitments ---
        let pre_hash = crate::hash::poseidon_hash_vec(cs.clone(), &pre_vars)?;
        let post_hash = crate::hash::poseidon_hash_vec(cs.clone(), &post_vars)?;
        pre_hash.enforce_equal(&pre_root_var)?;
        post_hash.enforce_equal(&post_root_var)?;

        // --- Step 3: Transition selector multiplexer ---
        // Allocate one boolean selector per transition: selector[t] = (t == transition_id) ? 1 : 0
        let zero = FpVar::constant(Fr::from(0u64));
        let one = FpVar::constant(Fr::from(1u64));

        let mut selectors = Vec::with_capacity(self.num_transitions);
        for t in 0..self.num_transitions {
            let val = if t == self.transition_id { 1u64 } else { 0u64 };
            let sel = FpVar::new_witness(cs.clone(), || Ok(Fr::from(val)))?;
            // Boolean constraint: sel * (sel - 1) == 0
            let sel_minus_one = &sel - &one;
            (&sel * &sel_minus_one).enforce_equal(&zero)?;
            selectors.push(sel);
        }

        // Exactly one selector is active
        let sel_sum = selectors
            .iter()
            .fold(zero.clone(), |acc, s| acc + s);
        sel_sum.enforce_equal(&one)?;

        // The active selector matches the transition ID
        let weighted_sum = selectors.iter().enumerate().fold(zero.clone(), |acc, (t, s)| {
            acc + s * FpVar::constant(Fr::from(t as u64))
        });
        weighted_sum.enforce_equal(&tid_var)?;

        // --- Step 4: Multiplexed delta from topology constants ---
        // delta[p] = sum_t(selector[t] * topology_delta[t][p])
        // All topology_delta values are CONSTANTS, so the R1CS structure is fixed.
        let mut delta_vars = vec![zero.clone(); self.num_places];
        for t in 0..self.num_transitions {
            let d = self.delta_for(t);
            for p in 0..self.num_places {
                if d[p] != 0 {
                    let d_const = if d[p] >= 0 {
                        FpVar::constant(Fr::from(d[p] as u64))
                    } else {
                        FpVar::constant(-Fr::from((-d[p]) as u64))
                    };
                    delta_vars[p] = &delta_vars[p] + &(&selectors[t] * &d_const);
                }
            }
        }

        // --- Step 5: State update ---
        for p in 0..self.num_places {
            let expected = &pre_vars[p] + &delta_vars[p];
            post_vars[p].enforce_equal(&expected)?;
        }

        // --- Step 6: Enabledness (gated by selectors) ---
        // For each transition t, for each input arc slot (padded to max):
        //   selector[t] * (pre[p] - weight - remainder) == 0
        // When selector=0: trivially 0==0
        // When selector=1: enforces pre[p] == weight + remainder
        let max_input_arcs = self.inputs.iter().map(|v| v.len()).max().unwrap_or(0);
        for t in 0..self.num_transitions {
            let arcs = &self.inputs[t];
            for slot in 0..max_input_arcs {
                let (p_idx, weight) = if slot < arcs.len() {
                    arcs[slot]
                } else {
                    (0, 0) // dummy slot
                };
                let weight_const = FpVar::constant(Fr::from(weight as u64));
                let remainder = FpVar::new_witness(cs.clone(), || {
                    let pre_val = self.pre_marking[p_idx];
                    let r = pre_val - weight;
                    Ok(Fr::from(r.max(0) as u64))
                })?;
                // selector[t] * (pre[p] - weight - remainder) == 0
                let diff = &pre_vars[p_idx] - &weight_const - &remainder;
                let gated = &selectors[t] * &diff;
                gated.enforce_equal(&zero)?;
            }
        }

        Ok(())
    }
}

impl PetriTransitionCircuit {
    /// Compute the delta vector for a specific transition.
    fn delta_for(&self, t: usize) -> Vec<i64> {
        let mut d = vec![0i64; self.num_places];
        for &(p, w) in &self.inputs[t] {
            d[p] -= w;
        }
        for &(p, w) in &self.outputs[t] {
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
