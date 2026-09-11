//! `Bundle`: independently-authored subnets plus the typed `Link`s between
//! them. Ported from go-pflow's `metamodel/compose.go`.
//!
//! The four link kinds are the ones the book specifies (ch04, "Why Types
//! Matter"):
//!
//! - `TokenLink` transfers tokens between schemas — resource coupling
//! - `DataLink` connects places for read-only observation across a boundary
//! - `EventLink` connects transitions — when one fires, the other fires too
//! - `GuardLink` gates a transition in one schema on a place in another
//!
//! `TokenLink` and `DataLink` are place fusion; `EventLink` is transition
//! fusion; `GuardLink` lowers to an inhibitor/read arc or a guard conjunct.

use std::collections::HashMap;

use pflow_metamodel::{Constraint, Model, Place};
use serde::{Deserialize, Serialize};

pub const BUNDLE_CONTEXT: &str = "https://pflow.xyz/schema";
pub const BUNDLE_TYPE: &str = "PetriNetBundle";
pub const SUBNET_TYPE: &str = "PetriNet";

/// Classifies what a subnet models; constrains which links are legal (see
/// [`crate::matrix::link_legal`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NetType {
    /// Back-compatible default: legal with every link kind.
    #[default]
    #[serde(rename = "")]
    Untyped,
    #[serde(rename = "WorkflowNet")]
    Workflow,
    #[serde(rename = "ResourceNet")]
    Resource,
    #[serde(rename = "GameNet")]
    Game,
    #[serde(rename = "ComputationNet")]
    Computation,
    #[serde(rename = "ClassificationNet")]
    Classification,
}

impl std::fmt::Display for NetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NetType::Untyped => "",
            NetType::Workflow => "WorkflowNet",
            NetType::Resource => "ResourceNet",
            NetType::Game => "GameNet",
            NetType::Computation => "ComputationNet",
            NetType::Classification => "ClassificationNet",
        })
    }
}

/// The direction of a [`Port`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortKind {
    In,
    Out,
    Inout,
    /// Read-only; for `DataLink` and `GuardLink`.
    Observe,
}

/// Whether a port exposes a place or a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortTarget {
    #[default]
    Place,
    Transition,
}

/// A named point on a subnet's boundary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Port {
    pub id: String,
    pub kind: Option<PortKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<PortTarget>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub place: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub transition: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub schema: String,
}

impl Port {
    pub fn is_transition(&self) -> bool {
        matches!(self.target, Some(PortTarget::Transition))
    }

    pub fn element(&self) -> &str {
        if self.is_transition() {
            &self.transition
        } else {
            &self.place
        }
    }
}

/// One independently-authored model plus its boundary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Subnet {
    #[serde(default, rename = "@type", skip_serializing_if = "String::is_empty")]
    pub typ: String,
    pub id: String,
    #[serde(default, rename = "net_type", skip_serializing_if = "is_untyped")]
    pub net_type: NetType,
    pub model: Model,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<Port>,
}

fn is_untyped(t: &NetType) -> bool {
    matches!(t, NetType::Untyped)
}

impl Subnet {
    pub fn port_by_id(&self, id: &str) -> Option<&Port> {
        self.ports.iter().find(|p| p.id == id)
    }

    /// Returns the subnet's ports, deriving them from exported places when
    /// none are declared.
    pub fn derived_ports(&self) -> Vec<Port> {
        if !self.ports.is_empty() {
            return self.ports.clone();
        }
        self.model
            .places
            .iter()
            .filter(|p| p.exported)
            .map(|p| Port {
                id: p.id.clone(),
                kind: Some(PortKind::Inout),
                place: p.id.clone(),
                schema: p.typ.clone(),
                ..Default::default()
            })
            .collect()
    }
}

/// One of the book's four typed connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkKind {
    #[default]
    Token,
    Data,
    Event,
    Guard,
}

/// Addresses one side of a link. `port` is the normal form; `place` and
/// `transition` are an escape hatch for addressing an element with no
/// declared port.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Endpoint {
    pub subnet: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub port: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub place: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub transition: String,
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.port.is_empty() {
            write!(f, "{}:{}", self.subnet, self.port)
        } else if !self.place.is_empty() {
            write!(f, "{}/{}", self.subnet, self.place)
        } else if !self.transition.is_empty() {
            write!(f, "{}/{}", self.subnet, self.transition)
        } else {
            write!(f, "{}", self.subnet)
        }
    }
}

pub const LOWERING_AUTO: &str = "auto";
pub const LOWERING_EXPR: &str = "expr";
pub const LOWERING_STRUCTURAL: &str = "structural";
pub const LOWERING_INHIBITOR: &str = "inhibitor";

/// Connects two subnets.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Link {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub kind: LinkKind,
    pub from: Endpoint,
    pub to: Endpoint,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub rename: HashMap<String, String>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub condition: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lowering: String,
}

/// Decides what happens when transition fusion produces two arcs between the
/// same place and transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArcMergePolicy {
    #[default]
    Sum,
    Max,
}

/// A `CompositeNet`: subnets plus the links between them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bundle {
    #[serde(default, rename = "@context", skip_serializing_if = "String::is_empty")]
    pub context: String,
    #[serde(default, rename = "@type", skip_serializing_if = "String::is_empty")]
    pub typ: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    pub subnets: Vec<Subnet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<Constraint>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc_merge: Option<ArcMergePolicy>,
}

