//! `pflow_metamodel::Model` / composable `Subnet` emitters for [`Workflow`],
//! ported from go-pflow's `workflow/metasubnet.go`.
//!
//! Each task becomes three places (`<id>_ready`, `<id>_running`,
//! `<id>_completed`) and two transitions (`start_<id>`, `complete_<id>`);
//! each resource becomes one place with `capacity == initial == Capacity`;
//! each requirement is a single weighted arc rather than `Quantity`
//! duplicate weight-1 arcs (go-pflow's older `tokenmodel/subnet`-targeting
//! `ToSubnet` needed the duplicates because that substrate has no arc
//! weight). Each `DepFinishToStart` dependency becomes one
//! `dep_<from>_to_<to>` transition; other dependency types are not lowered
//! here — go-pflow's own `addMetaWorkflowTransitions` only handles
//! `DepFinishToStart` too, so `StartToStart`/`FinishToFinish`/`StartToFinish`
//! carry no structural arc and are join/split business logic the
//! [`crate::engine::Engine`] alone enforces (see its module doc for why that
//! is not a second firing-rule implementation).
//!
//! Each resource pool's conservation law — free units plus units held by
//! every task currently running against it equal the pool's capacity — is
//! emitted as a `Constraint`, which is what makes it safe to *share* a pool
//! across subnets via a `TokenLink`.

use pflow_compose::bundle::{NetType, Port, PortKind, PortTarget, Subnet};
use pflow_compose::normalize_kinds;
use pflow_metamodel::{Arc as MetaArc, Constraint, Model, Place, StateKind, Transition as MetaTransition};

use crate::types::{sorted_keys, DependencyType, Workflow};

impl Workflow {
    /// Emits the workflow as a `pflow_metamodel::Model`, with no boundary.
    pub fn to_meta_model(&self) -> Model {
        let mut m = Model {
            name: self.id.clone(),
            ..Default::default()
        };
        add_meta_places(&mut m, self);
        add_meta_transitions(&mut m, self);
        m.constraints.extend(resource_constraints(self));
        normalize_kinds(&mut m);
        m
    }

    /// Emits the workflow as a composable `Subnet`.
    ///
    /// Boundary:
    ///   - `"start"` in-port on the start task's ready place.
    ///   - `"done:<task>"` out-ports on each end task's completed place.
    ///   - `"resource:<id>"` inout ports, so a pool can be *shared* with
    ///     another subnet through a `TokenLink` rather than duplicated.
    ///   - transition ports `"start:<task>"` / `"complete:<task>"`, so a
    ///     task can fuse with an external action and fire atomically with
    ///     it.
    pub fn to_meta_subnet(&self) -> Subnet {
        let mut m = self.to_meta_model();
        let export = |m: &mut Model, id: &str| {
            if let Some(p) = m.place_by_id_mut(id) {
                p.exported = true;
            }
        };
        if !self.start_task_id.is_empty() {
            export(&mut m, &format!("{}_ready", self.start_task_id));
        }
        for end_id in &self.end_task_ids {
            export(&mut m, &format!("{end_id}_completed"));
        }
        for rid in sorted_keys(&self.resources) {
            export(&mut m, &rid);
        }

        let mut ports = Vec::new();
        if !self.start_task_id.is_empty() {
            ports.push(Port {
                id: "start".into(),
                kind: Some(PortKind::In),
                place: format!("{}_ready", self.start_task_id),
                schema: "workflow:trigger".into(),
                ..Default::default()
            });
        }
        for end_id in &self.end_task_ids {
            ports.push(Port {
                id: format!("done:{end_id}"),
                kind: Some(PortKind::Out),
                place: format!("{end_id}_completed"),
                schema: "workflow:completion".into(),
                ..Default::default()
            });
        }
        for rid in sorted_keys(&self.resources) {
            ports.push(Port {
                id: format!("resource:{rid}"),
                kind: Some(PortKind::Inout),
                place: rid,
                schema: "resource".into(),
                ..Default::default()
            });
        }
        for tid in sorted_keys(&self.tasks) {
            ports.push(Port {
                id: format!("start:{tid}"),
                kind: Some(PortKind::In),
                target: Some(PortTarget::Transition),
                transition: format!("start_{tid}"),
                ..Default::default()
            });
            ports.push(Port {
                id: format!("complete:{tid}"),
                kind: Some(PortKind::Out),
                target: Some(PortTarget::Transition),
                transition: format!("complete_{tid}"),
                ..Default::default()
            });
        }

        Subnet {
            typ: "PetriNet".into(),
            id: self.id.clone(),
            net_type: NetType::Workflow,
            model: m,
            ports,
        }
    }
}

