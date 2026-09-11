//! The Heuristic Miner algorithm for process discovery — a port of
//! go-pflow's `mining/heuristic.go`.
//!
//! Unlike the Alpha algorithm, this handles noise and short loops: it uses
//! dependency measures derived from directly-follows counts to decide which
//! activities are causally related, rather than requiring exact choice/causal
//! relations.

use std::collections::HashMap;

use pflow_core::PetriNet;
use pflow_eventlog::EventLog;

use crate::discovery::{build_result, DiscoveryResult};
use crate::footprint::FootprintMatrix;

/// Tuning knobs for the Heuristic Miner.
#[derive(Debug, Clone)]
pub struct HeuristicMinerOptions {
    /// Minimum dependency score (0-1) to include a causal relation. Higher
    /// values produce simpler models.
    pub dependency_threshold: f64,
    /// Threshold for detecting AND splits/joins (parallelism). Reserved for
    /// future use, mirroring go-pflow's field (not yet consumed there either).
    pub and_threshold: f64,
    /// Minimum score to detect length-1 and length-2 loops.
    pub loop_threshold: f64,
}

impl Default for HeuristicMinerOptions {
    fn default() -> Self {
        Self {
            dependency_threshold: 0.5,
            and_threshold: 0.1,
            loop_threshold: 0.5,
        }
    }
}

/// The dependency score between activities `a` and `b`, in `[-1, 1]`.
/// Near 1: strong causal relation `a -> b`. Near 0: no clear relation. Near
/// -1: strong reverse relation `b -> a`.
///
/// `(|a>b| - |b>a|) / (|a>b| + |b>a| + 1)`
pub fn dependency_score(fp: &FootprintMatrix, a: &str, b: &str) -> f64 {
    let a_to_b = fp.directly_follows_count(a, b) as f64;
    let b_to_a = fp.directly_follows_count(b, a) as f64;
    if a_to_b + b_to_a == 0.0 {
        return 0.0;
    }
    (a_to_b - b_to_a) / (a_to_b + b_to_a + 1.0)
}

/// The score for a length-1 loop (`a > a`): `|a>a| / (|a>a| + 1)`.
pub fn loop_score(fp: &FootprintMatrix, a: &str) -> f64 {
    let self_loop = fp.directly_follows_count(a, a) as f64;
    self_loop / (self_loop + 1.0)
}

/// The score for a length-2 loop (`a > b > a`):
/// `(|a>b| + |b>a|) / (|a>b| + |b>a| + 1)`.
pub fn loop2_score(fp: &FootprintMatrix, a: &str, b: &str) -> f64 {
    let a_to_b = fp.directly_follows_count(a, b) as f64;
    let b_to_a = fp.directly_follows_count(b, a) as f64;
    if a_to_b == 0.0 || b_to_a == 0.0 {
        return 0.0;
    }
    (a_to_b + b_to_a) / (a_to_b + b_to_a + 1.0)
}

/// The causal dependency graph the miner constructs before building a net.
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    pub nodes: Vec<String>,
    /// `a -> b -> score`, filtered by `dependency_threshold`.
    pub edges: Vec<(String, String, f64)>,
    pub start_nodes: Vec<String>,
    pub end_nodes: Vec<String>,
    pub self_loops: HashMap<String, f64>,
}

/// Builds the dependency graph for a log under the given options.
pub fn build_dependency_graph(log: &EventLog, opts: &HeuristicMinerOptions) -> DependencyGraph {
    let fp = FootprintMatrix::new(log);
    let activities = fp.activities.clone();

    let mut self_loops = HashMap::new();
    for a in &activities {
        let score = loop_score(&fp, a);
        if score >= opts.loop_threshold {
            self_loops.insert(a.clone(), score);
        }
    }

    let mut edges = Vec::new();
    for a in &activities {
        for b in &activities {
            if a == b {
                continue;
            }
            let score = dependency_score(&fp, a, b);
            if score >= opts.dependency_threshold {
                edges.push((a.clone(), b.clone(), score));
            }
        }
    }

    DependencyGraph {
        nodes: activities,
        edges,
        start_nodes: fp.start_activities(),
        end_nodes: fp.end_activities(),
        self_loops,
    }
}

/// Discovers a Petri net using the Heuristic Miner algorithm.
pub fn mine(log: &EventLog, opts: &HeuristicMinerOptions) -> PetriNet {
    let graph = build_dependency_graph(log, opts);
    let mut net = PetriNet::new();

    for (i, activity) in graph.nodes.iter().enumerate() {
        let x = 150.0 + i as f64 * 120.0;
        net.add_transition(activity.clone(), "default", x, 200.0, Some(activity.clone()));
    }

    // Places are created deterministically in (a, b) edge order — go-pflow
    // iterates a Go map here, so its `p%d` numbering (which carries no
    // semantic meaning) is nondeterministic across runs; this instead orders
    // by the edges vector, itself already sorted by discovery order.
    for (i, (a, b, _score)) in graph.edges.iter().enumerate() {
        let place_name = format!("p{i}");
        let a_idx = graph.nodes.iter().position(|n| n == a).unwrap_or(0);
        let b_idx = graph.nodes.iter().position(|n| n == b).unwrap_or(0);
        let x = 150.0 + (a_idx + b_idx) as f64 * 60.0;

        net.add_place(place_name.clone(), vec![0.0], vec![], x, 100.0, None);
        net.add_arc(a.clone(), place_name.clone(), vec![1.0], false);
        net.add_arc(place_name, b.clone(), vec![1.0], false);
    }

    let mut loop_activities: Vec<&String> = graph.self_loops.keys().collect();
    loop_activities.sort();
    for a in loop_activities {
        let score = graph.self_loops[a];
        if score >= opts.loop_threshold {
            let place_name = format!("loop_{a}");
            let a_idx = graph.nodes.iter().position(|n| n == a).unwrap_or(0);
            let x = 150.0 + a_idx as f64 * 120.0;

            net.add_place(place_name.clone(), vec![1.0], vec![], x, 50.0, None);
            net.add_arc(a.clone(), place_name.clone(), vec![1.0], false);
            net.add_arc(place_name, a.clone(), vec![1.0], false);
        }
    }

    if !graph.start_nodes.is_empty() {
        net.add_place("start", vec![1.0], vec![], 50.0, 200.0, Some("start".to_string()));
        for act in &graph.start_nodes {
            net.add_arc("start", act.clone(), vec![1.0], false);
        }
    }

    if !graph.end_nodes.is_empty() {
        let x = 150.0 + graph.nodes.len() as f64 * 120.0;
        net.add_place("end", vec![0.0], vec![], x, 200.0, Some("end".to_string()));
        for act in &graph.end_nodes {
            net.add_arc(act.clone(), "end", vec![1.0], false);
        }
    }

    net
}

