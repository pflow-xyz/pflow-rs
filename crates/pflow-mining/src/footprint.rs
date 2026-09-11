//! Log-based ordering relations between activities — a port of go-pflow's
//! `mining/footprint.go`. This is the foundation the Alpha and Heuristic
//! miners both build on.

use std::collections::{HashMap, HashSet};

use pflow_eventlog::EventLog;

/// The ordering relation between two activities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// Never directly follow each other.
    NoRelation,
    /// `a > b`: `a` is directly followed by `b` at least once.
    DirectlyFollows,
    /// `a -> b`: `a` causes `b` (`a > b` and not `b > a`).
    Causality,
    /// `a <- b`: `b` causes `a`.
    ReverseCausality,
    /// `a || b`: both orderings exist.
    Parallel,
    /// `a # b`: neither ordering exists (exclusive choice).
    Choice,
}

impl Relation {
    pub fn symbol(&self) -> &'static str {
        match self {
            Relation::NoRelation => "#",
            Relation::DirectlyFollows => ">",
            Relation::Causality => "\u{2192}",
            Relation::ReverseCausality => "\u{2190}",
            Relation::Parallel => "||",
            Relation::Choice => "#",
        }
    }
}

/// The log-based ordering relations between activities.
#[derive(Debug, Clone)]
pub struct FootprintMatrix {
    pub activities: Vec<String>,
    follows: HashMap<String, HashMap<String, usize>>,
    pub start_set: HashSet<String>,
    pub end_set: HashSet<String>,
}

impl FootprintMatrix {
    /// Builds a footprint matrix from an event log.
    pub fn new(log: &EventLog) -> Self {
        let activities = log.activities();
        let mut follows: HashMap<String, HashMap<String, usize>> = HashMap::new();
        for a in &activities {
            follows.insert(a.clone(), HashMap::new());
        }

        let mut start_set = HashSet::new();
        let mut end_set = HashSet::new();

        for trace in log.traces() {
            if trace.events.is_empty() {
                continue;
            }
            start_set.insert(trace.events[0].activity.clone());
            end_set.insert(trace.events[trace.events.len() - 1].activity.clone());

            for w in trace.events.windows(2) {
                let a = &w[0].activity;
                let b = &w[1].activity;
                *follows.entry(a.clone()).or_default().entry(b.clone()).or_insert(0) += 1;
            }
        }

        Self {
            activities,
            follows,
            start_set,
            end_set,
        }
    }

    /// True if `a` is directly followed by `b` at least once.
    pub fn directly_follows(&self, a: &str, b: &str) -> bool {
        self.directly_follows_count(a, b) > 0
    }

    pub fn directly_follows_count(&self, a: &str, b: &str) -> usize {
        self.follows.get(a).and_then(|m| m.get(b)).copied().unwrap_or(0)
    }

    pub fn relation(&self, a: &str, b: &str) -> Relation {
        let ab = self.directly_follows(a, b);
        let ba = self.directly_follows(b, a);
        match (ab, ba) {
            (true, true) => Relation::Parallel,
            (true, false) => Relation::Causality,
            (false, true) => Relation::ReverseCausality,
            (false, false) => Relation::Choice,
        }
    }

    /// `a -> b`: `a` causes `b`.
    pub fn is_causal(&self, a: &str, b: &str) -> bool {
        self.directly_follows(a, b) && !self.directly_follows(b, a)
    }

    /// `a || b`: activities can occur in either order.
    pub fn is_parallel(&self, a: &str, b: &str) -> bool {
        self.directly_follows(a, b) && self.directly_follows(b, a)
    }

    /// `a # b`: exclusive choice.
    pub fn is_choice(&self, a: &str, b: &str) -> bool {
        !self.directly_follows(a, b) && !self.directly_follows(b, a)
    }

