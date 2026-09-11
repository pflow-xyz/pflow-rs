//! Simple discovery methods and the `Discover` dispatcher — a port of
//! go-pflow's `mining/discovery.go`.

use std::collections::HashMap;

use pflow_core::PetriNet;
use pflow_eventlog::EventLog;

use crate::alpha::discover_alpha;
use crate::heuristic::{discover_heuristic, HeuristicMinerOptions};

/// The discovered process model and metadata about how well it covers the log.
#[derive(Debug, Clone)]
pub struct DiscoveryResult {
    pub net: PetriNet,
    pub method: String,
    pub num_variants: usize,
    pub most_common_count: usize,
    pub coverage_percent: f64,
}

/// Creates a simple sequential Petri net: one place before each activity,
/// plus start/end places. This is a basic discovery approach — a linear
/// process model, not concurrency- or choice-aware.
pub fn discover_sequential_net(log: &EventLog) -> PetriNet {
    let activities = log.activities();
    let mut net = PetriNet::new();
    if activities.is_empty() {
        return net;
    }

    net.add_place("start", vec![1.0], vec![], 0.0, 100.0, None);
    for i in 0..activities.len() {
        let x = 100.0 + i as f64 * 150.0;
        net.add_place(format!("p_{i}"), vec![0.0], vec![], x, 100.0, None);
    }
    let end_x = 100.0 + activities.len() as f64 * 150.0;
    net.add_place("end", vec![0.0], vec![], end_x, 100.0, None);

    for (i, activity) in activities.iter().enumerate() {
        let x = 50.0 + i as f64 * 150.0;
        net.add_transition(activity.clone(), "default", x, 100.0, Some(activity.clone()));

        let src_place = if i == 0 { "start".to_string() } else { format!("p_{}", i - 1) };
        let dst_place = if i == activities.len() - 1 {
            "end".to_string()
        } else {
            format!("p_{i}")
        };

        net.add_arc(src_place, activity.clone(), vec![1.0], false);
        net.add_arc(activity.clone(), dst_place, vec![1.0], false);
    }

    net
}

/// Creates a Petri net from the most common activity sequence — the "happy
/// path". Ties on the maximum count are broken deterministically by the
/// variant's first occurrence in case-id order (go-pflow's Go map iteration
/// leaves this undefined; there is no golden pinning a particular choice).
pub fn discover_common_path(log: &EventLog) -> PetriNet {
    let mut variant_counts: HashMap<Vec<String>, usize> = HashMap::new();
    let mut first_seen_order: Vec<Vec<String>> = Vec::new();

    for trace in log.traces() {
        let variant = trace.activity_variant();
        if !variant_counts.contains_key(&variant) {
            first_seen_order.push(variant.clone());
        }
        *variant_counts.entry(variant).or_insert(0) += 1;
    }

    let mut most_common: Option<Vec<String>> = None;
    let mut max_count = 0;
    for variant in &first_seen_order {
        let count = variant_counts[variant];
        if count > max_count {
            max_count = count;
            most_common = Some(variant.clone());
        }
    }

    let Some(activities) = most_common else {
        return PetriNet::new();
    };

    let mut net = PetriNet::new();
    for i in 0..=activities.len() {
        let (place_name, initial, label): (String, f64, Option<String>) = if i == 0 {
            ("start".to_string(), 1.0, Some("Start".to_string()))
        } else if i == activities.len() {
            ("end".to_string(), 0.0, Some("End".to_string()))
        } else {
            (format!("p{i}"), 0.0, Some(format!("After {}", activities[i - 1])))
        };
        let x = 100.0 + i as f64 * 150.0;
        net.add_place(place_name, vec![initial], vec![], x, 100.0, label);
    }

    for (i, activity) in activities.iter().enumerate() {
        let x = 175.0 + i as f64 * 150.0;
        net.add_transition(activity.clone(), "default", x, 100.0, Some(activity.clone()));

        let src_place = if i == 0 { "start".to_string() } else { format!("p{i}") };
        let dst_place = if i < activities.len() - 1 {
            format!("p{}", i + 1)
        } else {
            "end".to_string()
        };

        net.add_arc(src_place, activity.clone(), vec![1.0], false);
        net.add_arc(activity.clone(), dst_place, vec![1.0], false);
    }

    net
}

