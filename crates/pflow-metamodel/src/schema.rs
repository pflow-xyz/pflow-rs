//! Shape B `Model`: the application schema, ported from go-pflow's
//! `metamodel/schema.go`, `access.go`, `presentation.go` and `parameters.go`
//! type declarations.
//!
//! Field names and `serde` renames mirror the Go struct tags exactly, field
//! for field, so a JSON document produced by go-pflow deserializes here
//! without loss and a document produced here round-trips through Go. Unknown
//! fields are tolerated by the library (`#[serde(default)]` throughout, no
//! `deny_unknown_fields` here) and rejected only in the crate's own tests, on
//! the same "library tolerates, tests are strict" rule the DSL and tokenmodel
//! crates already follow.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::schedule::RateSegment;

fn is_false(b: &bool) -> bool {
    !b
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

/// Discriminates between token-counting and data-holding places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StateKind {
    /// Holds an integer count (classic Petri net semantics). Also the
    /// default when `kind` is absent from JSON — see [`Place::is_token`].
    #[default]
    Token,
    /// Holds structured data (maps, structs).
    Data,
}

/// A place: a state or resource in the model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    #[serde(default)]
    pub initial: i64,

    /// Absent (`None`) reads as [`StateKind::Token`] — see [`Place::is_token`].
    /// `Option` rather than a defaulted enum so the wire format matches Go's
    /// `omitempty` on a string-backed type exactly: nothing is written when
    /// unset, and unset is distinguishable from an explicit `"token"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<StateKind>,

    #[serde(default, skip_serializing_if = "String::is_empty", rename = "type")]
    pub typ: String,

    #[serde(default, skip_serializing_if = "is_false")]
    pub exported: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub persisted: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_value: Option<serde_json::Value>,

    /// A POST-FIRING bound, not a cap on the marking: a transition is
    /// disabled when firing it would leave this place above `capacity`,
    /// netting out what the same firing consumes from it. Zero (the
    /// default) means unbounded. See [`crate::firing`].
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub capacity: i64,

    #[serde(default, skip_serializing_if = "is_false")]
    pub resource: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,

    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub x: i64,

    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub y: i64,
}

impl Place {
    /// True for a token-counting place. Absent `kind` reads as token, the
    /// same default go-pflow's `Place.IsToken` applies.
    pub fn is_token(&self) -> bool {
        matches!(self.kind, None | Some(StateKind::Token))
    }

    /// True for a data-holding place.
    pub fn is_data(&self) -> bool {
        matches!(self.kind, Some(StateKind::Data))
    }
}

/// A modeller's claim that several elements should be treated as one
/// parameter, with the reason recorded alongside it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssertedClass {
    pub id: String,
    pub members: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// One declared structural decision variable: an arc weight or a place
/// capacity. Exactly one of `arcs`/`capacity` is set. See go-pflow's
/// `metamodel/parameters.go` for the full contract.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arcs: Vec<ParameterArc>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub capacity: String,

    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub min: i64,

    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub max: i64,
}

/// Names an arc by its endpoints, the way the model's arc list does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterArc {
    pub from: String,
    pub to: String,
}

/// One screen of the application a model describes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewDecl {
    pub id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prompt: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub places: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
}

/// Configures ODE-based simulation for move evaluation and AI.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Simulation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub objective: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players: Option<HashMap<String, Player>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solver: Option<SolverConfig>,
}

/// An agent in the simulation (for games, optimization).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub maximizes: bool,

    #[serde(
        default,
        skip_serializing_if = "String::is_empty",
        rename = "turnPlace"
    )]
    pub turn_place: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<String>,
}

/// ODE solver parameters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SolverConfig {
    /// `[start, end]`. Go's `omitempty` on a fixed-size array is a no-op
    /// (length never reads as zero), so this always serializes.
    #[serde(default)]
    pub tspan: [f64; 2],

    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub dt: f64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rates: Option<HashMap<String, f64>>,
}

