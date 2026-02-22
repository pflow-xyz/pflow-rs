//! Shared I/O types for risc0 guest/host communication.

use serde::{Deserialize, Serialize};

/// Input sent to the risc0 guest program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestInput {
    /// Per-transition input arcs: `inputs[t]` = `[(place_idx, weight), ...]`.
    pub inputs: Vec<Vec<(usize, i64)>>,
    /// Per-transition output arcs: `outputs[t]` = `[(place_idx, weight), ...]`.
    pub outputs: Vec<Vec<(usize, i64)>>,
    /// Number of places in the net.
    pub num_places: usize,
    /// Pre-marking (token counts before firing).
    pub pre_marking: Vec<i64>,
    /// Which transition to fire.
    pub transition_id: usize,
}

/// Output committed by the risc0 guest program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestOutput {
    /// SHA-256 hash of the pre-marking.
    pub pre_hash: [u8; 32],
    /// SHA-256 hash of the post-marking.
    pub post_hash: [u8; 32],
    /// Which transition was fired.
    pub transition_id: usize,
}
