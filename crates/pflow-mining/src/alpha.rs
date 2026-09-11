//! The Alpha algorithm for process discovery — a port of go-pflow's
//! `mining/alpha.go`.
//!
//! Discovers a Petri net from an event log based on ordering relations:
//! 1. Build a footprint matrix of activity relations.
//! 2. Identify places from maximal pairs `(A, B)` where `A -> B` and `A`,
//!    `B` are each internally unrelated.
//! 3. Construct the net: transitions for activities, places for control flow.
//!
//! Limitations, inherited from the algorithm itself: cannot handle loops of
//! length 1 or 2, sensitive to noise, may produce unsound models for complex
//! processes. For noisy logs prefer [`crate::heuristic`].

use pflow_core::PetriNet;
use pflow_eventlog::EventLog;

use crate::discovery::{build_result, DiscoveryResult};
use crate::footprint::FootprintMatrix;

/// A candidate place connecting a set of input transitions to a set of
/// output transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceCandidate {
    pub input_set: Vec<String>,
    pub output_set: Vec<String>,
}

impl PlaceCandidate {
    /// A deterministic identifier for the candidate, sorted so that set
    /// order doesn't matter — used for dedup and for the generated place name.
    pub fn id(&self) -> String {
        let mut input = self.input_set.clone();
        input.sort();
        let mut output = self.output_set.clone();
        output.sort();
        format!("p_{}_{}", input.join("_"), output.join("_"))
    }
}

/// Discovers a Petri net using the Alpha algorithm.
pub fn mine(log: &EventLog) -> PetriNet {
    let fp = FootprintMatrix::new(log);
    let mut net = PetriNet::new();

    let activities = fp.activities.clone();
    for (i, activity) in activities.iter().enumerate() {
        let x = 150.0 + i as f64 * 120.0;
        net.add_transition(activity.clone(), "default", x, 200.0, Some(activity.clone()));
    }

    let candidates = find_place_candidates(&fp);
    let maximal = filter_maximal(candidates);

    for (place_index, pc) in maximal.iter().enumerate() {
        let place_name = pc.id();
        let x = 100.0 + place_index as f64 * 100.0;
        net.add_place(place_name.clone(), vec![0.0], vec![], x, 100.0, None);

        for input in &pc.input_set {
            net.add_arc(input.clone(), place_name.clone(), vec![1.0], false);
        }
        for output in &pc.output_set {
            net.add_arc(place_name.clone(), output.clone(), vec![1.0], false);
        }
    }

    let start_activities = fp.start_activities();
    if !start_activities.is_empty() {
        net.add_place("start", vec![1.0], vec![], 50.0, 200.0, Some("start".to_string()));
        for act in &start_activities {
            net.add_arc("start", act.clone(), vec![1.0], false);
        }
    }

    let end_activities = fp.end_activities();
    if !end_activities.is_empty() {
        let x = 150.0 + activities.len() as f64 * 120.0;
        net.add_place("end", vec![0.0], vec![], x, 200.0, Some("end".to_string()));
        for act in &end_activities {
            net.add_arc(act.clone(), "end", vec![1.0], false);
        }
    }

    net
}

/// Finds all valid place candidates `(A, B)` where every pair within `A` (and
/// within `B`) is in choice relation and every `a in A` causally precedes
/// every `b in B`.
fn find_place_candidates(fp: &FootprintMatrix) -> Vec<PlaceCandidate> {
    let mut candidates = Vec::new();
    let activities = &fp.activities;
    let max_set_size = activities.len().min(5); // Limit for performance, as go-pflow does.

    for size_a in 1..=max_set_size {
        for size_b in 1..=max_set_size {
            for set_a in generate_subsets(activities, size_a) {
                if !fp.set_is_unrelated(&set_a) {
                    continue;
                }
                for set_b in generate_subsets(activities, size_b) {
                    if !fp.set_is_unrelated(&set_b) {
                        continue;
                    }
                    if fp.sets_causally_connected(&set_a, &set_b) {
                        candidates.push(PlaceCandidate {
                            input_set: set_a.clone(),
                            output_set: set_b.clone(),
                        });
                    }
                }
            }
        }
    }

    candidates
}

