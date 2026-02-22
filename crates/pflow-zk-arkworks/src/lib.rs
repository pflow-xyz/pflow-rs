//! Arkworks-based ZK prover for Petri nets.
//!
//! Compiles Petri net topology into R1CS constraints and generates
//! Groth16 proofs over BN254. Uses Poseidon hash for state commitments.

pub mod circuit;
pub mod hash;

mod prover;

pub use prover::ArkworksProver;