/// The model's machine-readable theming. See go-pflow's
/// `metamodel/presentation.go` for the full rationale: presentation only
/// ever overrides a derived default, never supplies one a console cannot
/// derive on its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Presentation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub accent: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<HashMap<String, String>>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<ControlGroup>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disruptions: Vec<Disruption>,
}

/// Captions a set of controls that belong together.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ControlGroup {
    pub id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    pub members: Vec<String>,
}

/// A named scenario fragment: a marking, rate and/or schedule override
/// merged into whatever the operator has already dialled in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Disruption {
    pub id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marking: Option<HashMap<String, i64>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rates: Option<HashMap<String, f64>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<HashMap<String, Vec<RateSegment>>>,
}

/// A named role for access control. Not embedded in [`Model`] — go-pflow
/// keeps roles and access rules in the extension system (Phase 3's
/// `templates`/`derive`); this is the standalone type both sides share.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Role {
    pub id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherits: Vec<String>,

    #[serde(
        default,
        skip_serializing_if = "String::is_empty",
        rename = "dynamicGrant"
    )]
    pub dynamic_grant: String,
}

/// Who can execute a transition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AccessRule {
    pub transition: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guard: String,
}

/// Operational data needed for state computation on a transition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub name: String,
    #[serde(rename = "type")]
    pub typ: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub value: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub place: String,
}

/// A user input field for a transition's action form.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TransitionField {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty", rename = "type")]
    pub typ: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default: String,
    #[serde(default, skip_serializing_if = "String::is_empty", rename = "autoFill")]
    pub auto_fill: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<FieldOption>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// An option for a select-type transition field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FieldOption {
    pub value: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

/// An action/event in the model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guard: String,

    /// Marks a transition whose source carried a precondition that could
    /// not be written down in `guard`. The net is then an
    /// over-approximation of the source: it fires whenever its inputs
    /// allow, which may be more often than the source permits.
    #[serde(
        default,
        skip_serializing_if = "is_false",
        rename = "guardUnrepresentable"
    )]
    pub guard_unrepresentable: bool,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<Binding>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emits: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<TransitionField>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub http_method: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub http_path: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub duration: String,

    #[serde(
        default,
        skip_serializing_if = "String::is_empty",
        rename = "minDuration"
    )]
    pub min_duration: String,

    #[serde(
        default,
        skip_serializing_if = "String::is_empty",
        rename = "maxDuration"
    )]
    pub max_duration: String,

    /// Firing rate for ODE simulation.
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub rate: f64,

    /// Piecewise-constant rate over model time. See
    /// [`crate::schedule::RateSegment`]. Set only from a schedule the
    /// model declares.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedule: Vec<RateSegment>,

    /// Phase-type (Erlang-k) duration. 0 and 1 both mean plain exponential.
    /// See [`crate::stages::expand_stages`].
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub stages: i64,

    /// A deterministic firing duration, in model time units. Ignored by
    /// [`crate::firing`], which has no notion of a firing instant beyond
    /// this model's discrete rule; only an engine with one can honour it.
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub delay: f64,

    #[serde(default, skip_serializing_if = "is_false", rename = "clearsHistory")]
    pub clears_history: bool,

    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub x: i64,

    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub y: i64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event_type: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_bindings: Option<HashMap<String, String>>,
}

/// Discriminates between normal, inhibitor and read arcs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArcType {
    /// Consumes tokens from input places and produces tokens to output
    /// places.
    #[default]
    #[serde(rename = "")]
    Normal,
    /// Prevents firing while the source place holds at least `weight`
    /// tokens. Nothing is consumed or produced.
    Inhibitor,
    /// Permits firing only while the source place holds at least `weight`
    /// tokens, and consumes nothing. Place -> transition only.
    Read,
}

/// A flow between a place and a transition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Arc {
    pub from: String,
    pub to: String,

    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub weight: i64,

    #[serde(default, skip_serializing_if = "is_normal_arc")]
    #[serde(rename = "type")]
    pub typ: ArcType,

    /// Whether this input arc's place scales the transition's firing rate.
    /// `None` (absent from JSON) means kinetic — see [`Arc::is_kinetic`].
    /// The asymmetry against a plain `bool` is load-bearing: every model
    /// written before this field existed must keep meaning "kinetic" when
    /// re-read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinetic: Option<bool>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
}

