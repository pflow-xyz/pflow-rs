//! The reachability graph (state space), ported from go-pflow's
//! `reachability/graph.go`.
//!
//! Unlike Go, enabling and firing are not reimplemented here: they are
//! [`pflow_metamodel::Model::enabled`] and [`Model::fire`] — the one shared
//! firing rule Phase 0 ported into `pflow-metamodel`. Go's `Graph.isEnabled`
//! and `Graph.Fire` were a private copy of the same rule the SSA also used
//! to carry, which is exactly the class of divergence the metamodel crate's
//! doc comment (`firing.rs`) exists to rule out; this crate has no excuse to
//! reintroduce it.
//!
//! States and edges are addressed by the marking's canonical [`key`], not by
//! pointer as in Go, since Rust has no garbage collector to make a graph of
//! self-referential pointers convenient.

use std::collections::HashMap;

use pflow_metamodel::Model;

use crate::marking::{self, Marking};

/// One discovered marking and what is known about it.
#[derive(Debug, Clone)]
pub struct State {
    /// Discovery order, starting at 0 for the initial marking.
    pub id: usize,
    pub marking: Marking,
    pub key: String,
    /// Transitions enabled at this marking, in the model's declaration
    /// order (matches `Model::enabled_transitions`).
    pub enabled: Vec<String>,
    /// Indices into [`Graph::edges`] of edges leaving this state.
    pub successors: Vec<usize>,
    /// Indices into [`Graph::edges`] of edges entering this state.
    pub predecessors: Vec<usize>,
    pub is_initial: bool,
    /// No enabled transition.
    pub is_terminal: bool,
    /// Terminal and not an accepted end state — set by the analyzer, never
    /// by the graph itself (the graph doesn't know what "the initial
    /// marking had tokens" means).
    pub is_deadlock: bool,
    /// Shortest known distance from the initial marking; `-1` until an edge
    /// reaches this state (0 for the initial marking itself).
    pub depth: i64,
}

/// One transition firing from one state to another.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub transition: String,
}

/// The reachability graph under construction (or fully built).
pub struct Graph<'m> {
    pub model: &'m Model,
    pub initial: Marking,
    /// Keyed by [`marking::key`].
    pub states: HashMap<String, State>,
    pub edges: Vec<Edge>,
    pub root_key: Option<String>,
    /// Discovery order, for iteration that must match insertion (Go's
    /// `stateList`).
    state_order: Vec<String>,
}

impl<'m> Graph<'m> {
    pub fn new(model: &'m Model, initial: Marking) -> Self {
        Graph {
            model,
            initial,
            states: HashMap::new(),
            edges: Vec::new(),
            root_key: None,
            state_order: Vec::new(),
        }
    }

    /// States in discovery order. Go's `Graph.StatesList`.
    pub fn states_list(&self) -> Vec<&State> {
        self.state_order.iter().map(|k| &self.states[k]).collect()
    }

