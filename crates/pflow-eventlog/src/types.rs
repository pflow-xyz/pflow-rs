//! Core event log types — a port of go-pflow's `eventlog.Event`/`Trace`/
//! `EventLog`/`Summary` (`eventlog/types.go`).

use std::collections::HashMap;

use serde_json::Value;

use crate::timestamp::Timestamp;

/// A single event in a process execution.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub case_id: String,
    pub activity: String,
    pub timestamp: Timestamp,
    pub resource: String,
    pub lifecycle: String,
    pub attributes: HashMap<String, Value>,
}

impl Event {
    pub fn new(case_id: impl Into<String>, activity: impl Into<String>, timestamp: Timestamp) -> Self {
        Self {
            case_id: case_id.into(),
            activity: activity.into(),
            timestamp,
            resource: String::new(),
            lifecycle: String::new(),
            attributes: HashMap::new(),
        }
    }
}

/// A sequence of events for a single case.
#[derive(Debug, Clone, Default)]
pub struct Trace {
    pub case_id: String,
    pub events: Vec<Event>,
    pub attributes: HashMap<String, Value>,
}

impl Trace {
    /// The sequence of activity names, in event order.
    pub fn activity_variant(&self) -> Vec<String> {
        self.events.iter().map(|e| e.activity.clone()).collect()
    }

    /// Milliseconds from the first to the last event; 0 for fewer than two events.
    pub fn duration_millis(&self) -> i64 {
        if self.events.len() < 2 {
            return 0;
        }
        self.events.last().unwrap().timestamp - self.events.first().unwrap().timestamp
    }

    pub fn start_time(&self) -> Option<Timestamp> {
        self.events.first().map(|e| e.timestamp)
    }

    pub fn end_time(&self) -> Option<Timestamp> {
        self.events.last().map(|e| e.timestamp)
    }
}

/// All traces parsed from a process log.
#[derive(Debug, Clone, Default)]
pub struct EventLog {
    pub cases: HashMap<String, Trace>,
    pub attributes: HashMap<String, Value>,
}

impl EventLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an event to the log, creating a new trace for its case id if needed.
    pub fn add_event(&mut self, event: Event) {
        let trace = self.cases.entry(event.case_id.clone()).or_insert_with(|| Trace {
            case_id: event.case_id.clone(),
            events: Vec::new(),
            attributes: HashMap::new(),
        });
        trace.events.push(event);
    }

    /// Sorts events within each trace by timestamp (stable, so events tied on
    /// a timestamp keep their input order — matches Go's `sort.Slice`, which
    /// is not itself stable, but go-pflow's own tests never rely on tie
    /// order).
    pub fn sort_traces(&mut self) {
        for trace in self.cases.values_mut() {
            trace.events.sort_by_key(|e| e.timestamp);
        }
    }

    /// All traces, sorted by case id for deterministic iteration.
    pub fn traces(&self) -> Vec<&Trace> {
        let mut traces: Vec<&Trace> = self.cases.values().collect();
        traces.sort_by(|a, b| a.case_id.cmp(&b.case_id));
        traces
    }

    pub fn num_cases(&self) -> usize {
        self.cases.len()
    }

    pub fn num_events(&self) -> usize {
        self.cases.values().map(|t| t.events.len()).sum()
    }

    /// Sorted list of unique activity names.
    pub fn activities(&self) -> Vec<String> {
        let mut set: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for trace in self.cases.values() {
            for e in &trace.events {
                set.insert(e.activity.as_str());
            }
        }
        let mut out: Vec<String> = set.into_iter().map(str::to_string).collect();
        out.sort();
        out
    }

    /// Sorted list of unique non-empty resource names.
    pub fn resources(&self) -> Vec<String> {
        let mut set: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for trace in self.cases.values() {
            for e in &trace.events {
                if !e.resource.is_empty() {
                    set.insert(e.resource.as_str());
                }
            }
        }
        let mut out: Vec<String> = set.into_iter().map(str::to_string).collect();
        out.sort();
        out
    }

    /// Summary statistics over the whole log.
    pub fn summarize(&self) -> Summary {
        let mut summary = Summary {
            num_cases: self.num_cases(),
            num_events: self.num_events(),
            num_activities: self.activities().len(),
            num_resources: self.resources().len(),
            ..Default::default()
        };

        if summary.num_cases == 0 {
            return summary;
        }

        let mut variants: HashMap<String, usize> = HashMap::new();
        let mut total_duration: i64 = 0;
        let mut min_time: Option<Timestamp> = None;
        let mut max_time: Option<Timestamp> = None;

        for trace in self.cases.values() {
            let variant = trace.activity_variant().join(">");
            *variants.entry(variant).or_insert(0) += 1;

            if !trace.events.is_empty() {
                total_duration += trace.duration_millis();
                let start = trace.start_time().unwrap();
                let end = trace.end_time().unwrap();
                min_time = Some(min_time.map_or(start, |m| m.min(start)));
                max_time = Some(max_time.map_or(end, |m| m.max(end)));
            }
        }

        summary.num_variants = variants.len();
        summary.start_time = min_time.unwrap_or(0);
        summary.end_time = max_time.unwrap_or(0);
        summary.duration_millis = summary.end_time - summary.start_time;
        summary.avg_case_length = summary.num_events as f64 / summary.num_cases as f64;
        summary.avg_case_duration_millis = total_duration / summary.num_cases as i64;

        summary
    }
}

