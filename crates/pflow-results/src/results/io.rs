//! Reads and writes [`Results`] as JSON, ported from go-pflow's
//! `results/io.go`.

use std::path::Path;

use thiserror::Error;

use super::types::Results;

/// Errors reading or writing a [`Results`] value.
#[derive(Debug, Error)]
pub enum Error {
    #[error("read file: {0}")]
    Read(#[source] std::io::Error),
    #[error("write file: {0}")]
    Write(#[source] std::io::Error),
    #[error("marshal results: {0}")]
    Marshal(#[source] serde_json::Error),
    #[error("unmarshal results: {0}")]
    Unmarshal(#[source] serde_json::Error),
}

/// Writes `results` to a JSON file, pretty-printed with two-space indent
/// (matching go-pflow's `json.MarshalIndent(results, "", "  ")`).
pub fn write_json(results: &Results, path: impl AsRef<Path>) -> Result<(), Error> {
    let data = to_json(results)?;
    std::fs::write(path, data).map_err(Error::Write)
}

/// Reads a [`Results`] value from a JSON file.
pub fn read_json(path: impl AsRef<Path>) -> Result<Results, Error> {
    let data = std::fs::read_to_string(path).map_err(Error::Read)?;
    from_json(&data)
}

/// Converts `results` to a pretty-printed JSON string.
pub fn to_json(results: &Results) -> Result<String, Error> {
    serde_json::to_string_pretty(results).map_err(Error::Marshal)
}

/// Parses a [`Results`] value from a JSON string.
pub fn from_json(json: &str) -> Result<Results, Error> {
    serde_json::from_str(json).map_err(Error::Unmarshal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample() -> Results {
        super::super::types::Results {
            version: "1.0.0".into(),
            metadata: super::super::types::Metadata {
                timestamp: "1970-01-01T00:00:00Z".into(),
                solver: "Tsit5".into(),
                status: "success".into(),
                error: String::new(),
                compute_time: 0.01,
            },
            model: super::super::types::Model {
                name: "cafe".into(),
                places: vec!["A".into()],
                transitions: vec!["t".into()],
                arcs: 1,
                structure: None,
            },
            simulation: super::super::types::Simulation {
                timespan: [0.0, 10.0],
                initial_state: HashMap::from([("A".to_string(), 5.0)]),
                rates: HashMap::from([("t".to_string(), 1.0)]),
                options: None,
            },
            data: super::super::types::Data {
                summary: super::super::types::Summary {
                    points: 2,
                    final_time: 10.0,
                    final_state: HashMap::from([("A".to_string(), 1.0)]),
                },
                timeseries: super::super::types::Timeseries {
                    time: super::super::types::TimeData {
                        full: vec![],
                        downsampled: vec![0.0, 10.0],
                    },
                    variables: HashMap::new(),
                },
            },
            analysis: None,
            events: vec![],
        }
    }

    #[test]
    fn round_trips_through_json_string() {
        let r = sample();
        let json = to_json(&r).unwrap();
        let back = from_json(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("pflow-results-io-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("results.json");

        let r = sample();
        write_json(&r, &path).unwrap();
        let back = read_json(&path).unwrap();
        assert_eq!(back, r);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_json_rejects_malformed_input() {
        assert!(from_json("{not json").is_err());
    }
}