fn resource_constraints(w: &Workflow) -> Vec<Constraint> {
    let mut out = Vec::new();
    for rid in sorted_keys(&w.resources) {
        let r = &w.resources[&rid];
        let mut expr = format!("tokens(\"{rid}\")");
        for tid in sorted_keys(&w.tasks) {
            for req in &w.tasks[&tid].required_resources {
                if req.resource_id != rid {
                    continue;
                }
                let n = if req.quantity < 1.0 { 1 } else { req.quantity as i64 };
                if n == 1 {
                    expr += &format!(" + tokens(\"{tid}_running\")");
                } else {
                    expr += &format!(" + {n}*tokens(\"{tid}_running\")");
                }
            }
        }
        out.push(Constraint {
            id: format!("resource_conservation_{rid}"),
            expr: format!("{expr} == {}", r.capacity as i64),
        });
    }
    out
}

fn add_meta_places(m: &mut Model, w: &Workflow) {
    for tid in sorted_keys(&w.tasks) {
        let ready_initial = if tid == w.start_task_id { 1 } else { 0 };
        m.places.push(Place {
            id: format!("{tid}_ready"),
            kind: Some(StateKind::Token),
            initial: ready_initial,
            description: "task ready".into(),
            ..Default::default()
        });
        m.places.push(Place {
            id: format!("{tid}_running"),
            kind: Some(StateKind::Token),
            description: "task running".into(),
            ..Default::default()
        });
        m.places.push(Place {
            id: format!("{tid}_completed"),
            kind: Some(StateKind::Token),
            description: "task completed".into(),
            ..Default::default()
        });
    }
    for rid in sorted_keys(&w.resources) {
        let r = &w.resources[&rid];
        m.places.push(Place {
            id: rid,
            kind: Some(StateKind::Token),
            initial: r.capacity as i64,
            capacity: r.capacity as i64,
            resource: true,
            description: "resource pool".into(),
            ..Default::default()
        });
    }
}

