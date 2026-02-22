//! risc0 zkVM-based prover for Petri nets.
//!
//! Wraps the Petri net transition logic in a RISC-V guest program
//! and generates STARK proofs via risc0.

pub mod io;
mod prover;

pub use prover::Risc0Prover;
