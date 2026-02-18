//! Error types for tokenmodel operations.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    // Schema validation errors
    #[error("tokenmodel: element has empty ID")]
    EmptyId,

    #[error("tokenmodel: duplicate element ID: {0}")]
    DuplicateId(String),

    #[error("tokenmodel: arc source not found: {0}")]
    InvalidArcSource(String),

    #[error("tokenmodel: arc target not found: {0}")]
    InvalidArcTarget(String),

    #[error("tokenmodel: arcs must connect states to actions")]
    InvalidArcConnection,

    // Execution errors
    #[error("tokenmodel: action not found: {0}")]
    ActionNotFound(String),

    #[error("tokenmodel: insufficient tokens to execute")]
    InsufficientTokens,

    #[error("tokenmodel: action guard not satisfied")]
    GuardNotSatisfied,

    #[error("tokenmodel: guard evaluation error: {0}")]
    GuardEvaluation(String),

    #[error("tokenmodel: action not enabled: {0}")]
    ActionNotEnabled(String),

    // Constraint errors
    #[error("tokenmodel: constraint violated: {0}")]
    ConstraintViolated(String),

    #[error("tokenmodel: constraint evaluation error: {0}: {1}")]
    ConstraintEvaluation(String, String),
}

pub type Result<T> = std::result::Result<T, Error>;