fn is_normal_arc(t: &ArcType) -> bool {
    matches!(t, ArcType::Normal)
}

impl std::fmt::Display for ArcType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ArcType::Normal => "",
            ArcType::Inhibitor => "inhibitor",
            ArcType::Read => "read",
        })
    }
}

impl Arc {
    pub fn is_inhibitor(&self) -> bool {
        self.typ == ArcType::Inhibitor
    }

    pub fn is_read(&self) -> bool {
        self.typ == ArcType::Read
    }

    /// True if this arc only tests the marking and moves no tokens.
    pub fn is_read_only(&self) -> bool {
        self.is_inhibitor() || self.is_read()
    }

    /// Whether this arc's place scales the firing rate. An unset flag
    /// reads as true, so a model that never heard of kinetics keeps the
    /// mass-action law it was written under.
    pub fn is_kinetic(&self) -> bool {
        self.kinetic.unwrap_or(true)
    }

    /// The arc's effective weight: an unset (zero) weight defaults to 1,
    /// matching the firing rule.
    pub fn effective_weight(&self) -> i64 {
        if self.weight == 0 {
            1
        } else {
            self.weight
        }
    }
}

/// An invariant on the model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Constraint {
    pub id: String,
    pub expr: String,
}

/// The data contract for transitions (Events First schema).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    pub fields: Vec<EventField>,
}

/// A typed field within an [`Event`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventField {
    pub name: String,
    #[serde(rename = "type")]
    pub typ: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub of: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// A core Petri net model plus the application-level constructs that ride
/// along with it. Ported field-for-field from go-pflow's `metamodel.Model`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub name: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    pub places: Vec<Place>,
    pub transitions: Vec<Transition>,
    pub arcs: Vec<Arc>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<Constraint>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,

    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub decimals: i64,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub view: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<ViewDecl>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation: Option<Simulation>,

    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        rename = "assertedClasses"
    )]
    pub asserted_classes: Vec<AssertedClass>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<Parameter>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<Presentation>,
}

impl Model {
    /// Returns the named place, or `None`.
    pub fn place_by_id(&self, id: &str) -> Option<&Place> {
        self.places.iter().find(|p| p.id == id)
    }

    /// Returns the named place mutably, or `None`.
    pub fn place_by_id_mut(&mut self, id: &str) -> Option<&mut Place> {
        self.places.iter_mut().find(|p| p.id == id)
    }

    /// Returns the named transition, or `None`.
    pub fn transition_by_id(&self, id: &str) -> Option<&Transition> {
        self.transitions.iter().find(|t| t.id == id)
    }

    /// Returns the named transition mutably, or `None`.
    pub fn transition_by_id_mut(&mut self, id: &str) -> Option<&mut Transition> {
        self.transitions.iter_mut().find(|t| t.id == id)
    }

