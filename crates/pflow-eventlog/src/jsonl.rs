//! JSONL (JSON Lines) event log parsing — a port of go-pflow's
//! `eventlog/jsonl.go`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::timestamp::{parse_timestamp, Timestamp, DEFAULT_FORMATS};
use crate::types::{Event, EventLog};
use crate::Error;

/// Configures JSONL parsing behaviour.
#[derive(Debug, Clone)]
pub struct JsonlConfig {
    pub case_id_field: String,
    pub activity_field: String,
    pub timestamp_field: String,
    pub resource_field: String,
    pub lifecycle_field: String,
    pub timestamp_formats: Vec<&'static str>,
}

impl Default for JsonlConfig {
    /// Mirrors `eventlog.DefaultJSONLConfig()`.
    fn default() -> Self {
        Self {
            case_id_field: "case_id".to_string(),
            activity_field: "activity".to_string(),
            timestamp_field: "timestamp".to_string(),
            resource_field: "resource".to_string(),
            lifecycle_field: "lifecycle".to_string(),
            timestamp_formats: DEFAULT_FORMATS.to_vec(),
        }
    }
}

/// Parses an event log from a JSONL file.
pub fn parse_jsonl_file(path: impl AsRef<Path>, config: &JsonlConfig) -> Result<EventLog, Error> {
    let contents = fs::read_to_string(path.as_ref())
        .map_err(|e| Error::Io(format!("opening file: {e}")))?;
    parse_jsonl_str(&contents, config)
}

/// Parses an event log from JSONL text held entirely in memory. Each
/// non-blank line must be a JSON object.
pub fn parse_jsonl_str(source: &str, config: &JsonlConfig) -> Result<EventLog, Error> {
    if config.case_id_field.is_empty() {
        return Err(Error::Config("CaseIDField is required".into()));
    }
    if config.activity_field.is_empty() {
        return Err(Error::Config("ActivityField is required".into()));
    }
    if config.timestamp_field.is_empty() {
        return Err(Error::Config("TimestampField is required".into()));
    }

    let formats: &[&str] = if config.timestamp_formats.is_empty() {
        DEFAULT_FORMATS
    } else {
        &config.timestamp_formats
    };

    let mut log = EventLog::new();

    for (i, line) in source.lines().enumerate() {
        let line_num = i + 1;
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }

        let record: HashMap<String, Value> = serde_json::from_str(line)
            .map_err(|e| Error::Parse(format!("line {line_num}: invalid JSON: {e}")))?;

        let case_id = extract_string(&record, &config.case_id_field)
            .map_err(|e| Error::Parse(format!("line {line_num}: {e}")))?;
        let activity = extract_string(&record, &config.activity_field)
            .map_err(|e| Error::Parse(format!("line {line_num}: {e}")))?;
        let timestamp = extract_timestamp(&record, &config.timestamp_field, formats)
            .map_err(|e| Error::Parse(format!("line {line_num}: {e}")))?;

        let mut event = Event::new(case_id, activity, timestamp);

        if !config.resource_field.is_empty() {
            if let Ok(r) = extract_string(&record, &config.resource_field) {
                event.resource = r;
            }
        }
        if !config.lifecycle_field.is_empty() {
            if let Ok(l) = extract_string(&record, &config.lifecycle_field) {
                event.lifecycle = l;
            }
        }

        for (key, value) in record {
            if key == config.case_id_field
                || key == config.activity_field
                || key == config.timestamp_field
                || key == config.resource_field
                || key == config.lifecycle_field
            {
                continue;
            }
            event.attributes.insert(key, value);
        }

        log.add_event(event);
    }

    log.sort_traces();
    Ok(log)
}

