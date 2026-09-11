//! Event-driven statechart execution, ported from go-pflow's
//! `statemachine/machine.go`.
//!
//! go-pflow's `Machine` drives a *separate* Shape-A Petri net
//! (`Chart.ToPetriNet`, via the standalone `engine` package) with hand-rolled
//! state manipulation that bypasses the firing rule entirely — `SendEvent`
//! checks `state[sourcePlaceName] < 0.5` directly rather than asking the net
//! whether the transition is enabled, and `fireTransition` writes the next
//! marking by hand rather than calling `Fire`. Per ROADMAP.md ground rule 4
//! ("the firing rule has one home... every engine... calls it"), this port
//! instead drives [`crate::types::Chart::to_meta_model`]'s compiled
//! `pflow_metamodel::Model` through [`pflow_metamodel::Model::enabled`] /
//! [`pflow_metamodel::Model::fire`] directly. For a chart whose transitions
//! only ever reference top-level state paths (`"region:state"`, the common
//! case, and the only shape the showcase and go-pflow's own tests use), the
//! observable behavior is identical: a consuming arc off the source place
//! and a producing arc onto the target place is exactly what
//! `state[source] = 0; state[target] = 1` computes, and a self-transition
//! (`source == target`) nets to no change either way.
//!
//! **One gap, inherited from `ToMetaModel` itself, not introduced here**:
//! go-pflow's `ToPetriNet`/`Machine.fireTransition` has extra logic
//! (`addHierarchyArcs`) that also moves a token on the *parent* place when a
//! transition changes which top-level state is active while addressing a
//! substate (`"region:stateA:sub"` -> `"region:stateB:sub2"`). `ToMetaModel`
//! (`statemachine/metasubnet.go`, the metamodel path this crate builds on)
//! has no equivalent — go-pflow itself does not carry that hierarchy trick
//! into the metamodel emitter. A chart that transitions between two
//! top-level states while both paths name a substate will therefore leave
//! the two states' own places both marked, rather than moving the parent
//! token. Flagged rather than silently reproduced or silently dropped.

use std::collections::HashMap;

use pflow_metamodel::{Marking, Model};

use crate::types::{path_to_place_name, Chart, StatePath};

struct TransitionMapping {
    /// Index into `chart.transitions`.
    index: usize,
    /// Compiled net transition id (`"<event>_<n>"`).
    txn_id: String,
}

/// Wraps a compiled [`Chart`] and routes events to transitions through the
/// shared firing rule, tracking the active state configuration as a
/// [`Marking`].
pub struct Machine {
    chart: Chart,
    model: Model,
    marking: Marking,
    event_transitions: HashMap<String, Vec<TransitionMapping>>,
}

impl Machine {
    /// Creates a state machine from a chart, compiling it once via
    /// [`Chart::to_meta_model`].
    pub fn new(chart: Chart) -> Self {
        let model = chart.to_meta_model();
        let marking = model.initial_marking();

        let mut event_transitions: HashMap<String, Vec<TransitionMapping>> = HashMap::new();
        for (i, t) in chart.transitions.iter().enumerate() {
            let txn_id = format!("{}_{}", t.event, i + 1);
            event_transitions
                .entry(t.event.clone())
                .or_default()
                .push(TransitionMapping { index: i, txn_id });
        }

        Machine {
            chart,
            model,
            marking,
            event_transitions,
        }
    }

    /// Dispatches an event. Returns `true` if a transition fired, `false`
    /// if no transition for this event was enabled (an unknown event also
    /// returns `false`, same as go-pflow).
    pub fn send_event(&mut self, event: &str) -> bool {
        let Some(mappings) = self.event_transitions.get(event) else {
            return false;
        };

        for mapping in mappings {
            let t = &self.chart.transitions[mapping.index];
            let guard_ok = match &t.guard {
                Some(g) => g(&self.marking),
                None => true,
            };
            if guard_ok && self.model.enabled(&mapping.txn_id, &self.marking) {
                self.marking = self.model.fire(&mapping.txn_id, &self.marking);
                // IncrementAction is already realized by the compiled net's
                // weighted arc (see model.rs); SetAction/CallbackAction have
                // no structural form, so they run here.
                for a in &t.actions {
                    if a.as_any().downcast_ref::<crate::types::IncrementAction>().is_none() {
                        a.apply(&mut self.marking);
                    }
                }
                return true;
            }
        }
        false
    }

