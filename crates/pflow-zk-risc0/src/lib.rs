//! risc0 zkVM-based prover for Petri nets.
//!
//! Wraps the Petri net transition logic in a RISC-V guest program
//! and generates STARK proofs via risc0.
//!
//! # Feature flags
//!
//! - **`prove`** — enables real STARK proof generation via risc0 zkVM.
//!   Requires the risc0 toolchain (`cargo risczero install`).
//!   Without this feature, the prover runs in simulation mode.

pub mod io;
mod prover;

// When the `prove` feature is enabled, risc0-build compiles the guest
// program and generates ELF + image ID constants.
#[cfg(feature = "prove")]
include!(concat!(env!("OUT_DIR"), "/methods.rs"));

pub use prover::Risc0Prover;