/// Basic statistics about an event log, mirroring go-pflow's `eventlog.Summary`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub num_cases: usize,
    pub num_events: usize,
    pub num_activities: usize,
    pub num_resources: usize,
    pub num_variants: usize,
    pub start_time: Timestamp,
    pub end_time: Timestamp,
    pub duration_millis: i64,
    pub avg_case_length: f64,
    pub avg_case_duration_millis: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(case: &str, activity: &str, ts: Timestamp) -> Event {
        Event::new(case, activity, ts)
    }

    #[test]
    fn add_event_groups_by_case() {
        let mut log = EventLog::new();
        log.add_event(ev("c1", "a", 1));
        log.add_event(ev("c1", "b", 2));
        log.add_event(ev("c2", "a", 1));

        assert_eq!(log.num_cases(), 2);
        assert_eq!(log.num_events(), 3);
        assert_eq!(log.cases["c1"].events.len(), 2);
    }

    #[test]
    fn sort_traces_orders_by_timestamp() {
        let mut log = EventLog::new();
        log.add_event(ev("c1", "b", 20));
        log.add_event(ev("c1", "a", 10));
        log.sort_traces();

        let variant = log.cases["c1"].activity_variant();
        assert_eq!(variant, vec!["a", "b"]);
    }

    #[test]
    fn traces_are_sorted_by_case_id() {
        let mut log = EventLog::new();
        log.add_event(ev("c2", "x", 1));
        log.add_event(ev("c1", "y", 1));

        let ids: Vec<&str> = log.traces().iter().map(|t| t.case_id.as_str()).collect();
        assert_eq!(ids, vec!["c1", "c2"]);
    }

    #[test]
    fn activities_and_resources_are_sorted_and_deduped() {
        let mut log = EventLog::new();
        let mut e1 = ev("c1", "b", 1);
        e1.resource = "ana".into();
        let mut e2 = ev("c1", "a", 2);
        e2.resource = "ana".into();
        log.add_event(e1);
        log.add_event(e2);

        assert_eq!(log.activities(), vec!["a", "b"]);
        assert_eq!(log.resources(), vec!["ana"]);
    }

    #[test]
    fn trace_duration_and_bounds() {
        let mut trace = Trace {
            case_id: "c1".into(),
            events: vec![ev("c1", "a", 10), ev("c1", "b", 30)],
            attributes: HashMap::new(),
        };
        assert_eq!(trace.duration_millis(), 20);
        assert_eq!(trace.start_time(), Some(10));
        assert_eq!(trace.end_time(), Some(30));

        trace.events.truncate(1);
        assert_eq!(trace.duration_millis(), 0);
    }

    #[test]
    fn summarize_empty_log() {
        let log = EventLog::new();
        let summary = log.summarize();
        assert_eq!(summary, Summary::default());
    }

    #[test]
    fn summarize_computes_variants_and_duration() {
        let mut log = EventLog::new();
        log.add_event(ev("c1", "a", 0));
        log.add_event(ev("c1", "b", 1000));
        log.add_event(ev("c2", "a", 0));
        log.add_event(ev("c2", "b", 2000));

        let summary = log.summarize();
        assert_eq!(summary.num_cases, 2);
        assert_eq!(summary.num_events, 4);
        assert_eq!(summary.num_variants, 1); // both cases are [a, b]
        assert_eq!(summary.avg_case_length, 2.0);
        assert_eq!(summary.avg_case_duration_millis, 1500);
        assert_eq!(summary.start_time, 0);
        assert_eq!(summary.end_time, 2000);
    }
}