fn add_meta_transitions(m: &mut Model, w: &Workflow) {
    for tid in sorted_keys(&w.tasks) {
        let task = &w.tasks[&tid];
        let start_id = format!("start_{tid}");
        let complete_id = format!("complete_{tid}");

        m.transitions.push(MetaTransition {
            id: start_id.clone(),
            description: format!("begin {tid}"),
            ..Default::default()
        });
        m.transitions.push(MetaTransition {
            id: complete_id.clone(),
            description: format!("finish {tid}"),
            ..Default::default()
        });
        m.arcs.push(MetaArc { from: format!("{tid}_ready"), to: start_id.clone(), weight: 1, ..Default::default() });
        m.arcs.push(MetaArc { from: start_id.clone(), to: format!("{tid}_running"), weight: 1, ..Default::default() });
        m.arcs.push(MetaArc { from: format!("{tid}_running"), to: complete_id.clone(), weight: 1, ..Default::default() });
        m.arcs.push(MetaArc { from: complete_id.clone(), to: format!("{tid}_completed"), weight: 1, ..Default::default() });

        for req in &task.required_resources {
            let n = if req.quantity < 1.0 { 1 } else { req.quantity as i64 };
            m.arcs.push(MetaArc { from: req.resource_id.clone(), to: start_id.clone(), weight: n, ..Default::default() });
            m.arcs.push(MetaArc { from: complete_id.clone(), to: req.resource_id.clone(), weight: n, ..Default::default() });
        }
    }

    let mut deps: Vec<&crate::types::Dependency> = w.dependencies.iter().collect();
    deps.sort_by(|a, b| (a.from_task_id.as_str(), a.to_task_id.as_str())
        .cmp(&(b.from_task_id.as_str(), b.to_task_id.as_str())));
    for dep in deps {
        if !matches!(dep.typ, DependencyType::FinishToStart) {
            continue;
        }
        let txn_id = format!("dep_{}_to_{}", dep.from_task_id, dep.to_task_id);
        m.transitions.push(MetaTransition {
            id: txn_id.clone(),
            description: "dependency edge".into(),
            ..Default::default()
        });
        m.arcs.push(MetaArc { from: format!("{}_completed", dep.from_task_id), to: txn_id.clone(), weight: 1, ..Default::default() });
        m.arcs.push(MetaArc { from: txn_id, to: format!("{}_ready", dep.to_task_id), weight: 1, ..Default::default() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::WorkflowBuilder;
    use std::time::Duration;

    fn sample() -> Workflow {
        let mut b = WorkflowBuilder::new("order");
        b.resource("worker").capacity(2.0);
        b.task("receive").manual().requires("worker");
        b.task("ship").manual().requires("worker");
        b.connect("receive", "ship");
        b.start_at("receive");
        b.end_at(&["ship"]);
        b.build()
    }

    #[test]
    fn places_and_transitions_match_shape() {
        let m = sample().to_meta_model();
        let ids: Vec<&str> = m.places.iter().map(|p| p.id.as_str()).collect();
        for want in ["receive_ready", "receive_running", "receive_completed", "worker"] {
            assert!(ids.contains(&want), "missing {want}: {ids:?}");
        }
        assert_eq!(m.place_by_id("receive_ready").unwrap().initial, 1);
        assert_eq!(m.place_by_id("ship_ready").unwrap().initial, 0);
        assert_eq!(m.place_by_id("worker").unwrap().initial, 2);
        assert_eq!(m.place_by_id("worker").unwrap().capacity, 2);

        let txn_ids: Vec<&str> = m.transitions.iter().map(|t| t.id.as_str()).collect();
        assert!(txn_ids.contains(&"start_receive"));
        assert!(txn_ids.contains(&"complete_receive"));
        assert!(txn_ids.contains(&"dep_receive_to_ship"));

        assert_eq!(m.constraints.len(), 1);
        assert!(m.constraints[0].expr.contains("== 2"));
    }

    #[test]
    fn to_meta_subnet_exposes_start_done_and_resource_ports() {
        let sub = sample().to_meta_subnet();
        assert_eq!(sub.net_type, NetType::Workflow);
        assert!(sub.port_by_id("start").is_some());
        assert!(sub.port_by_id("done:ship").is_some());
        assert!(sub.port_by_id("resource:worker").is_some());
        assert!(sub.port_by_id("start:receive").is_some());
        assert!(sub.port_by_id("complete:receive").is_some());
    }

    #[test]
    fn multi_unit_requirement_is_one_weighted_arc() {
        let mut b = WorkflowBuilder::new("wf");
        b.resource("beans").capacity(10.0);
        b.task("brew").requires_n("beans", 3.0);
        b.start_at("brew");
        b.end_at(&["brew"]);
        let m = b.build().to_meta_model();

        let arcs: Vec<_> = m.arcs.iter().filter(|a| a.from == "beans" && a.to == "start_brew").collect();
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].weight, 3);
    }

    #[test]
    fn duration_field_is_carried_by_the_task_not_the_net() {
        // Sanity check that Task fields not lowered into the net (timing) are
        // still there for the engine to read directly.
        let mut b = WorkflowBuilder::new("wf");
        b.task("t").duration(Duration::from_secs(5));
        b.start_at("t");
        b.end_at(&["t"]);
        let wf = b.build();
        assert_eq!(wf.tasks["t"].estimated_duration, Duration::from_secs(5));
    }
}
