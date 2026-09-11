//! Fluent chart construction, adapted from go-pflow's
//! `statemachine/builder.go`.
//!
//! go-pflow stages the builder across four Go types (`ChartBuilder` ->
//! `RegionBuilder` -> `StateBuilder` -> `TransitionBuilder`), each holding a
//! pointer back to its parent so a chain like `.Region("x").State("y")`
//! type-checks. Rust has no ergonomic equivalent of a shared, freely
//! back-pointing builder graph without `Rc<RefCell<_>>`, so this port
//! collapses the four into one [`ChartBuilder`] that tracks "what am I
//! currently building" internally and exposes the same verbs as flat
//! `&mut self -> &mut Self` methods. The call sequences from the Go
//! examples still read the same; only the type names disappear.

use crate::types::{Action, Chart, Guard, Region, State, Transition};

/// Builds a [`Chart`] region by region, state by state, transition by
/// transition.
#[derive(Default)]
pub struct ChartBuilder {
    chart: Chart,
    current_region: Option<String>,
    /// Path of the state currently being built within the current region,
    /// root first (`["dateTime", "holding"]` for a substate).
    current_state_path: Vec<String>,
    current_transition: Option<usize>,
}

impl ChartBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        ChartBuilder {
            chart: Chart {
                name: name.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Starts (or resumes) a region.
    pub fn region(&mut self, name: impl Into<String>) -> &mut Self {
        let name = name.into();
        self.chart.regions.entry(name.clone()).or_insert_with(|| Region {
            name: name.clone(),
            ..Default::default()
        });
        self.current_region = Some(name);
        self.current_state_path.clear();
        self
    }

    fn region_mut(&mut self) -> &mut Region {
        let name = self
            .current_region
            .clone()
            .expect("region() must be called before state()");
        self.chart.regions.get_mut(&name).expect("region exists")
    }

    /// Adds a top-level state to the current region.
    pub fn state(&mut self, name: impl Into<String>) -> &mut Self {
        let name = name.into();
        self.region_mut().states.insert(
            name.clone(),
            State {
                name: name.clone(),
                is_leaf: true,
                ..Default::default()
            },
        );
        self.current_state_path = vec![name];
        self
    }

    /// Adds a substate to the state currently being built.
    pub fn sub(&mut self, name: impl Into<String>) -> &mut Self {
        let name = name.into();
        let path = self.current_state_path.clone();
        let region = self.region_mut();
        if let Some(parent) = state_at_path(region, &path) {
            parent.is_leaf = false;
            parent.children.insert(
                name.clone(),
                State {
                    name: name.clone(),
                    is_leaf: true,
                    ..Default::default()
                },
            );
        }
        self.current_state_path.push(name);
        self
    }

    /// Finishes the current (sub)state, returning to its parent.
    pub fn end(&mut self) -> &mut Self {
        if self.current_state_path.len() > 1 {
            self.current_state_path.pop();
        }
        self
    }

    /// Marks the state currently being built as its parent's initial state.
    pub fn initial(&mut self) -> &mut Self {
        let path = self.current_state_path.clone();
        let is_top_level = path.len() == 1;
        let region = self.region_mut();
        if let Some(s) = state_at_path(region, &path) {
            s.initial = true;
        }
        if is_top_level {
            region.initial = path[0].clone();
        }
        self
    }

    /// Finishes the current region.
    pub fn end_region(&mut self) -> &mut Self {
        self.current_region = None;
        self.current_state_path.clear();
        self
    }

    /// Starts building a transition triggered by `event`.
    pub fn when(&mut self, event: impl Into<String>) -> &mut Self {
        self.chart.transitions.push(Transition {
            event: event.into(),
            source: String::new(),
            target: String::new(),
            guard: None,
            actions: Vec::new(),
        });
        self.current_transition = Some(self.chart.transitions.len() - 1);
        self
    }

    fn transition_mut(&mut self) -> &mut Transition {
        let i = self
            .current_transition
            .expect("when() must be called before in_()/go_to()/do_()/if_guard()");
        &mut self.chart.transitions[i]
    }

    /// Sets the source state path for the transition being built.
    pub fn in_(&mut self, source_path: impl Into<String>) -> &mut Self {
        self.transition_mut().source = source_path.into();
        self
    }

    /// Sets the target state path for the transition being built.
    pub fn go_to(&mut self, target_path: impl Into<String>) -> &mut Self {
        self.transition_mut().target = target_path.into();
        self
    }

    /// Adds an action to the transition being built.
    pub fn do_action(&mut self, action: Box<dyn Action>) -> &mut Self {
        self.transition_mut().actions.push(action);
        self
    }

    /// Adds a guard condition to the transition being built.
    pub fn if_guard(&mut self, guard: Guard) -> &mut Self {
        self.transition_mut().guard = Some(guard);
        self
    }

    /// Consumes the accumulated state and returns the [`Chart`].
    pub fn build(&mut self) -> Chart {
        self.current_region = None;
        self.current_state_path.clear();
        self.current_transition = None;
        std::mem::take(&mut self.chart)
    }
}

fn state_at_path<'a>(region: &'a mut Region, path: &[String]) -> Option<&'a mut State> {
    let mut cur = region.states.get_mut(path.first()?)?;
    for name in &path[1..] {
        cur = cur.children.get_mut(name)?;
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_three_state_light() {
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
            .when("timer")
            .in_("state:green")
            .go_to("state:yellow");
        let chart = b.build();

        assert_eq!(chart.name, "light");
        let region = chart.regions.get("state").unwrap();
        assert_eq!(region.initial, "red");
        assert!(region.states["red"].initial);
        assert!(!region.states["green"].initial);
        assert_eq!(chart.transitions.len(), 2);
        assert_eq!(chart.transitions[0].source, "state:red");
        assert_eq!(chart.transitions[0].target, "state:green");
    }

    #[test]
    fn builds_nested_substates() {
        let mut b = ChartBuilder::new("clock");
        b.region("mode")
            .state("dateTime")
            .sub("default")
            .initial()
            .end()
            .sub("holding")
            .end();
        let chart = b.build();

        let region = &chart.regions["mode"];
        let date_time = &region.states["dateTime"];
        assert!(!date_time.is_leaf);
        assert!(date_time.children["default"].initial);
        assert!(!date_time.children["holding"].initial);
    }
}
