//! Process event log types plus CSV/JSONL readers — a port of go-pflow's
//! `eventlog` package (`eventlog/types.go`, `eventlog/csv.go`,
//! `eventlog/jsonl.go`).
//!
//! go-pflow's `eventlog` has readers only (`ParseCSV`, `ParseJSONL`), no
//! writers — nothing in that package or its callers (`mining`, the showcase,
//! petri-pilot's `petri_conformance`) ever serializes an `EventLog` back out,
//! so there is no Go behavior for a writer to hold parity with. This crate
//! matches that: `csv`/`jsonl` are readers.

pub mod csv;
pub mod jsonl;
pub mod timestamp;
pub mod types;

pub use csv::{parse_csv_file, parse_csv_str, CsvConfig};
pub use jsonl::{parse_jsonl_file, parse_jsonl_str, JsonlConfig};
pub use timestamp::{parse_timestamp, Timestamp, DEFAULT_FORMATS};
pub use types::{Event, EventLog, Summary, Trace};

/// Errors parsing an event log.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("io error: {0}")]
    Io(String),
}
