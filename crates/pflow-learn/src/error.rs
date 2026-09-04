//! Error type for `pflow-learn`.

use std::fmt;

/// Errors raised while building or solving a [`crate::problem::LearnableProblem`].
#[derive(Debug, Clone, PartialEq)]
pub enum LearnError {
    /// A net place has no corresponding entry in the initial state map. Silently
    /// defaulting to 0.0 here would drop an arc rather than clamp it off — two
    /// different dynamical systems — so this is a hard error instead.
    PlaceMissingFromU0(String),
    /// The problem has zero learnable parameters across all installed rate functions.
    ZeroParams,
    /// A [`crate::dataset::Dataset`] was built with an empty `times` vector.
    EmptyTimes,
    /// A [`crate::dataset::Dataset`] place's observation vector doesn't match `times`'s
    /// length.
    DatasetLengthMismatch {
        place: String,
        expected: usize,
        found: usize,
    },
    /// An adjoint backward segment did not reach its target time within `maxiters`. The
    /// gradient it would report is silently wrong, so it is never returned.
    AdjointTruncated,
}

impl fmt::Display for LearnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LearnError::PlaceMissingFromU0(place) => {
                write!(
                    f,
                    "place '{place}' referenced by the net is missing from u0"
                )
            }
            LearnError::ZeroParams => write!(f, "problem has zero learnable parameters"),
            LearnError::EmptyTimes => write!(f, "dataset has an empty times vector"),
            LearnError::DatasetLengthMismatch {
                place,
                expected,
                found,
            } => write!(
                f,
                "dataset place '{place}' has {found} observations, expected {expected} (len(times))"
            ),
            LearnError::AdjointTruncated => write!(
                f,
                "adjoint backward solve truncated: raise Maxiters or loosen tolerances"
            ),
        }
    }
}

impl std::error::Error for LearnError {}
