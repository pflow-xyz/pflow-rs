//! Structured simulation output, ported from go-pflow's `results` package
//! (`types.go`, `analysis.go`, `builder.go`, `io.go`). `sweep.go` was
//! ported once and removed 2026-10-21 for want of a consumer (see
//! ROADMAP.md, *Not ported, on purpose*).

pub mod analysis;
pub mod builder;
pub mod io;
pub mod types;

pub use analysis::Analyzer;
pub use builder::{downsample, downsample_aligned, Builder};
pub use io::{from_json, read_json, to_json, write_json};
pub use types::{
    Analysis, Conservation, Crossing, Data, Event, Invariant, Metadata, Model, Peak, Results,
    SeriesData, Simulation, SolverOptions, Stat, SteadyState, Summary, TimeData, Timeseries,
    TokenBalance, SCHEMA_VERSION,
};