impl Bundle {
    pub fn new(name: impl Into<String>) -> Self {
        Bundle {
            context: BUNDLE_CONTEXT.to_string(),
            typ: BUNDLE_TYPE.to_string(),
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn add_subnet(&mut self, mut s: Subnet) -> &mut Self {
        if s.typ.is_empty() {
            s.typ = SUBNET_TYPE.to_string();
        }
        self.subnets.push(s);
        self
    }

    pub fn add_link(&mut self, l: Link) -> &mut Self {
        self.links.push(l);
        self
    }

    pub fn subnet_by_id(&self, id: &str) -> Option<&Subnet> {
        self.subnets.iter().find(|s| s.id == id)
    }

    pub(crate) fn namespaced(&self) -> bool {
        self.namespace.unwrap_or(true)
    }

    pub(crate) fn arc_merge(&self) -> ArcMergePolicy {
        self.arc_merge.unwrap_or_default()
    }

    pub(crate) fn qualified_id(&self, subnet_id: &str, local_id: &str) -> String {
        if !self.namespaced() {
            local_id.to_string()
        } else {
            format!("{subnet_id}/{local_id}")
        }
    }

    pub(crate) fn prefix(&self, subnet_id: &str) -> String {
        if !self.namespaced() {
            String::new()
        } else {
            format!("{subnet_id}/")
        }
    }

    /// Subnets in ID order, so flattening is independent of the order they
    /// were added.
    pub(crate) fn sorted_subnets(&self) -> Vec<&Subnet> {
        let mut out: Vec<&Subnet> = self.subnets.iter().collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
}

/// One rewrite record from flattening. Downstream consumers read this
/// instead of re-deriving structure by parsing ID strings.
#[derive(Debug, Clone, Default)]
pub struct FlattenMap {
    /// subnet ID -> local ID -> flat ID.
    pub place: HashMap<String, HashMap<String, String>>,
    pub transition: HashMap<String, HashMap<String, String>>,
    /// subnet ID -> its namespace prefix.
    pub place_prefix: HashMap<String, String>,
    /// A fused place's flat ID -> the "<subnet>/<place>" members it was
    /// built from. Only fused places appear.
    pub wires: HashMap<String, Vec<String>>,
    /// A fused transition's flat ID -> its "<subnet>/<transition>" members.
    pub fused_groups: HashMap<String, Vec<String>>,
    /// A fused transition's flat ID -> the event ID each member still
    /// emits.
    pub member_events: HashMap<String, Vec<String>>,
    pub warnings: Vec<ValidationError>,
}

/// Validation codes. `E_` prefixed entries are errors; `W_` entries are
/// warnings.
pub const ERR_DUPLICATE_SUBNET: &str = "E_DUPLICATE_SUBNET";
pub const ERR_DUPLICATE_PORT: &str = "E_DUPLICATE_PORT";
pub const ERR_NO_MODEL: &str = "E_SUBNET_NO_MODEL";
pub const ERR_PORT_NOT_EXPORTED: &str = "E_PORT_NOT_EXPORTED";
pub const ERR_PORT_UNKNOWN_ELEMENT: &str = "E_PORT_UNKNOWN_ELEMENT";
pub const ERR_BAD_ENDPOINT: &str = "E_BAD_ENDPOINT";
pub const ERR_ENDPOINT_KIND: &str = "E_ENDPOINT_KIND";
pub const ERR_SCHEMA_MISMATCH: &str = "E_SCHEMA_MISMATCH";
pub const ERR_ILLEGAL_LINK: &str = "E_ILLEGAL_LINK";
pub const ERR_KIND_MISMATCH: &str = "E_KIND_MISMATCH";
pub const ERR_TYPE_MISMATCH: &str = "E_TYPE_MISMATCH";
pub const ERR_INITIAL_VALUE_CONFLICT: &str = "E_INITIAL_VALUE_CONFLICT";
pub const ERR_DATALINK_CONSUMES: &str = "E_DATALINK_CONSUMES";
pub const ERR_BINDING_CONFLICT: &str = "E_BINDING_CONFLICT";
pub const ERR_DUPLICATE_ID: &str = "E_DUPLICATE_ID";
pub const ERR_EVENT_ID_COLLISION: &str = "E_EVENT_ID_COLLISION";
pub const ERR_MULTIPLE_OBJECTIVES: &str = "E_MULTIPLE_OBJECTIVES";
pub const ERR_BAD_CONDITION: &str = "E_BAD_CONDITION";
pub const ERR_DURATION_CONFLICT: &str = "E_DURATION_CONFLICT";
pub const ERR_PORT_DIRECTION: &str = "E_PORT_DIRECTION";
pub const ERR_UNKNOWN_ARC_TYPE: &str = "E_UNKNOWN_ARC_TYPE";
pub const ERR_READ_ARC_DIRECTION: &str = "E_READ_ARC_DIRECTION";
pub const ERR_KINETIC_MISPLACED: &str = "E_KINETIC_MISPLACED";

pub const WARN_UNTYPED_SUBNET: &str = "W_UNTYPED_SUBNET";
pub const WARN_UNBOUNDED_QUEUE: &str = "W_UNBOUNDED_QUEUE";
pub const WARN_RESTRICTIVE_LINK: &str = "W_RESTRICTIVE_LINK";
pub const WARN_GUARD_OPAQUE: &str = "W_GUARD_OPAQUE";
pub const WARN_ROUTE_DROPPED: &str = "W_ROUTE_DROPPED";
pub const WARN_WORKFLOW_CURSOR: &str = "W_WORKFLOW_MULTI_CURSOR";
pub const WARN_EVENTLINK_CYCLE: &str = "W_EVENTLINK_CYCLE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
    pub element: String,
}

impl ValidationError {
    pub fn new(code: &str, message: impl Into<String>, element: impl Into<String>) -> Self {
        ValidationError {
            code: code.to_string(),
            message: message.into(),
            element: element.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationError>,
}

pub(crate) fn kind_name(p: &Place) -> &'static str {
    if p.is_token() {
        "token"
    } else {
        "data"
    }
}