fn extract_string(record: &HashMap<String, Value>, field: &str) -> Result<String, String> {
    let value = record
        .get(field)
        .ok_or_else(|| format!("missing required field '{field}'"))?;
    match value {
        Value::String(s) => {
            if s.is_empty() {
                Err(format!("empty value for field '{field}'"))
            } else {
                Ok(s.clone())
            }
        }
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Ok(format!("{f:.0}"))
            } else {
                Ok(n.to_string())
            }
        }
        other => Ok(other.to_string()),
    }
}

fn extract_timestamp(
    record: &HashMap<String, Value>,
    field: &str,
    formats: &[&str],
) -> Result<Timestamp, String> {
    let value = record
        .get(field)
        .ok_or_else(|| format!("missing required field '{field}'"))?;
    match value {
        Value::String(s) => {
            parse_timestamp(s, formats).ok_or_else(|| format!("invalid timestamp '{s}' for field '{field}'"))
        }
        Value::Number(n) => {
            let v = n
                .as_f64()
                .ok_or_else(|| format!("invalid timestamp type for field '{field}'"))?;
            // Unix seconds vs milliseconds, matching Go's `v > 1e12` heuristic.
            if v > 1e12 {
                Ok(v as i64)
            } else {
                Ok((v as i64) * 1000)
            }
        }
        _ => Err(format!("invalid timestamp type for field '{field}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_jsonl() {
        let src = "{\"case_id\":\"o1\",\"activity\":\"pay\",\"timestamp\":\"2026-09-06T08:01:00Z\",\"resource\":\"till\"}\n\
                    {\"case_id\":\"o1\",\"activity\":\"pick_up\",\"timestamp\":\"2026-09-06T08:05:10Z\"}\n";
        let log = parse_jsonl_str(src, &JsonlConfig::default()).unwrap();
        assert_eq!(log.num_cases(), 1);
        assert_eq!(log.num_events(), 2);
        assert_eq!(log.cases["o1"].events[0].resource, "till");
    }

    #[test]
    fn blank_lines_are_skipped() {
        let src = "{\"case_id\":\"o1\",\"activity\":\"pay\",\"timestamp\":\"2026-09-06T08:01:00Z\"}\n\n\n";
        let log = parse_jsonl_str(src, &JsonlConfig::default()).unwrap();
        assert_eq!(log.num_events(), 1);
    }

    #[test]
    fn unix_seconds_and_millis_are_both_accepted() {
        let src = "{\"case_id\":\"o1\",\"activity\":\"a\",\"timestamp\":1700000000}\n\
                    {\"case_id\":\"o1\",\"activity\":\"b\",\"timestamp\":1700000001000}\n";
        let log = parse_jsonl_str(src, &JsonlConfig::default()).unwrap();
        let events = &log.cases["o1"].events;
        assert_eq!(events[1].timestamp - events[0].timestamp, 1000);
    }

    #[test]
    fn extra_fields_become_attributes() {
        let src = "{\"case_id\":\"o1\",\"activity\":\"a\",\"timestamp\":\"2026-09-06T08:01:00Z\",\"amount\":4.5}\n";
        let log = parse_jsonl_str(src, &JsonlConfig::default()).unwrap();
        assert_eq!(log.cases["o1"].events[0].attributes["amount"], serde_json::json!(4.5));
    }

    #[test]
    fn missing_required_field_errors() {
        let src = "{\"activity\":\"a\",\"timestamp\":\"2026-09-06T08:01:00Z\"}\n";
        let err = parse_jsonl_str(src, &JsonlConfig::default()).unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
    }

    #[test]
    fn invalid_json_errors() {
        let src = "not json\n";
        let err = parse_jsonl_str(src, &JsonlConfig::default()).unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
    }

    #[test]
    fn numeric_case_id_is_stringified_without_decimals() {
        let src = "{\"case_id\":42,\"activity\":\"a\",\"timestamp\":\"2026-09-06T08:01:00Z\"}\n";
        let log = parse_jsonl_str(src, &JsonlConfig::default()).unwrap();
        assert!(log.cases.contains_key("42"));
    }
}