/// Available discovery methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Sequential,
    CommonPath,
    Alpha,
    Heuristic,
}

impl Method {
    fn label(&self) -> &'static str {
        match self {
            Method::Sequential => "sequential",
            Method::CommonPath => "common-path",
            Method::Alpha => "alpha",
            Method::Heuristic => "heuristic",
        }
    }
}

/// Performs process discovery on an event log using the named method:
/// `"sequential"`, `"common-path"`, `"alpha"` (discovers concurrency,
/// sensitive to noise) or `"heuristic"` (robust to noise, handles loops).
pub fn discover(log: &EventLog, method: &str) -> Result<DiscoveryResult, String> {
    let m = match method {
        "sequential" => Method::Sequential,
        "common-path" => Method::CommonPath,
        "alpha" => Method::Alpha,
        "heuristic" => Method::Heuristic,
        other => {
            return Err(format!(
                "unknown discovery method: {other} (available: sequential, common-path, alpha, heuristic)"
            ))
        }
    };

    let net = match m {
        Method::Sequential => discover_sequential_net(log),
        Method::CommonPath => discover_common_path(log),
        Method::Alpha => discover_alpha(log).net,
        Method::Heuristic => discover_heuristic(log, &HeuristicMinerOptions::default()).net,
    };

    Ok(build_result(log, net, m.label()))
}

pub(crate) fn build_result(log: &EventLog, net: PetriNet, method: &str) -> DiscoveryResult {
    let mut variant_counts: HashMap<Vec<String>, usize> = HashMap::new();
    for trace in log.traces() {
        *variant_counts.entry(trace.activity_variant()).or_insert(0) += 1;
    }

    let max_count = variant_counts.values().copied().max().unwrap_or(0);
    let coverage = if log.num_cases() > 0 {
        max_count as f64 / log.num_cases() as f64 * 100.0
    } else {
        0.0
    };

    DiscoveryResult {
        net,
        method: method.to_string(),
        num_variants: variant_counts.len(),
        most_common_count: max_count,
        coverage_percent: coverage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_eventlog::Event;

    fn log_from_traces(traces: &[&[&str]]) -> EventLog {
        let mut log = EventLog::new();
        for (i, activities) in traces.iter().enumerate() {
            let case = format!("c{i}");
            for (t, activity) in activities.iter().enumerate() {
                log.add_event(Event::new(case.clone(), activity.to_string(), t as i64));
            }
        }
        log
    }

    #[test]
    fn sequential_net_chains_every_activity() {
        let log = log_from_traces(&[&["a", "b", "c"]]);
        let net = discover_sequential_net(&log);
        assert_eq!(net.transitions.len(), 3);
        // start -> a -> p_0 -> b -> p_1 -> c -> end
        assert_eq!(net.places.len(), 5); // start, p_0, p_1, p_2(unused? no)
    }

    #[test]
    fn sequential_net_on_empty_log_is_empty() {
        let net = discover_sequential_net(&EventLog::new());
        assert!(net.places.is_empty());
        assert!(net.transitions.is_empty());
    }

    #[test]
    fn common_path_picks_majority_variant() {
        let log = log_from_traces(&[&["a", "b"], &["a", "b"], &["a", "c"]]);
        let net = discover_common_path(&log);
        assert!(net.transitions.contains_key("a"));
        assert!(net.transitions.contains_key("b"));
        assert!(!net.transitions.contains_key("c"));
    }

    #[test]
    fn discover_dispatches_by_method_name() {
        let log = log_from_traces(&[&["a", "b"]]);
        for method in ["sequential", "common-path", "alpha", "heuristic"] {
            let result = discover(&log, method).unwrap();
            assert_eq!(result.method, method);
        }
    }

    #[test]
    fn discover_rejects_unknown_method() {
        let log = log_from_traces(&[&["a"]]);
        assert!(discover(&log, "nonsense").is_err());
    }

    #[test]
    fn build_result_computes_coverage() {
        let log = log_from_traces(&[&["a", "b"], &["a", "b"], &["a", "c"]]);
        let net = discover_sequential_net(&log);
        let result = build_result(&log, net, "sequential");
        assert_eq!(result.num_variants, 2);
        assert_eq!(result.most_common_count, 2);
        assert!((result.coverage_percent - 66.666_66).abs() < 1e-3);
    }
}
