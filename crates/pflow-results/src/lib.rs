//! Structured simulation results, parameter sensitivity analysis, and
//! what-if hypothesis evaluation — the "data half" of ROADMAP.md Phase 4,
//! ported from go-pflow's `results`, `sensitivity` and `hypothesis`
//! packages.
//!
//! These three packages have no go-pflow-produced golden fixture (no
//! `cmd/*-goldens` analogue emits one, the same interim state Phase 3
//! documents for `templates`/`derive`): go-pflow's own tests are unit
//! tests, not byte-identical JSON, so this port is held to the equivalent
//! Rust unit tests instead — one per behavior go-pflow's own
//! `results_test.go`-equivalents assert (there is no `results_test.go`;
//! `sensitivity_test.go` and `hypothesis_test.go` are ported case for case
//! into `sensitivity` and `hypothesis`'s own `#[cfg(test)]` modules).
//! `pflow-xyz/examples/showcase/fixtures/{scenarios,observations,rates}.json`
//! feed `petri_scenario`/`petri_fit` in petri-pilot, not this crate
//! directly — `hypothesis::Evaluator` is go-pflow's library-level analogue
//! of the what-if shape those tools return, not a byte-for-byte consumer of
//! those fixtures.

pub mod hypothesis;
pub mod results;
pub mod sensitivity;
