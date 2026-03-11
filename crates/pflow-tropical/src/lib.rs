//! Tropical semiring (max-plus algebra) for Petri net analysis.
//!
//! Implements:
//! - Max-plus scalar and matrix arithmetic
//! - Tropical eigenvalue (max circuit mean) via Karp's algorithm
//! - Dense weight matrix factoring into sparse tropical structure
//! - Generic `NetMatrix` type with firing semantics (no pflow dependency)
//! - Extraction to pflow incidence matrix format (behind `pflow` feature)

mod semiring;
mod eigenvalue;
mod factoring;
mod extract;
mod invariants;
pub mod net_matrix;
pub mod relu_net;
pub mod rnn;
pub mod ttt_fixtures;


pub use semiring::{Matrix, mat_mul, mat_pow, tropical_add, tropical_mul, NEG_INF};
pub use eigenvalue::eigenvalue;
pub use factoring::{Factor, FactorConfig};
pub use net_matrix::NetMatrix;
pub use extract::extract;
pub use invariants::{p_invariants_from_dense, t_invariants_from_dense, support, sign_pattern};

#[cfg(feature = "pflow")]
pub use extract::to_incidence_matrix;
#[cfg(feature = "pflow")]
pub use invariants::{dense_incidence, p_invariants, t_invariants};
