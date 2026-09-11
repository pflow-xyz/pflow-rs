//! `ResourcePool`: a fixed-capacity acquire/release subnet, ported from the
//! composable form in go-pflow's `metamodel/patterns.go`
//! (`NewResourcePool`) and `patterns_compose.go` (`(*ResourcePool).ToSubnet`).
//!
//! Unlike go-pflow, this builds the [`Subnet`] directly rather than through
//! the generic `PetriNet[R]` pattern machinery in `patterns.go` — the
//! metadata-carrying generic net type has no Rust analogue here and the
//! subnet shape is what composition actually consumes. `StateMachine`,
//! `Workflow` and `EventSourced` (the other three `patterns.go`
//! constructors) are not ported: their behaviour already has a home in
//! `pflow-tokenmodel::Runtime`, and porting the generic `PetriNet[S]`
//! machinery just to re-derive a `Subnet` from it would duplicate that
//! without adding a capability composition needs. Tracked as a gap in
//! ROADMAP.md rather than silently dropped.

use pflow_metamodel::{Arc as MArc, Constraint, Model, Place, StateKind, Transition};

use crate::bundle::{NetType, Port, PortKind, PortTarget, Subnet, SUBNET_TYPE};

pub const POOL_AVAILABLE: &str = "available";
pub const POOL_IN_USE: &str = "in_use";
pub const POOL_ACQUIRE: &str = "acquire";
pub const POOL_RELEASE: &str = "release";

/// Builds a resource pool subnet with the given capacity. The pool's
/// conservation law (`available + in_use == capacity`) travels with it as a
/// [`Constraint`], so it stays provable of anything it composes into.
pub fn new_resource_pool(id: impl Into<String>, capacity: i64) -> Subnet {
    let id = id.into();
    let m = Model {
        name: id.clone(),
        places: vec![
            Place {
                id: POOL_AVAILABLE.to_string(),
                kind: Some(StateKind::Token),
                initial: capacity,
                capacity,
                exported: true,
                ..Default::default()
            },
            Place {
                id: POOL_IN_USE.to_string(),
                kind: Some(StateKind::Token),
                capacity,
                exported: true,
                ..Default::default()
            },
        ],
        transitions: vec![
            Transition {
                id: POOL_ACQUIRE.to_string(),
                ..Default::default()
            },
            Transition {
                id: POOL_RELEASE.to_string(),
                ..Default::default()
            },
        ],
        arcs: vec![
            MArc {
                from: POOL_AVAILABLE.to_string(),
                to: POOL_ACQUIRE.to_string(),
                weight: 1,
                ..Default::default()
            },
            MArc {
                from: POOL_ACQUIRE.to_string(),
                to: POOL_IN_USE.to_string(),
                weight: 1,
                ..Default::default()
            },
            MArc {
                from: POOL_IN_USE.to_string(),
                to: POOL_RELEASE.to_string(),
                weight: 1,
                ..Default::default()
            },
            MArc {
                from: POOL_RELEASE.to_string(),
                to: POOL_AVAILABLE.to_string(),
                weight: 1,
                ..Default::default()
            },
        ],
        constraints: vec![Constraint {
            id: "pool_conservation".to_string(),
            expr: format!(r#"tokens("{POOL_AVAILABLE}") + tokens("{POOL_IN_USE}") == {capacity}"#),
        }],
        ..Default::default()
    };

    let ports = vec![
        Port {
            id: POOL_AVAILABLE.to_string(),
            kind: Some(PortKind::Inout),
            place: POOL_AVAILABLE.to_string(),
            schema: "resource".to_string(),
            ..Default::default()
        },
        Port {
            id: POOL_IN_USE.to_string(),
            kind: Some(PortKind::Out),
            place: POOL_IN_USE.to_string(),
            schema: "resource".to_string(),
            ..Default::default()
        },
        Port {
            id: POOL_ACQUIRE.to_string(),
            kind: Some(PortKind::In),
            target: Some(PortTarget::Transition),
            transition: POOL_ACQUIRE.to_string(),
            ..Default::default()
        },
        Port {
            id: POOL_RELEASE.to_string(),
            kind: Some(PortKind::Out),
            target: Some(PortTarget::Transition),
            transition: POOL_RELEASE.to_string(),
            ..Default::default()
        },
    ];

    Subnet {
        typ: SUBNET_TYPE.to_string(),
        id,
        net_type: NetType::Resource,
        model: m,
        ports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_carries_conservation_constraint() {
        let s = new_resource_pool("baristas", 3);
        assert_eq!(s.model.places.len(), 2);
        assert_eq!(s.model.constraints.len(), 1);
        assert_eq!(s.model.place_by_id(POOL_AVAILABLE).unwrap().initial, 3);
        assert_eq!(s.model.place_by_id(POOL_IN_USE).unwrap().initial, 0);
    }
}
