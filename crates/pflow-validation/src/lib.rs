//! Structural validation for `pflow-metamodel` nets, ported from go-pflow's
//! `validation` package (ROADMAP.md Phase 2).
//!
//! Checks structure (empty net, negative markings, capacity vs. initial
//! marking, non-positive arc weights), connectivity, obviously-blocked
//! transitions, unboundedness (via [`pflow_reachability`]'s P-invariant
//! cover and coverability witness) and conservation.
//!
//! # The `+Inf` capacity clamp
//!
//! go-pflow's Shape A (`petri.PetriNet`) gives a place a *vector* capacity,
//! one component per color, where a zero component means that color is
//! unbounded — and one unbounded color makes the whole place unbounded
//! (`visualization/render.go`'s `getCapacity`). `reachability/graph.go`'s
//! own enablement check got this wrong: it *summed* the capacity vector and
//! only treated the place as unbounded when the sum was `<= 0`, so a place
//! like `[red:1, blue:0]` (blue unbounded) was clamped to a bogus total
//! capacity of 1 instead of being treated as unbounded — the bug
//! ROADMAP.md flags as having bitten `petri_analyze`.
//!
//! `pflow-metamodel`'s `Place::capacity` is already a single post-firing
//! `i64` (colors are unfolded upstream, in `pflow-core`/`pflow-parser`), so
//! there is no vector to sum incorrectly: `capacity <= 0` means unbounded,
//! full stop, and every check in this crate and in
//! `pflow_metamodel::firing` agrees on that reading. The historical bug has
//! no analogue to reintroduce here — noted so a future per-color capacity
//! feature on `Model` does not.
//!
//! # Non-positive arc weights
//!
//! go-pflow's Shape A arc weight is a float vector that can hold an
//! explicit `0`, distinct from an absent weight (which defaults to 1), so
//! `checkStructure` can flag "weight is exactly zero" as a mistake. Shape
//! B's `Arc::weight` is a plain `i64` with `0` reused to mean "unset"
//! (`Arc::effective_weight`), so an explicit zero is not representable
//! here; this crate's structure check instead flags
//! `effective_weight() <= 0`, which still catches a negative weight (a
//! typo `Model` JSON can produce) but cannot catch "zero on purpose",
//! because Shape B cannot express that arc in the first place.

pub mod checks;
pub mod types;

pub use types::{Issue, Severity, Summary, ValidationResult, Validator};