    pub fn state_count(&self) -> usize {
        self.states.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Registers `marking` as a state if it isn't already known, computing
    /// its enabled transitions. Returns the (possibly pre-existing) key.
    pub fn add_state(&mut self, marking: &Marking) -> String {
        let key = marking::key(marking);
        if self.states.contains_key(&key) {
            return key;
        }

        let enabled = self.model.enabled_transitions(marking);
        let is_initial = self.states.is_empty();
        let id = self.states.len();
        let state = State {
            id,
            marking: marking.clone(),
            key: key.clone(),
            is_terminal: enabled.is_empty(),
            enabled,
            successors: Vec::new(),
            predecessors: Vec::new(),
            is_initial,
            is_deadlock: false,
            depth: if is_initial { 0 } else { -1 },
        };
        if is_initial {
            self.root_key = Some(key.clone());
        }
        self.states.insert(key.clone(), state);
        self.state_order.push(key.clone());
        key
    }

    pub fn get_state(&self, marking: &Marking) -> Option<&State> {
        self.states.get(&marking::key(marking))
    }

    pub fn get_state_by_key(&self, key: &str) -> Option<&State> {
        self.states.get(key)
    }

    /// Whether `transition` can fire at `marking`. Delegates to
    /// [`Model::enabled`] — see the module doc.
    pub fn is_enabled(&self, marking: &Marking, transition: &str) -> bool {
        self.model.enabled(transition, marking)
    }

    /// Fires `transition` at `marking`, returning the resulting marking, or
    /// `None` if it is not enabled. Delegates to [`Model::fire`].
    pub fn fire(&self, marking: &Marking, transition: &str) -> Option<Marking> {
        if !self.is_enabled(marking, transition) {
            return None;
        }
        Some(self.model.fire(transition, marking))
    }

    /// Adds an edge between two already-added states and updates the
    /// target's depth if this edge offers a shorter path.
    pub fn add_edge(&mut self, from_key: &str, to_key: &str, transition: &str) {
        let edge_idx = self.edges.len();
        self.edges.push(Edge {
            from: from_key.to_string(),
            to: to_key.to_string(),
            transition: transition.to_string(),
        });

        let from_depth = self.states.get(from_key).map(|s| s.depth).unwrap_or(-1);
        if let Some(from) = self.states.get_mut(from_key) {
            from.successors.push(edge_idx);
        }
        if let Some(to) = self.states.get_mut(to_key) {
            to.predecessors.push(edge_idx);
            if from_depth >= 0 && (to.depth < 0 || to.depth > from_depth + 1) {
                to.depth = from_depth + 1;
            }
        }
    }

    /// States with no enabled transition. Go's `Graph.TerminalStates`.
    pub fn terminal_states(&self) -> Vec<&State> {
        self.state_order
            .iter()
            .map(|k| &self.states[k])
            .filter(|s| s.is_terminal)
            .collect()
    }

    /// Terminal states flagged as deadlocks by the analyzer. Go's
    /// `Graph.DeadlockStates`.
    pub fn deadlock_states(&self) -> Vec<&State> {
        self.state_order
            .iter()
            .map(|k| &self.states[k])
            .filter(|s| s.is_deadlock)
            .collect()
    }

    /// The maximum depth reached. Go's `Graph.MaxDepth`.
    pub fn max_depth(&self) -> i64 {
        self.states.values().map(|s| s.depth).max().unwrap_or(0)
    }

    /// The maximum token count reached in each place, across all states.
    /// Go's `Graph.MaxTokens`.
    pub fn max_tokens(&self) -> HashMap<String, i64> {
        let mut out: HashMap<String, i64> = HashMap::new();
        for state in self.states.values() {
            for (place, &tokens) in &state.marking {
                let entry = out.entry(place.clone()).or_insert(0);
                if tokens > *entry {
                    *entry = tokens;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_metamodel::schema::{Arc, Place, Transition};

    /// p0 --t0--> p1 --t1--> p0 (a two-cycle), initial marking [p0:1].
    fn cycle_model() -> Model {
        Model {
            name: "cycle".into(),
            places: vec![
                Place {
                    id: "p0".into(),
                    initial: 1,
                    ..Default::default()
                },
                Place {
                    id: "p1".into(),
                    ..Default::default()
                },
            ],
            transitions: vec![
                Transition {
                    id: "t0".into(),
                    ..Default::default()
                },
                Transition {
                    id: "t1".into(),
                    ..Default::default()
                },
            ],
            arcs: vec![
                Arc {
                    from: "p0".into(),
                    to: "t0".into(),
                    ..Default::default()
                },
                Arc {
                    from: "t0".into(),
                    to: "p1".into(),
                    ..Default::default()
                },
                Arc {
                    from: "p1".into(),
                    to: "t1".into(),
                    ..Default::default()
                },
                Arc {
                    from: "t1".into(),
                    to: "p0".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn builds_two_states_and_two_edges() {
        let model = cycle_model();
        let initial = model.initial_marking();
        let mut graph = Graph::new(&model, initial.clone());

        let k0 = graph.add_state(&initial);
        assert!(graph.states[&k0].is_initial);
        assert_eq!(graph.states[&k0].enabled, vec!["t0".to_string()]);

        let next = graph.fire(&initial, "t0").expect("t0 enabled");
        let k1 = graph.add_state(&next);
        graph.add_edge(&k0, &k1, "t0");

        let back = graph.fire(&next, "t1").expect("t1 enabled");
        assert_eq!(back, initial);
        graph.add_edge(&k1, &k0, "t1");

        assert_eq!(graph.state_count(), 2);
        assert_eq!(graph.edge_count(), 2);
        assert_eq!(graph.states[&k1].depth, 1);
        assert!(graph.terminal_states().is_empty());
    }

    #[test]
    fn fire_on_disabled_transition_is_none() {
        let model = cycle_model();
        let initial = model.initial_marking();
        let graph = Graph::new(&model, initial.clone());
        assert!(graph.fire(&initial, "t1").is_none());
    }
}
