//! Core types for ZK proofs over Petri nets.

use serde::{Deserialize, Serialize};

/// Integer marking vector: one entry per place in canonical order.
pub type Marking = Vec<i64>;

/// Index into the canonical transition list.
pub type TransitionId = usize;

/// Raw proof bytes (format depends on proof system).
pub type ProofBytes = Vec<u8>;

/// Serialized verifying key.
pub type VerifyingKeyBytes = Vec<u8>;

/// Witness for a single transition firing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionWitness {
    /// Token counts before firing (canonical place order).
    pub pre_marking: Marking,
    /// Which transition fired.
    pub transition_id: TransitionId,
    /// Token counts after firing (canonical place order).
    pub post_marking: Marking,
}

/// A generated ZK proof with metadata.
#[derive(Debug, Clone)]
pub struct Proof {
    /// The raw proof bytes.
    pub proof_bytes: ProofBytes,
    /// Serialized public inputs.
    pub public_inputs: Vec<u8>,
    /// Name of the proof system that generated this.
    pub system: String,
    /// Performance metrics from proof generation.
    pub metrics: ProofMetrics,
}

/// Performance metrics captured during proof generation/verification.
#[derive(Debug, Clone, Default)]
pub struct ProofMetrics {
    /// Time to generate the proof in milliseconds.
    pub generation_time_ms: u64,
    /// Size of the proof in bytes.
    pub proof_size_bytes: usize,
    /// Time to verify the proof in milliseconds (if measured).
    pub verification_time_ms: Option<u64>,
    /// Number of R1CS constraints (if applicable).
    pub constraint_count: Option<usize>,
}
