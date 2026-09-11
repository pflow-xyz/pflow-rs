//! Structured simulation output, ported field-for-field from go-pflow's
//! `results/types.go`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Schema version stamped on every [`Results`] value, mirroring
/// `results.SchemaVersion` in go-pflow.
pub const SCHEMA_VERSION: &str = "1.0.0";

/// Complete simulation output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Results {
    pub version: String,
    pub metadata: Metadata,
    pub model: Model,
    pub simulation: Simulation,
    #[serde(rename = "results")]
    pub data: Data,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis: Option<Analysis>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub events: Vec<Event>,
}

/// Simulation execution information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    pub timestamp: String,
    pub solver: String,
    /// success, error, timeout, unstable
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub error: String,
    #[serde(rename = "computeTime")]
    pub compute_time: f64,
}

/// Summary of the Petri net structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub name: String,
    pub places: Vec<String>,
    pub transitions: Vec<String>,
    pub arcs: usize,
    /// Optional: full Petri net, left as untyped JSON like go-pflow's `any`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure: Option<serde_json::Value>,
}

/// Simulation parameters used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Simulation {
    pub timespan: [f64; 2],
    #[serde(rename = "initialState")]
    pub initial_state: HashMap<String, f64>,
    pub rates: HashMap<String, f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<SolverOptions>,
}

/// Solver configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolverOptions {
    #[serde(skip_serializing_if = "is_zero", default)]
    pub dt: f64,
    #[serde(skip_serializing_if = "is_zero", default)]
    pub abstol: f64,
    #[serde(skip_serializing_if = "is_zero", default)]
    pub reltol: f64,
    pub adaptive: bool,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// Simulation results: summary plus timeseries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Data {
    pub summary: Summary,
    pub timeseries: Timeseries,
}

/// Quick overview of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub points: usize,
    #[serde(rename = "finalTime")]
    pub final_time: f64,
    #[serde(rename = "finalState")]
    pub final_state: HashMap<String, f64>,
}

/// Multi-resolution time series data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeseries {
    pub time: TimeData,
    pub variables: HashMap<String, SeriesData>,
}

/// Time vectors at different resolutions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TimeData {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub full: Vec<f64>,
    pub downsampled: Vec<f64>,
}

/// Values at different resolutions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SeriesData {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub full: Vec<f64>,
    pub downsampled: Vec<f64>,
}

/// Automatically computed insights.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Analysis {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub peaks: Vec<Peak>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub troughs: Vec<Peak>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub crossings: Vec<Crossing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "steadyState")]
    pub steady_state: Option<SteadyState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conservation: Option<Conservation>,
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub statistics: HashMap<String, Stat>,
}

/// A local maximum or minimum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Peak {
    pub variable: String,
    pub time: f64,
    pub value: f64,
    #[serde(skip_serializing_if = "is_zero", default)]
    pub prominence: f64,
}

/// Where two variables intersect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crossing {
    pub var1: String,
    pub var2: String,
    pub time: f64,
    pub value: f64,
}

/// Equilibrium analysis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SteadyState {
    pub reached: bool,
    #[serde(skip_serializing_if = "is_zero", default)]
    pub time: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<HashMap<String, f64>>,
    pub tolerance: f64,
}

/// Mass-balance tracking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conservation {
    #[serde(rename = "totalTokens")]
    pub total_tokens: TokenBalance,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub invariants: Vec<Invariant>,
}

/// Total token conservation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenBalance {
    pub initial: f64,
    pub r#final: f64,
    pub conserved: bool,
}

/// A P-invariant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invariant {
    pub places: Vec<String>,
    pub coefficients: Vec<f64>,
    pub value: f64,
}

/// Statistical summary.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Stat {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub median: f64,
    pub std: f64,
}

