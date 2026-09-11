//! CSV event log parsing — a port of go-pflow's `eventlog/csv.go`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::timestamp::{parse_timestamp, DEFAULT_FORMATS};
use crate::types::{Event, EventLog};
use crate::Error;

/// Configures CSV parsing behaviour.
#[derive(Debug, Clone)]
pub struct CsvConfig {
    pub case_id_column: String,
    pub activity_column: String,
    pub timestamp_column: String,
    pub resource_column: String,
    pub lifecycle_column: String,
    pub timestamp_formats: Vec<&'static str>,
    pub delimiter: char,
    pub skip_rows: usize,
}

impl Default for CsvConfig {
    /// Mirrors `eventlog.DefaultCSVConfig()`.
    fn default() -> Self {
        Self {
            case_id_column: "case_id".to_string(),
            activity_column: "activity".to_string(),
            timestamp_column: "timestamp".to_string(),
            resource_column: "resource".to_string(),
            lifecycle_column: "lifecycle".to_string(),
            timestamp_formats: DEFAULT_FORMATS.to_vec(),
            delimiter: ',',
            skip_rows: 0,
        }
    }
}

/// Parses an event log from a CSV file.
pub fn parse_csv_file(path: impl AsRef<Path>, config: &CsvConfig) -> Result<EventLog, Error> {
    let contents = fs::read_to_string(path.as_ref())
        .map_err(|e| Error::Io(format!("opening file: {e}")))?;
    parse_csv_str(&contents, config)
}

/// Parses an event log from CSV text held entirely in memory.
pub fn parse_csv_str(source: &str, config: &CsvConfig) -> Result<EventLog, Error> {
    if config.case_id_column.is_empty() {
        return Err(Error::Config("CaseIDColumn is required".into()));
    }
    if config.activity_column.is_empty() {
        return Err(Error::Config("ActivityColumn is required".into()));
    }
    if config.timestamp_column.is_empty() {
        return Err(Error::Config("TimestampColumn is required".into()));
    }

    let mut rows = split_csv_rows(source, config.delimiter);
    for _ in 0..config.skip_rows {
        if rows.is_empty() {
            return Err(Error::Parse("skipping rows: fewer rows than SkipRows".into()));
        }
        rows.remove(0);
    }
    if rows.is_empty() {
        return Err(Error::Parse("reading header: empty input".into()));
    }
    let header = rows.remove(0);

    let mut col_index: HashMap<String, usize> = HashMap::new();
    for (i, col) in header.iter().enumerate() {
        col_index.insert(col.trim().to_lowercase(), i);
    }

    let case_idx = *col_index
        .get(&config.case_id_column.to_lowercase())
        .ok_or_else(|| Error::Parse(format!("case ID column '{}' not found in header", config.case_id_column)))?;
    let activity_idx = *col_index
        .get(&config.activity_column.to_lowercase())
        .ok_or_else(|| Error::Parse(format!("activity column '{}' not found in header", config.activity_column)))?;
    let timestamp_idx = *col_index
        .get(&config.timestamp_column.to_lowercase())
        .ok_or_else(|| Error::Parse(format!("timestamp column '{}' not found in header", config.timestamp_column)))?;

    let resource_idx = if !config.resource_column.is_empty() {
        col_index.get(&config.resource_column.to_lowercase()).copied()
    } else {
        None
    };
    let lifecycle_idx = if !config.lifecycle_column.is_empty() {
        col_index.get(&config.lifecycle_column.to_lowercase()).copied()
    } else {
        None
    };

    let mut log = EventLog::new();
    let mut line_num = config.skip_rows + 2;

    for record in rows {
        if record.len() <= case_idx || record.len() <= activity_idx || record.len() <= timestamp_idx {
            return Err(Error::Parse(format!("line {line_num}: insufficient columns")));
        }

        let case_id = record[case_idx].trim().to_string();
        let activity = record[activity_idx].trim().to_string();
        let timestamp_str = record[timestamp_idx].trim().to_string();

        if case_id.is_empty() {
            return Err(Error::Parse(format!("line {line_num}: empty case ID")));
        }
        if activity.is_empty() {
            return Err(Error::Parse(format!("line {line_num}: empty activity")));
        }

        let timestamp = parse_timestamp(&timestamp_str, &config.timestamp_formats)
            .ok_or_else(|| Error::Parse(format!("line {line_num}: invalid timestamp '{timestamp_str}'")))?;

        let mut event = Event::new(case_id, activity, timestamp);

        if let Some(idx) = resource_idx {
            if let Some(v) = record.get(idx) {
                event.resource = v.trim().to_string();
            }
        }
        if let Some(idx) = lifecycle_idx {
            if let Some(v) = record.get(idx) {
                event.lifecycle = v.trim().to_string();
            }
        }

        for (i, value) in record.iter().enumerate() {
            if i == case_idx
                || i == activity_idx
                || i == timestamp_idx
                || Some(i) == resource_idx
                || Some(i) == lifecycle_idx
            {
                continue;
            }
            let Some(col_name) = header.get(i) else { continue };
            if col_name.is_empty() {
                continue;
            }
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }
            let attr = match trimmed.parse::<f64>() {
                Ok(n) if n.is_finite() => {
                    serde_json::Number::from_f64(n).map(Value::Number).unwrap_or(Value::String(trimmed.to_string()))
                }
                _ => Value::String(trimmed.to_string()),
            };
            event.attributes.insert(col_name.clone(), attr);
        }

        log.add_event(event);
        line_num += 1;
    }

    log.sort_traces();
    Ok(log)
}

