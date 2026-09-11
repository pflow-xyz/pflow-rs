//! State space analysis for `pflow-metamodel` nets: reachability graphs,
//! coverability, minimal-support P/T invariants and eigenvector centrality.
//! Ported from go-pflow's `reachability` package (ROADMAP.md Phase 2).
//!
//! Every engine here reuses the one shared firing rule
//! ([`pflow_metamodel::Model::enabled`]/[`pflow_metamodel::Model::fire`])
//! rather than reimplementing enablement — see [`graph`]'s module doc.

pub mod analyzer;
pub mod coverability;
pub mod graph;
pub mod invariants;
pub mod marking;
pub mod spectral;

pub use analyzer::{AnalysisResult, Analyzer, ExplorationStats};
pub use coverability::UnboundedWitness;
pub use graph::{Edge, Graph, State};
pub use invariants::{FarkasResult, Invariant, InvariantAnalyzer, TInvariant};
pub use marking::Marking;
pub use spectral::SpectralResult;