/// A notable occurrence during simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub time: f64,
    /// threshold_exceeded, rate_change, etc.
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<HashMap<String, serde_json::Value>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Results {
        Results {
            version: SCHEMA_VERSION.to_string(),
            metadata: Metadata {
                timestamp: "1970-01-01T00:00:00Z".into(),
                solver: "Tsit5".into(),
                status: "success".into(),
                error: String::new(),
                compute_time: 0.5,
            },
            model: Model {
                name: "cafe".into(),
                places: vec!["A".into(), "B".into()],
                transitions: vec!["t".into()],
                arcs: 2,
                structure: None,
            },
            simulation: Simulation {
                timespan: [0.0, 24.0],
                initial_state: HashMap::from([("A".to_string(), 5.0)]),
                rates: HashMap::from([("t".to_string(), 1.0)]),
                options: Some(SolverOptions {
                    dt: 0.01,
                    abstol: 1e-6,
                    reltol: 1e-3,
                    adaptive: true,
                }),
            },
            data: Data {
                summary: Summary {
                    points: 2,
                    final_time: 24.0,
                    final_state: HashMap::from([("A".to_string(), 1.0)]),
                },
                timeseries: Timeseries {
                    time: TimeData {
                        full: vec![],
                        downsampled: vec![0.0, 24.0],
                    },
                    variables: HashMap::new(),
                },
            },
            analysis: Some(Analysis {
                peaks: vec![Peak {
                    variable: "A".into(),
                    time: 1.0,
                    value: 9.0,
                    prominence: 3.0,
                }],
                troughs: vec![],
                crossings: vec![Crossing {
                    var1: "A".into(),
                    var2: "B".into(),
                    time: 2.0,
                    value: 4.0,
                }],
                steady_state: Some(SteadyState {
                    reached: true,
                    time: 20.0,
                    values: Some(HashMap::from([("A".to_string(), 1.0)])),
                    tolerance: 0.01,
                }),
                conservation: Some(Conservation {
                    total_tokens: TokenBalance {
                        initial: 5.0,
                        r#final: 5.0,
                        conserved: true,
                    },
                    invariants: vec![Invariant {
                        places: vec!["A".into()],
                        coefficients: vec![1.0],
                        value: 5.0,
                    }],
                }),
                statistics: HashMap::from([(
                    "A".to_string(),
                    Stat {
                        min: 0.0,
                        max: 9.0,
                        mean: 3.0,
                        median: 2.0,
                        std: 1.0,
                    },
                )]),
            }),
            events: vec![Event {
                time: 3.0,
                kind: "threshold_exceeded".into(),
                description: "A crossed 5".into(),
                data: None,
            }],
        }
    }

    #[test]
    fn round_trips_through_serde_json() {
        let r = sample();
        let json = serde_json::to_string(&r).unwrap();
        let back: Results = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    /// Field names must match go-pflow's `json:"..."` tags exactly, so a
    /// fixture go-pflow writes and a `Results` this crate serializes are
    /// interchangeable.
    #[test]
    fn json_field_names_match_go_pflow_tags() {
        let r = sample();
        let json = serde_json::to_string(&r).unwrap();
        for key in [
            "\"initialState\"",
            "\"finalState\"",
            "\"finalTime\"",
            "\"computeTime\"",
            "\"steadyState\"",
            "\"totalTokens\"",
        ] {
            assert!(json.contains(key), "missing {key} in {json}");
        }
    }

    #[test]
    fn library_tolerates_unknown_fields() {
        // The library itself never rejects an unknown field — only this
        // crate's own tests hold a strict reader to that contract, matching
        // go-pflow's "tolerate at the library, reject only in test
        // fixtures" rule (see pflow-metamodel::schema for the same pattern).
        let json =
            r#"{"min":1.0,"max":2.0,"mean":1.5,"median":1.5,"std":0.5,"fromTheFuture":true}"#;
        let s: Stat = serde_json::from_str(json).unwrap();
        assert_eq!(s.min, 1.0);
    }

    /// Strict reader used only by this crate's own tests, never by the
    /// library.
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StrictStat {
        #[allow(dead_code)]
        min: f64,
        #[allow(dead_code)]
        max: f64,
        #[allow(dead_code)]
        mean: f64,
        #[allow(dead_code)]
        median: f64,
        #[allow(dead_code)]
        std: f64,
    }

    #[test]
    fn strict_reader_rejects_unknown_fields() {
        let json =
            r#"{"min":1.0,"max":2.0,"mean":1.5,"median":1.5,"std":0.5,"fromTheFuture":true}"#;
        assert!(serde_json::from_str::<StrictStat>(json).is_err());
    }
}
