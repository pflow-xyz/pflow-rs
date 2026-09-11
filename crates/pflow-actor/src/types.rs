//! Actor-system types, ported from go-pflow's `actor/types.go` — narrowed
//! to what the [`crate::bus`] and [`crate::metabundle`] surfaces need (the
//! scope named for this crate: "bus, subnets, metabundle"). go-pflow's
//! `Behavior` can also wrap a raw `petri.PetriNet` or a `workflow.Workflow`
//! and dispatch signals through it (`actor.go`'s `dispatch`); that
//! execution engine is not ported here — see the crate doc.

use std::collections::HashMap;

/// A message on the bus.
#[derive(Debug, Clone, Default)]
pub struct Signal {
    pub id: String,
    pub typ: String,
    pub source: String,
    /// Specific target actor id, or empty for broadcast.
    pub target: String,
    pub payload: HashMap<String, String>,
    pub correlation_id: String,
    pub reply_to: String,
}

/// Context passed to a [`SignalHandler`].
pub struct ActorContext<'a> {
    pub actor_id: &'a str,
    pub signal: &'a Signal,
    pub state: &'a mut HashMap<String, String>,
}

/// Processes an incoming signal.
pub type SignalHandler = Box<dyn FnMut(&mut ActorContext<'_>, &Signal) -> Result<(), String>>;

/// A condition evaluated against a signal (a bus filter, a trigger
/// condition, or a behaviour guard).
pub type SignalPredicate = Box<dyn Fn(&Signal) -> bool>;

/// One actor's subscription to a signal type.
pub struct Subscription {
    pub actor_id: String,
    pub signal_type: String,
    pub priority: i32,
    pub(crate) handler: SignalHandler,
    pub(crate) filter: Option<SignalPredicate>,
}

/// Defines how a signal activates a [`Behavior`] — which transition would
/// fire and any condition gating it. Ported only far enough to support
/// [`crate::metabundle`]'s structural emission (go-pflow's full `Trigger`
/// also carries a `TokenMap` for driving an internal Petri net, which this
/// crate does not execute).
pub struct Trigger {
    pub signal_type: String,
    /// A Go closure in go-pflow (`func(*ActorContext, *Signal) bool`), so it
    /// cannot cross into `pflow_metamodel::Transition::guard`'s string
    /// field either — see `metabundle.rs`'s module doc.
    pub condition: Option<SignalPredicate>,
}

/// Declares that a [`Behavior`] emits `signal_type` when it fires.
pub struct Emitter {
    pub signal_type: String,
}

/// A named unit of signal-handling behavior on an [`Actor`]. go-pflow's
/// `Behavior` additionally wraps a Petri net/workflow model this crate does
/// not execute — see the crate doc.
#[derive(Default)]
pub struct Behavior {
    pub id: String,
    pub name: String,
    pub triggers: HashMap<String, Trigger>,
    pub emitters: Vec<Emitter>,
    /// Same Go-closure-can't-cross-into-a-string-guard story as
    /// `Trigger::condition`.
    pub guard: Option<SignalPredicate>,
}

/// An autonomous agent: bus subscriptions plus named [`Behavior`]s.
#[derive(Default)]
pub struct Actor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub behaviors: HashMap<String, Behavior>,
    pub(crate) state: HashMap<String, String>,
}

impl Actor {
    pub fn new(id: impl Into<String>) -> Self {
        Actor {
            id: id.into(),
            ..Default::default()
        }
    }
}

pub(crate) fn sorted_keys<V>(m: &HashMap<String, V>) -> Vec<String> {
    let mut out: Vec<String> = m.keys().cloned().collect();
    out.sort();
    out
}
