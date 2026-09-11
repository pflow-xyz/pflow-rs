//! Real-time predictive process monitoring, ported from go-pflow's
//! `monitoring` package (`types.go`, `monitor.go`, `predictor.go`).
//!
//! Tracks active cases, predicts completion times via
//! [`pflow_solver`] simulation, and detects SLA violations. See
//! `monitor.rs`'s module doc for the one deliberate concurrency-model
//! difference from go-pflow, and `predictor.rs`/`monitor.rs` for smaller
//! notes on faithfully-ported behaviour (including one pre-existing bug).

pub mod monitor;
pub mod predictor;
pub mod types;

pub use predictor::{
    estimate_current_state, predict_next_activity, predict_remaining_time, Predictor,
    SimulationPrediction,
};
pub use types::{
    Alert, AlertHandler, AlertSeverity, AlertType, Case, Event, Monitor, MonitorConfig,
    NextActivity, Prediction, Statistics,
};
