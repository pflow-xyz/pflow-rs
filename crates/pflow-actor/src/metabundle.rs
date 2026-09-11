//! Composable form of [`ActorSystem`] on the metamodel composition layer,
//! ported from go-pflow's `actor/metabundle.go`.
//!
//! go-pflow also ships `actor/subnet.go`'s `ToBundle`, targeting the older
//! `tokenmodel/subnet.Bundle` (place-fusion only, weight-1 arcs, no
//! constraints) — not ported here. `pflow-rs` has no port of
//! `tokenmodel/subnet` at all (see `pflow-compose`'s crate doc: "Use
//! `metamodel.Bundle` for anything that generates code or gets verified"),
//! and this crate's declared scope is "bus, subnets, metabundle" — read as
//! the metamodel-layer subnet construction this module performs on the way
//! to a `Bundle`, not a second bundle format.
//!
//! Bus fan-out is expressed as `EventLink`s: the emitting transition and
//! the handling transition are fused into one rendezvous, so an emitter
//! cannot outrun its subscribers (a synchronous bus, not a buffered one —
//! put a `pflow_compose::new_queue` between two actors for buffering, the
//! same guidance go-pflow's own doc comment gives).
//!
//! **Static-topology caveat, inherited from go-pflow's `ToBundle`/
//! `ToMetaBundle` both**: an emission that happens because handler code
//! calls `ctx.emit(...)` at runtime (this crate does not execute handlers
//! against a compiled net at all — see the crate doc) is invisible here.
//! Only signals declared on a [`Behavior`]'s `emitters` list appear as
//! out-ports and `EventLink`s.

use std::collections::HashMap;

use pflow_compose::bundle::{Bundle, Endpoint, Link, LinkKind, NetType, Port, PortKind, PortTarget, Subnet};
use pflow_compose::normalize_kinds;
use pflow_metamodel::{Arc as MetaArc, Model, Place, StateKind, Transition as MetaTransition};

use crate::bus::Bus;
use crate::types::{sorted_keys, Actor};

/// Manages a group of actors sharing one [`Bus`].
#[derive(Default)]
pub struct ActorSystem {
    pub name: String,
    pub bus: Bus,
    pub actors: HashMap<String, Actor>,
    pub running: bool,
}

