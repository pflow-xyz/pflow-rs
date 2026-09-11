//! Discrete-stochastic simulation of a [`pflow_metamodel::Model`]: stages,
//! rate schedules with per-realization marking carry across boundaries,
//! guard-aware propensities, supply classification, and the reporting layer
//! (`Caveats` vs `Assumptions`, time-weighted `Metrics`, `Contended`,
//! `Depleted`). Ported from go-pflow's `stochastic` package
//! (`stochastic.go`, `schedule.go`, `guard.go`, `supply.go`).
//!
//! This sits alongside, not on top of, [`crate::ssa`]: that module is the
//! byte-exact portable Gillespie SSA held to cross-language goldens and
//! consumed by pflow-xyz/pflow-jl parity tests; this module is the richer
//! engine — stages, schedules, contention/metrics reporting, supply
//! classification, guard evaluation — operating directly on
//! [`pflow_metamodel::Model`] rather than the portable SSA's own ordered
//! [`crate::ssa::SsaModel`]. It reuses [`crate::ssa::combinations`],
//! [`crate::ssa::Xoshiro256`] and [`crate::ssa::plog`] for its arithmetic
//! and randomness, and — since `engine::ssa`'s waiting-time draw was fixed
//! to use the same portable, no-clamp `-plog(1.0 - u)` shape [`crate::ssa`]
//! already used — **does** carry a byte-parity contract of its own now: see
//! `tests/scheduled_parity.rs`'s `scheduled_run_matches_go_pflow_exactly`.
//!
//! `Forecast` (the continuous mass-action ODE dispatch) is not part of this
//! port; [`crate::ode`] already runs the ODE side directly. See
//! `ROADMAP.md` Phase 1 for what remains out of scope.

mod compile;
mod engine;
mod options;
mod result;
mod simulate;
mod supply;

pub use compile::{token_places, StochasticError};
pub use options::{rates, start_from, Options, DEFAULT_RATE};
pub use result::{
    Contention, Depletion, Metrics, Series, SimResult, EXPONENTIAL_SERVICE_ASSUMPTION,
};
pub use simulate::{simulate, simulate_schedule};
pub use supply::{classify_supply, SupplyKind};