    pub fn successors(&self, a: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .follows
            .get(a)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    pub fn predecessors(&self, b: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .follows
            .iter()
            .filter(|(_, m)| m.get(b).copied().unwrap_or(0) > 0)
            .map(|(a, _)| a.clone())
            .collect();
        out.sort();
        out
    }

    pub fn causal_successors(&self, a: &str) -> Vec<String> {
        self.successors(a).into_iter().filter(|b| self.is_causal(a, b)).collect()
    }

    pub fn causal_predecessors(&self, b: &str) -> Vec<String> {
        self.predecessors(b)
            .into_iter()
            .filter(|a| self.is_causal(a, b))
            .collect()
    }

    pub fn start_activities(&self) -> Vec<String> {
        let mut out: Vec<String> = self.start_set.iter().cloned().collect();
        out.sort();
        out
    }

    pub fn end_activities(&self) -> Vec<String> {
        let mut out: Vec<String> = self.end_set.iter().cloned().collect();
        out.sort();
        out
    }

    /// True when every pair of activities in the set is in choice relation —
    /// used by the Alpha miner to verify candidate place sets.
    pub fn set_is_unrelated(&self, activities: &[String]) -> bool {
        for i in 0..activities.len() {
            for j in (i + 1)..activities.len() {
                if !self.is_choice(&activities[i], &activities[j]) {
                    return false;
                }
            }
        }
        true
    }

    /// True when every activity in `set_a` causally precedes every activity in `set_b`.
    pub fn sets_causally_connected(&self, set_a: &[String], set_b: &[String]) -> bool {
        set_a.iter().all(|a| set_b.iter().all(|b| self.is_causal(a, b)))
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
    fn sequential_activities_are_causal() {
        let log = log_from_traces(&[&["a", "b", "c"]]);
        let fp = FootprintMatrix::new(&log);
        assert!(fp.is_causal("a", "b"));
        assert!(fp.is_causal("b", "c"));
        assert!(!fp.is_causal("c", "a"));
        assert_eq!(fp.relation("a", "b"), Relation::Causality);
    }

    #[test]
    fn parallel_activities_seen_both_orders() {
        let log = log_from_traces(&[&["a", "b"], &["b", "a"]]);
        let fp = FootprintMatrix::new(&log);
        assert!(fp.is_parallel("a", "b"));
        assert_eq!(fp.relation("a", "b"), Relation::Parallel);
    }

    #[test]
    fn never_adjacent_activities_are_in_choice() {
        let log = log_from_traces(&[&["a", "c"], &["b", "c"]]);
        let fp = FootprintMatrix::new(&log);
        assert!(fp.is_choice("a", "b"));
        assert_eq!(fp.relation("a", "b"), Relation::Choice);
    }

    #[test]
    fn start_and_end_activities() {
        let log = log_from_traces(&[&["a", "b", "c"], &["a", "c"]]);
        let fp = FootprintMatrix::new(&log);
        assert_eq!(fp.start_activities(), vec!["a".to_string()]);
        assert_eq!(fp.end_activities(), vec!["c".to_string()]);
    }

    #[test]
    fn set_is_unrelated_requires_pairwise_choice() {
        let log = log_from_traces(&[&["a", "c"], &["b", "c"]]);
        let fp = FootprintMatrix::new(&log);
        assert!(fp.set_is_unrelated(&["a".into(), "b".into()]));
        assert!(!fp.set_is_unrelated(&["a".into(), "c".into()]));
    }

    #[test]
    fn sets_causally_connected_requires_every_pair() {
        let log = log_from_traces(&[&["a", "c"], &["b", "c"]]);
        let fp = FootprintMatrix::new(&log);
        assert!(fp.sets_causally_connected(&["a".into(), "b".into()], &["c".into()]));
        assert!(!fp.sets_causally_connected(&["a".into()], &["b".into()]));
    }

    #[test]
    fn successors_and_predecessors_are_sorted() {
        let log = log_from_traces(&[&["a", "b"], &["a", "c"]]);
        let fp = FootprintMatrix::new(&log);
        assert_eq!(fp.successors("a"), vec!["b".to_string(), "c".to_string()]);
        assert_eq!(fp.predecessors("b"), vec!["a".to_string()]);
    }
}