impl ActorSystem {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        ActorSystem {
            bus: Bus::new(format!("{name}:default")),
            name,
            ..Default::default()
        }
    }

    pub fn add_actor(&mut self, actor: Actor) -> &mut Self {
        self.actors.insert(actor.id.clone(), actor);
        self
    }

    pub fn start(&mut self) -> &mut Self {
        self.running = true;
        self
    }

    pub fn stop(&mut self) -> &mut Self {
        self.running = false;
        self
    }

    /// Returns the system's topology as a `metamodel.Bundle`, with bus
    /// fan-out expressed as `EventLink`s between emitting and handling
    /// transitions. Purely structural: mutates no system state.
    pub fn to_meta_bundle(&self) -> Bundle {
        let mut b = Bundle::new(self.name.clone());

        for aid in sorted_keys(&self.actors) {
            b.add_subnet(actor_meta_subnet(&self.actors[&aid], self.running, &self.bus));
        }

        let (emitters, subscribers) = signal_topology(&self.actors, &self.bus);
        for sig in sorted_keys(&union(&emitters, &subscribers)) {
            let mut from = emitters.get(&sig).cloned().unwrap_or_default();
            let mut to = subscribers.get(&sig).cloned().unwrap_or_default();
            from.sort();
            to.sort();

            for from_actor in &from {
                for to_actor in &to {
                    if from_actor == to_actor {
                        continue;
                    }
                    let emit_txn = emit_transition_id(self.actors.get(from_actor), &sig);
                    let handle_txn = handle_transition_id(self.actors.get(to_actor), &sig, &self.bus);
                    let (Some(emit_txn), Some(handle_txn)) = (emit_txn, handle_txn) else {
                        continue;
                    };
                    b.add_link(Link {
                        kind: LinkKind::Event,
                        from: Endpoint {
                            subnet: from_actor.clone(),
                            transition: emit_txn,
                            ..Default::default()
                        },
                        to: Endpoint {
                            subnet: to_actor.clone(),
                            transition: handle_txn,
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
            }
        }

        b
    }
}

fn actor_meta_subnet(a: &Actor, running: bool, bus: &Bus) -> Subnet {
    let mut m = Model {
        name: a.id.clone(),
        ..Default::default()
    };

    m.places.push(Place {
        id: "actor:state".into(),
        kind: Some(StateKind::Token),
        initial: if running { 1 } else { 0 },
        description: "liveness marker".into(),
        ..Default::default()
    });

    let (subs, emits) = actor_signals(a, bus);

    let mut ports = Vec::new();
    for sig in sorted_keys(&subs) {
        let place_id = format!("sig:in:{sig}");
        m.places.push(Place {
            id: place_id.clone(),
            kind: Some(StateKind::Token),
            exported: true,
            description: format!("inbound {sig}"),
            ..Default::default()
        });
        ports.push(Port {
            id: format!("in:{sig}"),
            kind: Some(PortKind::In),
            place: place_id,
            schema: format!("signal:{sig}"),
            ..Default::default()
        });
    }
    for sig in sorted_keys(&emits) {
        let place_id = format!("sig:out:{sig}");
        m.places.push(Place {
            id: place_id.clone(),
            kind: Some(StateKind::Token),
            exported: true,
            description: format!("outbound {sig}"),
            ..Default::default()
        });
        ports.push(Port {
            id: format!("out:{sig}"),
            kind: Some(PortKind::Out),
            place: place_id,
            schema: format!("signal:{sig}"),
            ..Default::default()
        });
    }

    let mut covered = std::collections::HashSet::new();
    for bid in sorted_keys(&a.behaviors) {
        let bh = &a.behaviors[&bid];
        for sig in sorted_keys(&bh.triggers) {
            covered.insert(sig.clone());
            let txn_id = format!("handle:{bid}:{sig}");
            let trigger = &bh.triggers[&sig];
            let unrepresentable = bh.guard.is_some() || trigger.condition.is_some();
            let mut description = format!("handle {sig}");
            if unrepresentable {
                description += "; guard not represented: the behaviour's precondition is a Rust \
                    closure, so this net over-approximates it (sound for safety, not for \
                    liveness)";
            }
            m.transitions.push(MetaTransition {
                id: txn_id.clone(),
                description,
                guard_unrepresentable: unrepresentable,
                ..Default::default()
            });
            m.arcs.push(MetaArc { from: format!("sig:in:{sig}"), to: txn_id.clone(), weight: 1, ..Default::default() });
            m.arcs.push(MetaArc { from: txn_id.clone(), to: "actor:state".into(), weight: 1, ..Default::default() });
            for em in &bh.emitters {
                m.arcs.push(MetaArc { from: txn_id.clone(), to: format!("sig:out:{}", em.signal_type), weight: 1, ..Default::default() });
            }
            ports.push(Port {
                id: format!("handle:{sig}"),
                kind: Some(PortKind::In),
                target: Some(PortTarget::Transition),
                transition: txn_id,
                ..Default::default()
            });
        }
    }

    // Bus subscriptions with no behaviour trigger behind them.
    for sig in sorted_keys(&subs) {
        if covered.contains(&sig) {
            continue;
        }
        let txn_id = format!("handle:{}:{sig}", a.id);
        m.transitions.push(MetaTransition {
            id: txn_id.clone(),
            description: format!("handle {sig}"),
            ..Default::default()
        });
        m.arcs.push(MetaArc { from: format!("sig:in:{sig}"), to: txn_id.clone(), weight: 1, ..Default::default() });
        m.arcs.push(MetaArc { from: txn_id.clone(), to: "actor:state".into(), weight: 1, ..Default::default() });
        ports.push(Port {
            id: format!("handle:{sig}"),
            kind: Some(PortKind::In),
            target: Some(PortTarget::Transition),
            transition: txn_id,
            ..Default::default()
        });
    }

    normalize_kinds(&mut m);
    Subnet {
        typ: "PetriNet".into(),
        id: a.id.clone(),
        net_type: NetType::Untyped, // an actor is not one of the five shapes
        model: m,
        ports,
    }
}

/// The signal types an actor subscribes to and emits, from both its
/// declared [`Behavior`]s and its raw bus subscriptions.
fn actor_signals(a: &Actor, bus: &Bus) -> (HashMap<String, bool>, HashMap<String, bool>) {
    let mut subs = HashMap::new();
    let mut emits = HashMap::new();
    for bh in a.behaviors.values() {
        for sig in bh.triggers.keys() {
            subs.insert(sig.clone(), true);
        }
        for em in &bh.emitters {
            emits.insert(em.signal_type.clone(), true);
        }
    }
    for (sig, subscribers) in bus.subscriptions() {
        if subscribers.iter().any(|s| s.actor_id == a.id) {
            subs.insert(sig.clone(), true);
        }
    }
    (subs, emits)
}

fn signal_topology(
    actors: &HashMap<String, Actor>,
    bus: &Bus,
) -> (HashMap<String, Vec<String>>, HashMap<String, Vec<String>>) {
    let mut emitters: HashMap<String, Vec<String>> = HashMap::new();
    let mut subscribers: HashMap<String, Vec<String>> = HashMap::new();
    for a in actors.values() {
        for bh in a.behaviors.values() {
            for sig in bh.triggers.keys() {
                subscribers.entry(sig.clone()).or_default().push(a.id.clone());
            }
            for em in &bh.emitters {
                emitters.entry(em.signal_type.clone()).or_default().push(a.id.clone());
            }
        }
        for (sig, subs) in bus.subscriptions() {
            if subs.iter().any(|s| s.actor_id == a.id) {
                subscribers.entry(sig.clone()).or_default().push(a.id.clone());
            }
        }
    }
    (emitters, subscribers)
}

fn emit_transition_id(a: Option<&Actor>, sig: &str) -> Option<String> {
    let a = a?;
    for bid in sorted_keys(&a.behaviors) {
        let bh = &a.behaviors[&bid];
        if bh.emitters.iter().any(|e| e.signal_type == sig) {
            let triggers = sorted_keys(&bh.triggers);
            let first = triggers.first()?;
            return Some(format!("handle:{bid}:{first}"));
        }
    }
    None
}

fn handle_transition_id(a: Option<&Actor>, sig: &str, bus: &Bus) -> Option<String> {
    let a = a?;
    for bid in sorted_keys(&a.behaviors) {
        if a.behaviors[&bid].triggers.contains_key(sig) {
            return Some(format!("handle:{bid}:{sig}"));
        }
    }
    if let Some(subs) = bus.subscriptions().get(sig) {
        if subs.iter().any(|s| s.actor_id == a.id) {
            return Some(format!("handle:{}:{sig}", a.id));
        }
    }
    None
}

fn union(a: &HashMap<String, Vec<String>>, b: &HashMap<String, Vec<String>>) -> HashMap<String, bool> {
    let mut out = HashMap::new();
    for k in a.keys() {
        out.insert(k.clone(), true);
    }
    for k in b.keys() {
        out.insert(k.clone(), true);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Actor, Behavior, Emitter, Trigger};
    use pflow_compose::bundle::LinkKind;

    fn behavior_handling(id: &str, sig: &str) -> Behavior {
        Behavior {
            id: id.to_string(),
            triggers: HashMap::from([(
                sig.to_string(),
                Trigger { signal_type: sig.to_string(), condition: None },
            )]),
            ..Default::default()
        }
    }

    #[test]
    fn one_subnet_per_actor() {
        let mut sys = ActorSystem::new("sys");
        sys.add_actor(Actor::new("a"));
        sys.add_actor(Actor::new("b"));

        let bundle = sys.to_meta_bundle();
        assert_eq!(bundle.subnets.len(), 2);
        assert!(bundle.subnet_by_id("a").is_some());
        assert!(bundle.subnet_by_id("b").is_some());
    }

    #[test]
    fn emitter_and_subscriber_get_an_event_link() {
        let mut sys = ActorSystem::new("sys");

        let mut producer = Actor::new("producer");
        let mut b = behavior_handling("intake", "start");
        b.emitters.push(Emitter { signal_type: "done".into() });
        producer.behaviors.insert("intake".into(), b);
        sys.add_actor(producer);

        let mut consumer = Actor::new("consumer");
        consumer.behaviors.insert("react".into(), behavior_handling("react", "done"));
        sys.add_actor(consumer);

        let bundle = sys.to_meta_bundle();
        assert_eq!(bundle.links.len(), 1);
        let link = &bundle.links[0];
        assert_eq!(link.kind, LinkKind::Event);
        assert_eq!(link.from.subnet, "producer");
        assert_eq!(link.from.transition, "handle:intake:start");
        assert_eq!(link.to.subnet, "consumer");
        assert_eq!(link.to.transition, "handle:react:done");
    }

    #[test]
    fn an_actor_does_not_link_to_itself() {
        let mut sys = ActorSystem::new("sys");
        let mut a = Actor::new("a");
        let mut b = behavior_handling("loop", "ping");
        b.emitters.push(Emitter { signal_type: "ping".into() });
        a.behaviors.insert("loop".into(), b);
        sys.add_actor(a);

        let bundle = sys.to_meta_bundle();
        assert!(bundle.links.is_empty());
    }

    #[test]
    fn bus_only_subscription_gets_a_handler_transition_with_no_behaviour() {
        let mut sys = ActorSystem::new("sys");
        sys.add_actor(Actor::new("worker"));
        sys.bus.subscribe("worker", "task", Box::new(|_c, _s| Ok(())));

        let bundle = sys.to_meta_bundle();
        let sub = bundle.subnet_by_id("worker").unwrap();
        let txn_ids: Vec<&str> = sub.model.transitions.iter().map(|t| t.id.as_str()).collect();
        assert!(txn_ids.contains(&"handle:worker:task"));
    }

    #[test]
    fn guard_condition_marks_transition_unrepresentable() {
        let mut sys = ActorSystem::new("sys");
        let mut a = Actor::new("a");
        a.behaviors.insert(
            "b".into(),
            Behavior {
                id: "b".into(),
                triggers: HashMap::from([(
                    "go".to_string(),
                    Trigger { signal_type: "go".into(), condition: Some(Box::new(|_s| true)) },
                )]),
                ..Default::default()
            },
        );
        sys.add_actor(a);

        let bundle = sys.to_meta_bundle();
        let sub = bundle.subnet_by_id("a").unwrap();
        let t = sub.model.transitions.iter().find(|t| t.id == "handle:b:go").unwrap();
        assert!(t.guard_unrepresentable);
    }

    #[test]
    fn running_system_sets_the_liveness_marker() {
        let mut sys = ActorSystem::new("sys");
        sys.add_actor(Actor::new("a"));
        sys.start();

        let bundle = sys.to_meta_bundle();
        let sub = bundle.subnet_by_id("a").unwrap();
        assert_eq!(sub.model.place_by_id("actor:state").unwrap().initial, 1);
    }
}
