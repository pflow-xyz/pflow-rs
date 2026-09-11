//! Process discovery and conformance checking — a port of go-pflow's
//! `mining` package.
//!
//! | module | go-pflow source | what it does |
//! |---|---|---|
//! | [`footprint`] | `mining/footprint.go` | log-based ordering relations (the Alpha/Heuristic foundation) |
//! | [`alpha`] | `mining/alpha.go` | Alpha algorithm discovery |
//! | [`heuristic`] | `mining/heuristic.go` | Heuristic Miner discovery (noise/loop tolerant) |
//! | [`discovery`] | `mining/discovery.go` | sequential/common-path discovery, the `Discover` dispatcher |
//! | [`conformance`] | `mining/conformance.go` | token-based-replay fitness + ETC precision |
//! | [`timing`] | `mining/timing.go` (partial — see module docs) | timing statistics, rate estimation |
//! | [`dataflow_discovery`] | `mining/dataflow_discovery.go` | pipeline-shape (windowing) discovery |

pub mod alpha;
pub mod conformance;
pub mod dataflow_discovery;
pub mod discovery;
pub mod footprint;
pub mod heuristic;
pub mod timing;

pub use alpha::{discover_alpha, mine as mine_alpha, PlaceCandidate};
pub use conformance::{
    check_conformance, check_full_conformance, check_precision, ConformanceResult, FullConformanceResult,
    PrecisionResult, TokenState, TraceReplayResult,
};
pub use dataflow_discovery::{
    discover_pipeline, PipelineDiscoveryOptions, PipelineDiscoveryResult, PipelineDiscoveryStats, PipelineSpec,
    TriggerSpec, WindowSpec,
};
pub use discovery::{discover, discover_common_path, discover_sequential_net, DiscoveryResult, Method};
pub use footprint::{FootprintMatrix, Relation};
pub use heuristic::{
    build_dependency_graph, dependency_matrix, dependency_score, discover_heuristic, loop2_score, loop_score,
    mine as mine_heuristic, top_edges, DependencyEdge, DependencyGraph, HeuristicMinerOptions,
};
pub use timing::{extract_timing, learn_rates_from_log, TimingStatistics};
