//! Hierarchical statechart types, ported field-for-field from go-pflow's
//! `statemachine/types.go`.

use std::collections::HashMap;

use pflow_metamodel::Marking;

/// A state in the state machine. States can be simple (leaf) or composite
/// (containing substates).
#[derive(Debug, Clone, Default)]
pub struct State {
    pub name: String,
    /// Substates, keyed by name. Empty for a leaf state.
    pub children: HashMap<String, State>,
    /// Is this the initial substate of its parent?
    pub initial: bool,
    /// True if `children` is empty.
    pub is_leaf: bool,
}

/// An orthogonal region (parallel component). Each region has its own set
/// of states that evolve independently.
#[derive(Debug, Clone, Default)]
pub struct Region {
    pub name: String,
    pub states: HashMap<String, State>,
    /// Name of the initial state.
    pub initial: String,
}

/// A predicate that must be true for a transition to fire. Evaluated
/// against the machine's current counter marking (see
/// [`pflow_metamodel::Marking`]) alongside the compiled net's own
/// enablement check — go-pflow's `statemachine.Guard` is a Go closure with
/// no expression form, so it cannot cross into `pflow_metamodel::Model`'s
/// string guard field either; see `model.rs`'s module doc.
pub type Guard = Box<dyn Fn(&Marking) -> bool>;

/// A side effect executed during a transition, beyond the structural
/// state-move and counter-increment the compiled net already realizes via
/// the firing rule. Mirrors go-pflow's `statemachine.Action` interface.
pub trait Action {
    /// Applies the action to the marking used for the machine's counter
    /// places (not the state-cursor places, which the firing rule alone
    /// governs).
    fn apply(&self, marking: &mut Marking);
    fn name(&self) -> String;
    /// Lets `model.rs` recognize an [`IncrementAction`] the way go-pflow's
    /// `addMetaTransitions` recognizes it with a type switch (`a.(*IncrementAction)`)
    /// — Rust has no such switch over a plain trait object, so callers that
    /// need the concrete type opt in via `Any`.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Increments a counter place. Structurally represented in the compiled
/// net as a weighted arc (see `model.rs`), so [`IncrementAction::apply`] is
/// a no-op when the machine fires through the compiled model — it exists
/// for API parity and for callers driving a marking directly.
#[derive(Debug, Clone)]
pub struct IncrementAction {
    pub place_name: String,
    pub amount: i64,
}

impl Action for IncrementAction {
    fn apply(&self, marking: &mut Marking) {
        *marking.entry(self.place_name.clone()).or_insert(0) += self.amount;
    }
    fn name(&self) -> String {
        format!("increment:{}", self.place_name)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Increments a counter place by 1.
pub fn increment(place_name: impl Into<String>) -> IncrementAction {
    IncrementAction {
        place_name: place_name.into(),
        amount: 1,
    }
}

/// Increments a counter place by `amount`.
pub fn increment_by(place_name: impl Into<String>, amount: i64) -> IncrementAction {
    IncrementAction {
        place_name: place_name.into(),
        amount,
    }
}

/// Sets a place to a specific value. Has no structural representation in
/// the compiled net (go-pflow's own `ToMetaModel` only lowers
/// `IncrementAction`), so it is applied only when the machine fires
/// directly against a marking.
#[derive(Debug, Clone)]
pub struct SetAction {
    pub place_name: String,
    pub value: i64,
}

impl Action for SetAction {
    fn apply(&self, marking: &mut Marking) {
        marking.insert(self.place_name.clone(), self.value);
    }
    fn name(&self) -> String {
        format!("set:{}", self.place_name)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn set(place_name: impl Into<String>, value: i64) -> SetAction {
    SetAction {
        place_name: place_name.into(),
        value,
    }
}

/// Executes an arbitrary callback. Has no structural representation
/// either, for the same reason as [`SetAction`].
pub struct CallbackAction {
    pub callback_name: String,
    pub callback: Box<dyn Fn(&mut Marking)>,
}

impl Action for CallbackAction {
    fn apply(&self, marking: &mut Marking) {
        (self.callback)(marking);
    }
    fn name(&self) -> String {
        format!("callback:{}", self.callback_name)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn callback(
    name: impl Into<String>,
    f: impl Fn(&mut Marking) + 'static,
) -> CallbackAction {
    CallbackAction {
        callback_name: name.into(),
        callback: Box::new(f),
    }
}

/// A state transition triggered by an event.
pub struct Transition {
    /// Triggering event name.
    pub event: String,
    /// Source state path (e.g. "mode:dateTime:default").
    pub source: String,
    /// Target state path.
    pub target: String,
    /// Optional precondition, evaluated in addition to the compiled net's
    /// own enablement.
    pub guard: Option<Guard>,
    pub actions: Vec<Box<dyn Action>>,
}

/// A complete state chart with multiple regions.
#[derive(Default)]
pub struct Chart {
    pub name: String,
    pub regions: HashMap<String, Region>,
    pub transitions: Vec<Transition>,
}

/// A hierarchical state path like `"mode:dateTime:holding"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatePath(pub String);

impl StatePath {
    pub fn parse(&self) -> Vec<String> {
        self.0
            .split(':')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    pub fn region(&self) -> String {
        self.parse().first().cloned().unwrap_or_default()
    }

    pub fn state(&self) -> String {
        self.parse().get(1).cloned().unwrap_or_default()
    }

    pub fn substate(&self) -> String {
        self.parse().get(2).cloned().unwrap_or_default()
    }
}

impl From<&str> for StatePath {
    fn from(s: &str) -> Self {
        StatePath(s.to_string())
    }
}

impl From<String> for StatePath {
    fn from(s: String) -> Self {
        StatePath(s)
    }
}

/// Converts a state path to the place name used in the compiled net:
/// `"mode:dateTime:default"` -> `"mode_dateTime_default"`. Matches
/// go-pflow's `Chart.pathToPlaceName`.
pub fn path_to_place_name(path: &StatePath) -> String {
    path.parse().join("_")
}

pub(crate) fn sorted_keys<V>(m: &HashMap<String, V>) -> Vec<String> {
    let mut out: Vec<String> = m.keys().cloned().collect();
    out.sort();
    out
}
