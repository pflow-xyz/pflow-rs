//! Error types for `pflow-metamodel`.
//!
//! One enum, not one type per rule, because every caller that cares
//! (`Enabled`, `Fire`, `ApplyParameters`, `ExpandStages`, ...) wants to
//! either check "did this fail" or show the message to a human — never to
//! match on the specific variant. The messages mirror go-pflow's
//! `fmt.Errorf` text so a caller porting expectations across the two finds
//! the same words.

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum Error {
    #[error("no transition {0:?} in model {1:?}")]
    UnknownTransition(String, String),

    #[error("{transition} needs {weight} token(s) on {place}, have {have}")]
    InsufficientInput {
        transition: String,
        place: String,
        weight: i64,
        have: i64,
    },

    #[error("{transition} is inhibited: {place} holds {have} (>= {weight})")]
    Inhibited {
        transition: String,
        place: String,
        weight: i64,
        have: i64,
    },

    #[error("{transition} reads {weight} token(s) from {place}, have {have}")]
    ReadUnsatisfied {
        transition: String,
        place: String,
        weight: i64,
        have: i64,
    },

    #[error(
        "{transition} would leave {after} token(s) on {place}, over its capacity of {capacity}"
    )]
    CapacityExceeded {
        transition: String,
        place: String,
        after: i64,
        capacity: i64,
    },

    #[error("unknown parameter {0:?}")]
    UnknownParameter(String),

    #[error("parameter {0:?}: {1}")]
    Parameter(String, String),

    #[error("no arc {from} -> {to}")]
    NoArc { from: String, to: String },

    #[error("no place {0:?}")]
    NoPlace(String),

    #[error("transition {0:?} declares {1} stages; negative stages mean nothing")]
    NegativeStages(String, i64),

    #[error("transition {0:?}: {1}")]
    Stage(String, String),

    #[error("{transition} refuses roles {roles:?}: not in {allowed:?}")]
    AccessDenied {
        transition: String,
        roles: Vec<String>,
        allowed: Vec<String>,
    },

    /// go-pflow's own firing rule has no guard against a negative arc
    /// weight either (verified against `metamodel/firing.go`), but letting
    /// one through here means a consuming arc is always "satisfied" (since
    /// `have < negative_weight` is never true) and firing it manufactures
    /// tokens instead of consuming them. This is a deliberate hardening
    /// beyond go-pflow, not a golden mismatch: a model with only
    /// non-negative weights (the only shape a well-formed net declares)
    /// behaves identically on both sides.
    #[error("{transition}: arc to/from {place} has negative weight {weight}")]
    NegativeWeight {
        transition: String,
        place: String,
        weight: i64,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
