use risc0_zkvm::guest::env;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Input from the host.
#[derive(Serialize, Deserialize)]
struct GuestInput {
    inputs: Vec<Vec<(usize, i64)>>,
    outputs: Vec<Vec<(usize, i64)>>,
    num_places: usize,
    pre_marking: Vec<i64>,
    transition_id: usize,
}

/// Output committed to the journal.
#[derive(Serialize, Deserialize)]
struct GuestOutput {
    pre_hash: [u8; 32],
    post_hash: [u8; 32],
    transition_id: usize,
}

fn hash_marking(marking: &[i64]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for &v in marking {
        hasher.update(v.to_le_bytes());
    }
    hasher.finalize().into()
}

fn main() {
    let input: GuestInput = env::read();

    // Verify transition is in range
    assert!(
        input.transition_id < input.inputs.len(),
        "transition out of range"
    );

    // Check enabledness
    for &(p_idx, weight) in &input.inputs[input.transition_id] {
        assert!(
            input.pre_marking[p_idx] >= weight,
            "transition not enabled: place {} has {} tokens, need {}",
            p_idx,
            input.pre_marking[p_idx],
            weight
        );
    }

    // Compute post-marking
    let mut post_marking = input.pre_marking.clone();
    for &(p_idx, weight) in &input.inputs[input.transition_id] {
        post_marking[p_idx] -= weight;
    }
    for &(p_idx, weight) in &input.outputs[input.transition_id] {
        post_marking[p_idx] += weight;
    }

    // Hash markings
    let pre_hash = hash_marking(&input.pre_marking);
    let post_hash = hash_marking(&post_marking);

    // Commit output to journal
    let output = GuestOutput {
        pre_hash,
        post_hash,
        transition_id: input.transition_id,
    };
    env::commit(&output);
}
