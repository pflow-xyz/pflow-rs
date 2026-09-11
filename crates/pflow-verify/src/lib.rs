//! Turns questions about a `pflow-metamodel` net into verdicts with
//! evidence: state a property, get back proved, refuted, or unknown, with a
//! replayable firing sequence when refuted and a named structural proof
//! when proved. Ported from go-pflow's `verify` package (ROADMAP.md
//! Phase 2).

pub mod property;
pub mod verify;

pub use property::{
    parse_expr, Counterexample, Kind, LinearExpr, Method, ParseError, Property, Relation, Report,
    Status, Verdict,
};
pub use verify::{Verifier, DEFAULT_MAX_STATES};
