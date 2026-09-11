//! `pflow_core::PetriNet` (Shape A) emitter for [`Workflow`], ported from
//! go-pflow's `workflow/builder.go`'s `ToPetriNet`.
//!
//! This is the *older* of the two net views go-pflow's `workflow` package
//! carries — `metasubnet.rs`'s `to_meta_model`/`to_meta_subnet` are the
//! composable, weight-carrying Shape B path this crate's engine and
//! composition surface are built on. `to_petri_net` exists only because
//! [`crate::monitor::WorkflowPredictor`] needs a `pflow_solver`-compatible
//! net for ODE-based prediction, and `pflow_solver::ode::Problem` takes a
//! `pflow_core::PetriNet`, not a `pflow_metamodel::Model`. Only
//! `DepFinishToStart` dependencies are lowered, matching go-pflow's own
//! `ToPetriNet` exactly (see `metasubnet.rs`'s module doc for the same
//! limitation in the Shape B emitter).

use pflow_core::net::PetriNet;

use crate::types::{sorted_keys, DependencyType, Workflow};

impl Workflow {
    pub fn to_petri_net(&self) -> PetriNet {
        let mut net = PetriNet::new();
        let mut y = 100.0_f64;

        for tid in sorted_keys(&self.tasks) {
            let task = &self.tasks[&tid];
            let initial = if tid == self.start_task_id { 1.0 } else { 0.0 };
            net.add_place(format!("{tid}_ready"), vec![initial], vec![], 100.0, y, None);
            net.add_place(format!("{tid}_running"), vec![0.0], vec![], 200.0, y, None);
            net.add_place(format!("{tid}_completed"), vec![0.0], vec![], 300.0, y, None);

            net.add_transition(format!("start_{tid}"), "default", 150.0, y, None);
            net.add_arc(format!("{tid}_ready"), format!("start_{tid}"), vec![1.0], false);
            net.add_arc(format!("start_{tid}"), format!("{tid}_running"), vec![1.0], false);

            net.add_transition(format!("complete_{tid}"), "default", 250.0, y, None);
            net.add_arc(format!("{tid}_running"), format!("complete_{tid}"), vec![1.0], false);
            net.add_arc(format!("complete_{tid}"), format!("{tid}_completed"), vec![1.0], false);

            for req in &task.required_resources {
                net.add_arc(req.resource_id.clone(), format!("start_{tid}"), vec![req.quantity], false);
                net.add_arc(format!("complete_{tid}"), req.resource_id.clone(), vec![req.quantity], false);
            }

            y += 80.0;
        }

        for rid in sorted_keys(&self.resources) {
            let r = &self.resources[&rid];
            net.add_place(rid, vec![r.capacity], vec![], 400.0, y, None);
            y += 50.0;
        }

        for dep in &self.dependencies {
            if !matches!(dep.typ, DependencyType::FinishToStart) {
                continue;
            }
            let trans_name = format!("dep_{}_to_{}", dep.from_task_id, dep.to_task_id);
            net.add_transition(trans_name.clone(), "default", 350.0, y, None);
            net.add_arc(format!("{}_completed", dep.from_task_id), trans_name.clone(), vec![1.0], false);
            net.add_arc(trans_name, format!("{}_ready", dep.to_task_id), vec![1.0], false);
            y += 30.0;
        }

        net
    }
}

#[cfg(test)]
mod tests {
    use crate::builder::WorkflowBuilder;

    #[test]
    fn to_petri_net_has_ready_running_completed_places() {
        let mut b = WorkflowBuilder::new("wf");
        b.task("a");
        b.task("b");
        b.connect("a", "b");
        b.start_at("a");
        b.end_at(&["b"]);
        let net = b.build().to_petri_net();

        assert!(net.places.contains_key("a_ready"));
        assert!(net.places.contains_key("a_running"));
        assert!(net.places.contains_key("a_completed"));
        assert_eq!(net.places["a_ready"].initial, vec![1.0]);
        assert_eq!(net.places["b_ready"].initial, vec![0.0]);
        assert!(net.transitions.contains_key("dep_a_to_b"));
    }
}
