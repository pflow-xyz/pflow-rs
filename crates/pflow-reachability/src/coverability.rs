//! Karp–Miller coverability witnesses, ported from go-pflow's
//! `reachability/coverability.go`.

use std::collections::{HashMap, HashSet, VecDeque};

use pflow_metamodel::{ArcType, Model};

use crate::analyzer::Analyzer;
use crate::graph::Graph;
use crate::marking::{self, Marking};

/// A finite proof that a net is unbounded: a firing sequence that reaches a
/// marking strictly covering an earlier marking on the same path. Because the
/// covering marking has at least as many tokens everywhere as its ancestor and
/// strictly more somewhere, the intervening firing sequence is enabled again
/// from the covering marking, and repeating it pumps tokens into `places`
/// without limit (the Karp–Miller criterion).
#[derive(Debug, Clone)]
pub struct UnboundedWitness {
    /// The firing sequence from the initial marking to the ancestor marking.
    pub prefix: Vec<String>,
    /// The firing sequence from the ancestor marking to the marking that
    /// strictly covers it. Repeating it grows `places` without bound.
    pub pump: Vec<String>,
    pub from: Marking,
    pub to: Marking,
    /// Places whose token count strictly increases across `pump`.
    pub places: Vec<String>,
}

impl<'m> Analyzer<'m> {
    /// Searches (breadth-first, so a found witness is shortest) for a proof
    /// that the net is unbounded from the analyzer's initial marking.
    /// Returns `None` if no witness was found within the state limit, which
    /// is *not* a proof of boundedness.
    ///
    /// Inhibitor arcs and capacity-bounded output places break the
    /// monotonicity the pump argument relies on (more tokens can cross an
    /// inhibitor's disable threshold, or hit a place's cap), so a pump
    /// containing a transition gated either way is skipped rather than
    /// reported. Go's `Analyzer.FindUnboundedWitness`.
    pub fn find_unbounded_witness(&self) -> Option<UnboundedWitness> {
        let model = self.model();
        let mut graph = Graph::new(model, self.initial().clone());

        let unsafe_to_repeat = unsafe_pump_transitions(model);

        struct Node {
            marking: Marking,
            path: Vec<Marking>,
            trace: Vec<String>,
        }

        let mut queue: VecDeque<Node> = VecDeque::new();
        queue.push_back(Node { marking: self.initial().clone(), path: Vec::new(), trace: Vec::new() });
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(marking::key(self.initial()));

        while let Some(cur) = queue.pop_front() {
            if visited.len() >= self.max_states() {
                break;
            }

            let key = graph.add_state(&cur.marking);
            let enabled = graph.states[&key].enabled.clone();

            for trans in enabled {
                let Some(next) = graph.fire(&cur.marking, &trans) else {
                    continue;
                };

                let mut new_path = cur.path.clone();
                new_path.push(cur.marking.clone());
                let mut new_trace = cur.trace.clone();
                new_trace.push(trans.clone());

                for (i, ancestor) in new_path.iter().enumerate() {
                    if !marking::strictly_covers(&next, ancestor) {
                        continue;
                    }
                    if pump_has_unsafe(&new_trace[i..], &unsafe_to_repeat) {
                        continue;
                    }
                    return Some(UnboundedWitness {
                        prefix: new_trace[..i].to_vec(),
                        pump: new_trace[i..].to_vec(),
                        from: ancestor.clone(),
                        to: next.clone(),
                        places: marking::growing_places(ancestor, &next),
                    });
                }

                let next_key = marking::key(&next);
                if visited.insert(next_key) {
                    queue.push_back(Node { marking: next, path: new_path, trace: new_trace });
                }
            }
        }

        None
    }
}

/// Transitions unsafe to repeat in a pump: gated by an input inhibitor arc,
/// or producing into a capacity-bounded place.
fn unsafe_pump_transitions(model: &Model) -> HashSet<String> {
    let mut unsafe_set = HashSet::new();
    let capacities: HashMap<&str, i64> =
        model.places.iter().filter(|p| p.is_token()).map(|p| (p.id.as_str(), p.capacity)).collect();

    for arc in &model.arcs {
        if arc.typ == ArcType::Inhibitor {
            if model.transition_by_id(&arc.to).is_some() {
                unsafe_set.insert(arc.to.clone());
            }
            continue;
        }
        if arc.typ != ArcType::Normal {
            continue;
        }
        if model.transition_by_id(&arc.from).is_some() {
            if let Some(&cap) = capacities.get(arc.to.as_str()) {
                if cap > 0 {
                    unsafe_set.insert(arc.from.clone());
                }
            }
        }
    }
    unsafe_set
}

fn pump_has_unsafe(pump: &[String], unsafe_set: &HashSet<String>) -> bool {
    if unsafe_set.is_empty() {
        return false;
    }
    pump.iter().any(|t| unsafe_set.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_metamodel::schema::{Arc, ArcType as AT, Place, Transition};

    #[test]
    fn finds_a_witness_for_a_simple_pump() {
        let model = Model {
            name: "unbounded".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
            ],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let analyzer = Analyzer::new(&model);
        let witness = analyzer.find_unbounded_witness().expect("p1 is unbounded");
        assert_eq!(witness.places, vec!["p1".to_string()]);
        assert_eq!(witness.pump, vec!["t0".to_string()]);
    }

    #[test]
    fn a_bounded_cycle_has_no_witness() {
        let model = Model {
            name: "cycle".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
            ],
            transitions: vec![
                Transition { id: "t0".into(), ..Default::default() },
                Transition { id: "t1".into(), ..Default::default() },
            ],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
                Arc { from: "p1".into(), to: "t1".into(), ..Default::default() },
                Arc { from: "t1".into(), to: "p0".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let analyzer = Analyzer::new(&model);
        assert!(analyzer.find_unbounded_witness().is_none());
    }

    #[test]
    fn inhibited_pump_transition_is_skipped() {
        // t0 pumps p1 upward, but is inhibited once "guard" (which t0 also
        // sets) reaches 1 — so the pump is not actually repeatable forever
        // and must not be reported as a witness.
        let model = Model {
            name: "guarded".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, capacity: 0, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
                Place { id: "guard".into(), ..Default::default() },
            ],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "guard".into(), to: "t0".into(), typ: AT::Inhibitor, weight: 1, ..Default::default() },
                Arc { from: "t0".into(), to: "p0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "guard".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let analyzer = Analyzer::new(&model);
        // t0 fires exactly once (guard blocks the second firing), so p1
        // cannot actually grow without bound — no witness should be found.
        assert!(analyzer.find_unbounded_witness().is_none());
    }
}