    /// Returns the named parameter, or `None`.
    pub fn parameter_by_id(&self, id: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.id == id)
    }

    /// A token place named by `id`; `None` if it does not exist or is a
    /// data place. Data places take no part in the firing rule.
    pub(crate) fn token_place(&self, id: &str) -> Option<&Place> {
        self.place_by_id(id).filter(|p| p.is_token())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_defaults_serialize_to_the_bare_minimum() {
        let a = Arc {
            from: "p".into(),
            to: "t".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, r#"{"from":"p","to":"t"}"#);
    }

    #[test]
    fn arc_kinetic_round_trips_explicit_true_and_false() {
        for (kinetic, want_json) in [
            (false, r#"{"from":"p","to":"t","kinetic":false}"#),
            (true, r#"{"from":"p","to":"t","kinetic":true}"#),
        ] {
            let a = Arc {
                from: "p".into(),
                to: "t".into(),
                kinetic: Some(kinetic),
                ..Default::default()
            };
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(json, want_json);
            let back: Arc = serde_json::from_str(&json).unwrap();
            assert_eq!(back.is_kinetic(), kinetic);
        }
    }

    #[test]
    fn arc_absent_kinetic_defaults_to_true() {
        let a: Arc = serde_json::from_str(r#"{"from":"p","to":"t"}"#).unwrap();
        assert!(a.is_kinetic(), "an arc that never heard of kinetics must keep the mass-action law it was written under");
    }

    #[test]
    fn arc_type_serializes_as_go_does() {
        assert_eq!(serde_json::to_string(&ArcType::Normal).unwrap(), r#""""#);
        assert_eq!(serde_json::to_string(&ArcType::Read).unwrap(), r#""read""#);
        assert_eq!(
            serde_json::to_string(&ArcType::Inhibitor).unwrap(),
            r#""inhibitor""#
        );

        let read = Arc {
            from: "p".into(),
            to: "t".into(),
            typ: ArcType::Read,
            ..Default::default()
        };
        let json = serde_json::to_string(&read).unwrap();
        assert!(json.contains(r#""type":"read""#), "{json}");
        // The default arc type omits the field entirely.
        let normal = Arc {
            from: "p".into(),
            to: "t".into(),
            ..Default::default()
        };
        assert!(!serde_json::to_string(&normal).unwrap().contains("\"type\""));
    }

    #[test]
    fn place_absent_kind_reads_as_token() {
        let p: Place = serde_json::from_str(r#"{"id":"p"}"#).unwrap();
        assert!(p.is_token());
        assert!(!p.is_data());
    }

    #[test]
    fn model_round_trips_through_json() {
        let m = Model {
            name: "cafe".into(),
            version: "v1".into(),
            places: vec![Place {
                id: "cups".into(),
                initial: 3,
                ..Default::default()
            }],
            transitions: vec![Transition {
                id: "brew".into(),
                rate: 2.0,
                ..Default::default()
            }],
            arcs: vec![Arc {
                from: "cups".into(),
                to: "brew".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn library_tolerates_unknown_fields() {
        // The library itself never rejects an unknown field — only this
        // crate's own tests, elsewhere, hold a strict reader to that
        // contract with `deny_unknown_fields` on a wrapper type. Here we
        // assert the *library* type does not choke on one, matching
        // go-pflow's "tolerate at the library, reject only in test
        // fixtures" rule.
        let json = r#"{"id":"p","initial":1,"fromTheFuture":true}"#;
        let p: Place = serde_json::from_str(json).unwrap();
        assert_eq!(p.initial, 1);
    }

    #[test]
    fn asserted_classes_uses_go_camel_case_key() {
        let m = Model {
            name: "m".into(),
            asserted_classes: vec![AssertedClass {
                id: "a".into(),
                members: vec!["p".into()],
                note: String::new(),
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"assertedClasses\""), "{json}");
    }

    #[test]
    fn transition_camel_case_fields_match_go() {
        let t = Transition {
            id: "t".into(),
            guard_unrepresentable: true,
            min_duration: "1s".into(),
            max_duration: "2s".into(),
            clears_history: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"guardUnrepresentable\":true"), "{json}");
        assert!(json.contains("\"minDuration\":\"1s\""), "{json}");
        assert!(json.contains("\"maxDuration\":\"2s\""), "{json}");
        assert!(json.contains("\"clearsHistory\":true"), "{json}");
    }
}

/// Strict readers used only by this crate's own tests, never by the
/// library: unknown fields are a hard error here, matching the ground
/// rule that a golden or hand-authored fixture failing to parse means the
/// fixture (or this crate) has drifted, not that tolerance should be
/// added to the library type.
#[cfg(test)]
pub(crate) mod strict {
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Place {
        #[allow(dead_code)]
        pub id: String,
        #[serde(default)]
        #[allow(dead_code)]
        pub initial: i64,
    }
}

#[cfg(test)]
mod strict_tests {
    use super::strict::Place as StrictPlace;

    #[test]
    fn strict_reader_rejects_unknown_fields() {
        let json = r#"{"id":"p","initial":1,"fromTheFuture":true}"#;
        let err = serde_json::from_str::<StrictPlace>(json).unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }
}
