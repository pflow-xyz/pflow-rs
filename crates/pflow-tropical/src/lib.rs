//! Tropical semiring (max-plus algebra) for Petri net analysis.
//!
//! Implements:
//! - Max-plus scalar and matrix arithmetic
//! - Tropical eigenvalue (max circuit mean) via Karp's algorithm
//! - Dense weight matrix factoring into sparse tropical structure
//! - Extraction to pflow incidence matrix format

mod semiring;
mod eigenvalue;
mod factoring;
mod extract;
mod invariants;
pub mod relu_net;
pub mod ttt_fixtures;
pub mod ttt_game;

pub use semiring::{Matrix, mat_mul, mat_pow, tropical_add, tropical_mul, NEG_INF};
pub use eigenvalue::eigenvalue;
pub use factoring::{Factor, FactorConfig};
pub use extract::{extract, PflowNet};
pub use invariants::{dense_incidence, p_invariants, t_invariants, support, sign_pattern};