/// The full `activity x activity` dependency score matrix.
pub fn dependency_matrix(log: &EventLog) -> HashMap<String, HashMap<String, f64>> {
    let fp = FootprintMatrix::new(log);
    let mut matrix = HashMap::new();
    for a in &fp.activities {
        let mut row = HashMap::new();
        for b in &fp.activities {
            row.insert(b.clone(), dependency_score(&fp, a, b));
        }
        matrix.insert(a.clone(), row);
    }
    matrix
}

/// One edge in the dependency graph, for [`top_edges`].
#[derive(Debug, Clone, PartialEq)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub score: f64,
}

/// The top `n` positive-scoring edges by dependency score, descending.
pub fn top_edges(log: &EventLog, n: usize) -> Vec<DependencyEdge> {
    let fp = FootprintMatrix::new(log);
    let mut edges = Vec::new();
    for a in &fp.activities {
        for b in &fp.activities {
            if a == b {
                continue;
            }
            let score = dependency_score(&fp, a, b);
            if score > 0.0 {
                edges.push(DependencyEdge {
                    from: a.clone(),
                    to: b.clone(),
                    score,
                });
            }
        }
    }
    edges.sort_by(|x, y| y.score.partial_cmp(&x.score).unwrap());
    edges.truncate(n);
    edges
}

/// Runs the Heuristic Miner with default options and computes discovery metadata.
pub fn discover_heuristic(log: &EventLog, opts: &HeuristicMinerOptions) -> DiscoveryResult {
    let net = mine(log, opts);
    build_result(log, net, "heuristic")
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
    fn dependency_score_favors_the_observed_direction() {
        let log = log_from_traces(&[&["a", "b"], &["a", "b"], &["a", "b"]]);
        let fp = FootprintMatrix::new(&log);
        let score = dependency_score(&fp, "a", "b");
        assert!(score > 0.5, "expected strong a->b dependency, got {score}");
        assert_eq!(dependency_score(&fp, "b", "a"), -score);
    }

    #[test]
    fn dependency_score_zero_when_never_adjacent() {
        let log = log_from_traces(&[&["a", "c"], &["b", "c"]]);
        let fp = FootprintMatrix::new(&log);
        assert_eq!(dependency_score(&fp, "a", "b"), 0.0);
    }

    #[test]
    fn loop_score_increases_with_repetition() {
        let mut log = EventLog::new();
        log.add_event(Event::new("c1", "a", 0));
        log.add_event(Event::new("c1", "a", 1));
        log.add_event(Event::new("c1", "a", 2));
        let fp = FootprintMatrix::new(&log);
        assert_eq!(loop_score(&fp, "a"), 2.0 / 3.0);
    }

    #[test]
    fn loop2_score_requires_both_directions() {
        let log = log_from_traces(&[&["a", "b", "a"]]);
        let fp = FootprintMatrix::new(&log);
        assert!(loop2_score(&fp, "a", "b") > 0.0);
        let log2 = log_from_traces(&[&["a", "b"]]);
        let fp2 = FootprintMatrix::new(&log2);
        assert_eq!(loop2_score(&fp2, "a", "b"), 0.0);
    }

    #[test]
    fn build_dependency_graph_filters_by_threshold() {
        let log = log_from_traces(&[&["a", "b"], &["a", "b"]]);
        let opts = HeuristicMinerOptions {
            dependency_threshold: 2.0, // impossibly high
            ..Default::default()
        };
        let graph = build_dependency_graph(&log, &opts);
        assert!(graph.edges.is_empty());
    }

    #[test]
    fn mine_produces_transitions_for_every_activity() {
        let log = log_from_traces(&[&["a", "b", "c"], &["a", "b", "c"]]);
        let net = mine(&log, &HeuristicMinerOptions::default());
        assert_eq!(net.transitions.len(), 3);
        assert!(net.places.contains_key("start"));
        assert!(net.places.contains_key("end"));
    }

    #[test]
    fn discover_heuristic_reports_method_name() {
        let log = log_from_traces(&[&["a", "b"]]);
        let result = discover_heuristic(&log, &HeuristicMinerOptions::default());
        assert_eq!(result.method, "heuristic");
    }

    #[test]
    fn top_edges_sorted_descending_and_truncated() {
        let log = log_from_traces(&[&["a", "b"], &["a", "b"], &["a", "c"]]);
        let edges = top_edges(&log, 1);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from, "a");
    }
}