    /// The currently active state for `region_name`, or `""` if none is
    /// (region unknown, or somehow no place holds a token).
    pub fn state(&self, region_name: &str) -> String {
        let Some(region) = self.chart.regions.get(region_name) else {
            return String::new();
        };
        for state_name in region.states.keys() {
            let place_name = format!("{region_name}_{state_name}");
            if *self.marking.get(&place_name).unwrap_or(&0) > 0 {
                return state_name.clone();
            }
        }
        String::new()
    }

    /// The currently active substate within `state_name` of `region_name`.
    pub fn substate(&self, region_name: &str, state_name: &str) -> String {
        let Some(region) = self.chart.regions.get(region_name) else {
            return String::new();
        };
        let Some(parent) = region.states.get(state_name) else {
            return String::new();
        };
        for sub_name in parent.children.keys() {
            let place_name = format!("{region_name}_{state_name}_{sub_name}");
            if *self.marking.get(&place_name).unwrap_or(&0) > 0 {
                return sub_name.clone();
            }
        }
        String::new()
    }

    /// `"state"` or `"state:substate"` for a region.
    pub fn full_state(&self, region_name: &str) -> String {
        let state_name = self.state(region_name);
        if state_name.is_empty() {
            return String::new();
        }
        let sub = self.substate(region_name, &state_name);
        if sub.is_empty() {
            state_name
        } else {
            format!("{state_name}:{sub}")
        }
    }

    /// Whether a specific state path is currently active.
    pub fn is_in(&self, path: &str) -> bool {
        let place_name = path_to_place_name(&StatePath(path.to_string()));
        *self.marking.get(&place_name).unwrap_or(&0) > 0
    }

    /// The current value of a counter place.
    pub fn counter(&self, name: &str) -> i64 {
        *self.marking.get(name).unwrap_or(&0)
    }

    pub fn marking(&self) -> &Marking {
        &self.marking
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn chart(&self) -> &Chart {
        &self.chart
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::ChartBuilder;
    use crate::types::increment;

    fn traffic_light() -> Chart {
        let mut b = ChartBuilder::new("light");
        b.region("state")
            .state("red")
            .initial()
            .state("green")
            .state("yellow")
            .end_region()
            .when("timer")
            .in_("state:red")
            .go_to("state:green")
            .do_action(Box::new(increment("cycles")))
            .when("timer")
            .in_("state:green")
            .go_to("state:yellow")
            .when("timer")
            .in_("state:yellow")
            .go_to("state:red");
        b.build()
    }

    #[test]
    fn cycles_through_states_on_event() {
        let mut m = Machine::new(traffic_light());
        assert_eq!(m.state("state"), "red");

        assert!(m.send_event("timer"));
        assert_eq!(m.state("state"), "green");
        assert_eq!(m.counter("cycles"), 1);

        assert!(m.send_event("timer"));
        assert_eq!(m.state("state"), "yellow");

        assert!(m.send_event("timer"));
        assert_eq!(m.state("state"), "red");
        assert_eq!(m.counter("cycles"), 1);
    }

    #[test]
    fn unknown_event_does_not_fire() {
        let mut m = Machine::new(traffic_light());
        assert!(!m.send_event("nope"));
        assert_eq!(m.state("state"), "red");
    }

    #[test]
    fn guard_blocks_transition() {
        let mut b = ChartBuilder::new("gate");
        b.region("state")
            .state("closed")
            .initial()
            .state("open")
            .end_region()
            .when("go")
            .in_("state:closed")
            .go_to("state:open")
            .if_guard(Box::new(|_m| false));
        let mut m = Machine::new(b.build());

        assert!(!m.send_event("go"));
        assert_eq!(m.state("state"), "closed");
    }

    #[test]
    fn is_in_reflects_active_path() {
        let mut m = Machine::new(traffic_light());
        assert!(m.is_in("state:red"));
        assert!(!m.is_in("state:green"));
        m.send_event("timer");
        assert!(m.is_in("state:green"));
    }
}
