//! Timing statistics extracted from an event log — a port of the
//! `ExtractTiming`/`TimingStatistics`/`LearnRatesFromLog` half of go-pflow's
//! `mining/timing.go`.
//!
//! `FitRateFunctionsFromLog` and `CompareToLog` are not ported:
//! `FitRateFunctionsFromLog` wraps `EstimateRate` in a `learn.RateFunc` for
//! go-pflow's `learn` package (pflow-learn's equivalent rate-function type
//! isn't wired to this crate, and the extra indirection adds nothing
//! `EstimateRate` doesn't already give a caller), and `CompareToLog` is an
//! unimplemented stub on the Go side (`CompareSimulationToLog`'s doc comment
//! says "Future: implement actual simulation comparison" — it discards its
//! log argument and returns a zeroed result) — there is no behavior there to
//! hold parity with.

use std::collections::HashMap;

use pflow_core::PetriNet;
use pflow_eventlog::EventLog;

/// Timing information extracted from an event log. Durations and
/// inter-arrival times are in milliseconds (go-pflow reports seconds as
/// `float64`; this crate's `Timestamp` is already millisecond-resolution).
#[derive(Debug, Clone, Default)]
pub struct TimingStatistics {
    /// Activity -> observed durations to the next event in the same trace.
    pub activity_durations: HashMap<String, Vec<f64>>,
    /// Time between consecutive case starts, sorted by start time.
    pub inter_arrival_times: Vec<f64>,
    /// Total duration (first to last event) per case.
    pub case_durations: Vec<f64>,
    /// Activity -> number of occurrences across the whole log.
    pub activity_counts: HashMap<String, usize>,
}

/// Extracts timing statistics from an event log.
pub fn extract_timing(log: &EventLog) -> TimingStatistics {
    let mut stats = TimingStatistics::default();
    let mut case_starts = Vec::new();

    for trace in log.traces() {
        if trace.events.is_empty() {
            continue;
        }

        stats.case_durations.push(trace.duration_millis() as f64);
        case_starts.push(trace.start_time().unwrap());

        for w in trace.events.windows(2) {
            let activity = &w[0].activity;
            let duration = (w[1].timestamp - w[0].timestamp) as f64;
            stats.activity_durations.entry(activity.clone()).or_default().push(duration);
            *stats.activity_counts.entry(activity.clone()).or_insert(0) += 1;
        }

        let last_activity = &trace.events[trace.events.len() - 1].activity;
        *stats.activity_counts.entry(last_activity.clone()).or_insert(0) += 1;
    }

    case_starts.sort();
    for w in case_starts.windows(2) {
        stats.inter_arrival_times.push((w[1] - w[0]) as f64);
    }

    stats
}

impl TimingStatistics {
    pub fn mean_duration(&self, activity: &str) -> f64 {
        let Some(durations) = self.activity_durations.get(activity) else {
            return 0.0;
        };
        if durations.is_empty() {
            return 0.0;
        }
        durations.iter().sum::<f64>() / durations.len() as f64
    }

    pub fn std_duration(&self, activity: &str) -> f64 {
        let Some(durations) = self.activity_durations.get(activity) else {
            return 0.0;
        };
        if durations.len() < 2 {
            return 0.0;
        }
        let mean = self.mean_duration(activity);
        let sum_sq: f64 = durations.iter().map(|d| (d - mean).powi(2)).sum();
        (sum_sq / (durations.len() - 1) as f64).sqrt()
    }

    /// `1 / mean_duration`, in events per millisecond — matches the Go
    /// package's `1/mean_duration` in whatever unit the durations are in.
    /// Returns `0.1` (go-pflow's default) if there's no data.
    pub fn estimate_rate(&self, activity: &str) -> f64 {
        let mean = self.mean_duration(activity);
        if mean <= 0.0 {
            0.1
        } else {
            1.0 / mean
        }
    }
}

/// Learns transition rates from an event log for a given Petri net, mapping
/// event-log activities to net transitions by name.
pub fn learn_rates_from_log(log: &EventLog, net: &PetriNet) -> HashMap<String, f64> {
    let stats = extract_timing(log);
    net.transitions
        .keys()
        .map(|name| (name.clone(), stats.estimate_rate(name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_eventlog::Event;

    fn log_with_two_cases() -> EventLog {
        let mut log = EventLog::new();
        log.add_event(Event::new("c1", "a", 0));
        log.add_event(Event::new("c1", "b", 1000));
        log.add_event(Event::new("c1", "c", 3000));

        log.add_event(Event::new("c2", "a", 500));
        log.add_event(Event::new("c2", "b", 2500));
        log.add_event(Event::new("c2", "c", 4500));
        log
    }

    #[test]
    fn extract_timing_computes_activity_durations() {
        let log = log_with_two_cases();
        let stats = extract_timing(&log);

        assert_eq!(stats.activity_durations["a"], vec![1000.0, 2000.0]);
        assert_eq!(stats.activity_counts["a"], 2);
        assert_eq!(stats.activity_counts["c"], 2); // last activity still counted
    }

    #[test]
    fn extract_timing_computes_case_durations_and_inter_arrival() {
        let log = log_with_two_cases();
        let stats = extract_timing(&log);

        assert_eq!(stats.case_durations.len(), 2);
        assert!(stats.case_durations.contains(&3000.0));
        assert_eq!(stats.inter_arrival_times, vec![500.0]);
    }

    #[test]
    fn mean_and_std_duration() {
        let log = log_with_two_cases();
        let stats = extract_timing(&log);
        assert_eq!(stats.mean_duration("a"), 1500.0);
        assert!(stats.std_duration("a") > 0.0);
        assert_eq!(stats.std_duration("nonexistent"), 0.0);
    }

    #[test]
    fn estimate_rate_defaults_when_no_data() {
        let stats = TimingStatistics::default();
        assert_eq!(stats.estimate_rate("missing"), 0.1);
    }

    #[test]
    fn learn_rates_from_log_covers_every_transition() {
        let log = log_with_two_cases();
        let mut net = PetriNet::new();
        net.add_transition("a", "default", 0.0, 0.0, None);
        net.add_transition("b", "default", 0.0, 0.0, None);

        let rates = learn_rates_from_log(&log, &net);
        assert_eq!(rates.len(), 2);
        assert!(rates["a"] > 0.0);
    }
}
