//! `Queue`: a composable bounded (or unbounded) buffer subnet, ported from
//! go-pflow's `metamodel/queue.go`.
//!
//! A bounded queue is modelled with the *complementary-place* idiom: `items`
//! and `slots` sum to the capacity, so the bound is a P-invariant that
//! Farkas can derive structurally rather than an inhibitor arc that merely
//! enforces it operationally.

use pflow_metamodel::{Arc as MArc, Constraint, Model, Place, StateKind, Transition};

use crate::bundle::{NetType, Port, PortKind, PortTarget, Subnet, SUBNET_TYPE};

pub const QUEUE_ITEMS: &str = "items";
pub const QUEUE_SLOTS: &str = "slots";
pub const QUEUE_ENQUEUE: &str = "enqueue";
pub const QUEUE_DEQUEUE: &str = "dequeue";

#[derive(Debug, Clone, Default)]
pub struct QueueSpec {
    /// Names the subnet; places and transitions are local to it.
    pub id: String,
    /// Bounds the queue. Zero means unbounded.
    pub capacity: i64,
    /// How many items start queued. Must not exceed `capacity`.
    pub initial: i64,
    pub description: String,
}

/// Builds a queue subnet.
///
/// With `capacity > 0` the queue is bounded and carries a capacity
/// invariant. With `capacity == 0` it is unbounded: `slots` is omitted and
/// `enqueue` becomes a source transition with no input place — a genuine
/// modelling choice, but it does mean the net fails structural boundedness.
pub fn new_queue(spec: QueueSpec) -> Result<Subnet, String> {
    if spec.id.is_empty() {
        return Err("queue: ID is required".to_string());
    }
    if spec.capacity < 0 {
        return Err(format!("queue {:?}: capacity {} is negative", spec.id, spec.capacity));
    }
    if spec.initial < 0 {
        return Err(format!("queue {:?}: initial {} is negative", spec.id, spec.initial));
    }
    if spec.capacity > 0 && spec.initial > spec.capacity {
        return Err(format!(
            "queue {:?}: initial {} exceeds capacity {}",
            spec.id, spec.initial, spec.capacity
        ));
    }

    let bounded = spec.capacity > 0;

    let mut m = Model {
        name: spec.id.clone(),
        description: spec.description.clone(),
        places: vec![Place {
            id: QUEUE_ITEMS.to_string(),
            kind: Some(StateKind::Token),
            initial: spec.initial,
            capacity: spec.capacity,
            exported: true,
            ..Default::default()
        }],
        transitions: vec![
            Transition {
                id: QUEUE_ENQUEUE.to_string(),
                description: "accept one item".to_string(),
                ..Default::default()
            },
            Transition {
                id: QUEUE_DEQUEUE.to_string(),
                description: "release one item".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    if bounded {
        m.places.push(Place {
            id: QUEUE_SLOTS.to_string(),
            kind: Some(StateKind::Token),
            initial: spec.capacity - spec.initial,
            capacity: spec.capacity,
            description: "free capacity; complements items".to_string(),
            ..Default::default()
        });
        m.arcs.extend([
            MArc {
                from: QUEUE_SLOTS.to_string(),
                to: QUEUE_ENQUEUE.to_string(),
                weight: 1,
                ..Default::default()
            },
            MArc {
                from: QUEUE_ENQUEUE.to_string(),
                to: QUEUE_ITEMS.to_string(),
                weight: 1,
                ..Default::default()
            },
            MArc {
                from: QUEUE_ITEMS.to_string(),
                to: QUEUE_DEQUEUE.to_string(),
                weight: 1,
                ..Default::default()
            },
            MArc {
                from: QUEUE_DEQUEUE.to_string(),
                to: QUEUE_SLOTS.to_string(),
                weight: 1,
                ..Default::default()
            },
        ]);
        m.constraints.push(Constraint {
            id: "queue_capacity".to_string(),
            expr: format!(r#"tokens("{QUEUE_ITEMS}") + tokens("{QUEUE_SLOTS}") == {}"#, spec.capacity),
        });
    } else {
        m.arcs.extend([
            MArc {
                from: QUEUE_ENQUEUE.to_string(),
                to: QUEUE_ITEMS.to_string(),
                weight: 1,
                ..Default::default()
            },
            MArc {
                from: QUEUE_ITEMS.to_string(),
                to: QUEUE_DEQUEUE.to_string(),
                weight: 1,
                ..Default::default()
            },
        ]);
    }

    let ports = vec![
        Port {
            id: "in".to_string(),
            kind: Some(PortKind::In),
            place: QUEUE_ITEMS.to_string(),
            schema: "queue:items".to_string(),
            ..Default::default()
        },
        Port {
            id: "out".to_string(),
            kind: Some(PortKind::Out),
            place: QUEUE_ITEMS.to_string(),
            schema: "queue:items".to_string(),
            ..Default::default()
        },
        Port {
            id: "depth".to_string(),
            kind: Some(PortKind::Observe),
            place: QUEUE_ITEMS.to_string(),
            schema: "queue:items".to_string(),
            ..Default::default()
        },
        Port {
            id: QUEUE_ENQUEUE.to_string(),
            kind: Some(PortKind::In),
            target: Some(PortTarget::Transition),
            transition: QUEUE_ENQUEUE.to_string(),
            ..Default::default()
        },
        Port {
            id: QUEUE_DEQUEUE.to_string(),
            kind: Some(PortKind::Out),
            target: Some(PortTarget::Transition),
            transition: QUEUE_DEQUEUE.to_string(),
            ..Default::default()
        },
    ];

    Ok(Subnet {
        typ: SUBNET_TYPE.to_string(),
        id: spec.id,
        net_type: NetType::Resource,
        model: m,
        ports,
    })
}

/// Reports whether a subnet is a queue with no capacity bound. `Validate`
/// uses this to warn.
pub fn is_unbounded_queue(s: &Subnet) -> bool {
    if s.model.place_by_id(QUEUE_ITEMS).is_none() || s.model.transition_by_id(QUEUE_ENQUEUE).is_none() {
        return false;
    }
    s.model.place_by_id(QUEUE_SLOTS).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_queue_carries_capacity_invariant() {
        let s = new_queue(QueueSpec {
            id: "jobs".to_string(),
            capacity: 4,
            initial: 1,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(s.model.places.len(), 2);
        assert_eq!(s.model.constraints.len(), 1);
        assert!(!is_unbounded_queue(&s));
    }

    #[test]
    fn unbounded_queue_has_no_slots_and_is_flagged() {
        let s = new_queue(QueueSpec {
            id: "jobs".to_string(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(s.model.places.len(), 1);
        assert!(is_unbounded_queue(&s));
    }

    #[test]
    fn initial_exceeding_capacity_is_rejected() {
        let err = new_queue(QueueSpec {
            id: "jobs".to_string(),
            capacity: 2,
            initial: 3,
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.contains("exceeds capacity"), "{err}");
    }
}
