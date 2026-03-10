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

pub use semiring::{Matrix, mat_mul, mat_pow, tropical_add, tropical_mul, NEG_INF};
pub use eigenvalue::eigenvalue;
pub use factoring::{Factor, FactorConfig};
pub use extract::{extract, PflowNet};
