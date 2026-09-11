//! Structured simulation output, ported from go-pflow's `results` package
//! (`types.go`, `analysis.go`, `builder.go`, `sweep.go`, `io.go`).

pub mod analysis;
pub mod builder;
pub mod io;
pub mod sweep;
pub mod types;

pub use analysis::Analyzer;
pub use builder::{downsample, downsample_aligned, Builder};
pub use io::{from_json, read_json, to_json, write_json};
pub use sweep::{
    extract_metrics, generate_recommendations, objective_by_name, rank_variants, Metrics,
    ObjectiveFunc, ParameterSweep, SweepResults, SweepSummary, VariantResult,
};
pub use types::{
    Analysis, Conservation, Crossing, Data, Event, Invariant, Metadata, Model, Peak, Results,
    SeriesData, Simulation, SolverOptions, Stat, SteadyState, Summary, TimeData, Timeseries,
    TokenBalance, SCHEMA_VERSION,
};