/// Splits CSV text into rows of fields, honoring RFC4180-style quoting
/// (`"a,b"`, doubled `""` as an escaped quote) with a configurable
/// delimiter. Lines are split on `\n`, tolerating a trailing `\r`.
fn split_csv_rows(source: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut field = String::new();
    let mut row = Vec::new();
    let mut in_quotes = false;
    let mut chars = source.chars().peekable();
    let mut saw_any = false;

    while let Some(c) = chars.next() {
        saw_any = true;
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }

        if c == '"' && field.is_empty() {
            in_quotes = true;
        } else if c == delimiter {
            row.push(std::mem::take(&mut field));
        } else if c == '\r' {
            // Swallow; \n (or EOF) ends the row.
        } else if c == '\n' {
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
        } else {
            field.push(c);
        }
    }

    if saw_any && (!field.is_empty() || !row.is_empty()) {
        row.push(field);
        rows.push(row);
    }

    // Drop wholly blank trailing/embedded lines the same way encoding/csv
    // (with default options) skips blank lines.
    rows.into_iter()
        .filter(|r| !(r.len() == 1 && r[0].is_empty()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_csv() {
        let src = "case_id,activity,timestamp,resource\n\
                    o1,pay,2026-09-06T08:01:00Z,till\n\
                    o1,pick_up,2026-09-06T08:05:10Z,\n\
                    o2,pay,2026-09-06T08:02:00Z,till\n";
        let log = parse_csv_str(src, &CsvConfig::default()).unwrap();
        assert_eq!(log.num_cases(), 2);
        assert_eq!(log.num_events(), 3);
        assert_eq!(log.cases["o1"].events[0].resource, "till");
    }

    #[test]
    fn quoted_fields_with_embedded_delimiter() {
        let src = "case_id,activity,timestamp,note\n\
                    o1,pay,2026-09-06T08:01:00Z,\"hello, world\"\n";
        let log = parse_csv_str(src, &CsvConfig::default()).unwrap();
        let ev = &log.cases["o1"].events[0];
        assert_eq!(ev.attributes["note"], Value::String("hello, world".into()));
    }

    #[test]
    fn numeric_attribute_parsed_as_number() {
        let src = "case_id,activity,timestamp,amount\n\
                    o1,pay,2026-09-06T08:01:00Z,4.5\n";
        let log = parse_csv_str(src, &CsvConfig::default()).unwrap();
        let ev = &log.cases["o1"].events[0];
        assert_eq!(ev.attributes["amount"], serde_json::json!(4.5));
    }

    #[test]
    fn missing_case_id_column_errors() {
        let src = "activity,timestamp\npay,2026-09-06T08:01:00Z\n";
        let err = parse_csv_str(src, &CsvConfig::default()).unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
    }

    #[test]
    fn empty_case_id_errors() {
        let src = "case_id,activity,timestamp\n,pay,2026-09-06T08:01:00Z\n";
        let err = parse_csv_str(src, &CsvConfig::default()).unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
    }

    #[test]
    fn invalid_timestamp_errors() {
        let src = "case_id,activity,timestamp\no1,pay,not-a-date\n";
        let err = parse_csv_str(src, &CsvConfig::default()).unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
    }

    #[test]
    fn skip_rows_and_custom_delimiter() {
        let cfg = CsvConfig {
            delimiter: ';',
            skip_rows: 1,
            ..CsvConfig::default()
        };
        let src = "ignored preamble\n\
                    case_id;activity;timestamp\n\
                    o1;pay;2026-09-06T08:01:00Z\n";
        let log = parse_csv_str(src, &cfg).unwrap();
        assert_eq!(log.num_events(), 1);
    }

    #[test]
    fn events_are_sorted_by_timestamp_within_a_trace() {
        let src = "case_id,activity,timestamp\n\
                    o1,pick_up,2026-09-06T08:05:00Z\n\
                    o1,pay,2026-09-06T08:01:00Z\n";
        let log = parse_csv_str(src, &CsvConfig::default()).unwrap();
        assert_eq!(log.cases["o1"].activity_variant(), vec!["pay", "pick_up"]);
    }
}