/// Filters place candidates down to the maximal ones: `(A, B)` is maximal if
/// no other candidate `(A', B')` has `A subset A'`, `B subset B'` and
/// `(A,B) != (A',B')`.
fn filter_maximal(candidates: Vec<PlaceCandidate>) -> Vec<PlaceCandidate> {
    let mut maximal = Vec::new();
    for (i, c1) in candidates.iter().enumerate() {
        let dominated = candidates.iter().enumerate().any(|(j, c2)| {
            i != j && is_subset_of(&c1.input_set, &c2.input_set) && is_subset_of(&c1.output_set, &c2.output_set)
        });
        if !dominated {
            maximal.push(c1.clone());
        }
    }
    // Dedup by id (distinct candidates can be identical after sort/join —
    // go-pflow doesn't dedup here, but two identical (A,B) pairs would just
    // overwrite the same place name/arcs in AddPlace/AddArc, which is a
    // silent no-op there; deduping first is the same end state, explicit.
    let mut seen = std::collections::HashSet::new();
    maximal.retain(|c| seen.insert(c.id()));
    maximal
}

fn generate_subsets(elements: &[String], size: usize) -> Vec<Vec<String>> {
    if size == 0 {
        return vec![vec![]];
    }
    if size > elements.len() {
        return vec![];
    }
    let mut result = Vec::new();
    let mut current = Vec::new();
    generate(elements, size, 0, &mut current, &mut result);
    result
}

fn generate(elements: &[String], size: usize, start: usize, current: &mut Vec<String>, result: &mut Vec<Vec<String>>) {
    if current.len() == size {
        result.push(current.clone());
        return;
    }
    for i in start..elements.len() {
        current.push(elements[i].clone());
        generate(elements, size, i + 1, current, result);
        current.pop();
    }
}

fn is_subset_of(set_a: &[String], set_b: &[String]) -> bool {
    let b: std::collections::HashSet<&String> = set_b.iter().collect();
    set_a.iter().all(|a| b.contains(a))
}

/// Runs the Alpha algorithm and computes discovery metadata.
pub fn discover_alpha(log: &EventLog) -> DiscoveryResult {
    let net = mine(log);
    build_result(log, net, "alpha")
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
    fn place_candidate_id_is_order_independent() {
        let pc1 = PlaceCandidate {
            input_set: vec!["a".into(), "b".into()],
            output_set: vec!["c".into()],
        };
        let pc2 = PlaceCandidate {
            input_set: vec!["b".into(), "a".into()],
            output_set: vec!["c".into()],
        };
        assert_eq!(pc1.id(), pc2.id());
    }

    #[test]
    fn generate_subsets_of_size_two() {
        let elements = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let subsets = generate_subsets(&elements, 2);
        assert_eq!(subsets.len(), 3);
        assert!(subsets.contains(&vec!["a".to_string(), "b".to_string()]));
        assert!(subsets.contains(&vec!["b".to_string(), "c".to_string()]));
    }

    #[test]
    fn is_subset_of_basic() {
        assert!(is_subset_of(&["a".to_string()], &["a".to_string(), "b".to_string()]));
        assert!(!is_subset_of(&["c".to_string()], &["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn mines_a_sequential_net() {
        let log = log_from_traces(&[&["a", "b", "c"]]);
        let net = mine(&log);
        assert_eq!(net.transitions.len(), 3);
        assert!(net.places.contains_key("start"));
        assert!(net.places.contains_key("end"));
        // A place should connect a->b and one for b->c.
        assert!(net.arcs.iter().any(|arc| arc.source == "a"));
        assert!(net.arcs.iter().any(|arc| arc.target == "c"));
    }

    #[test]
    fn discover_alpha_reports_coverage() {
        let log = log_from_traces(&[&["a", "b"], &["a", "b"]]);
        let result = discover_alpha(&log);
        assert_eq!(result.method, "alpha");
        assert_eq!(result.coverage_percent, 100.0);
    }
}
