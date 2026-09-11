//! Fluent workflow construction, adapted from go-pflow's
//! `workflow/builder.go`. As in `pflow_statemachine::builder`, Go's chain of
//! back-pointing `Builder`/`TaskBuilder`/`ResourceBuilder` types collapses
//! into one [`WorkflowBuilder`] tracking "what's currently being built".

use std::time::Duration;

use crate::types::{
    Dependency, DependencyType, FailureAction, JoinType, Priority, Resource, ResourceProduction,
    ResourceRequirement, ResourceType, SlaBreachAction, SplitType, Task, TaskCondition,
    TaskSla, TaskType, Workflow, WorkflowSla,
};

#[derive(Default)]
pub struct WorkflowBuilder {
    workflow: Workflow,
    current_task: Option<String>,
    current_resource: Option<String>,
}

impl WorkflowBuilder {
    pub fn new(id: impl Into<String>) -> Self {
        WorkflowBuilder {
            workflow: Workflow {
                id: id.into(),
                default_priority: Priority::Medium,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn name(&mut self, name: impl Into<String>) -> &mut Self {
        self.workflow.name = name.into();
        self
    }

    pub fn description(&mut self, desc: impl Into<String>) -> &mut Self {
        self.workflow.description = desc.into();
        self
    }

    pub fn version(&mut self, v: impl Into<String>) -> &mut Self {
        self.workflow.version = v.into();
        self
    }

    pub fn default_timeout(&mut self, d: Duration) -> &mut Self {
        self.workflow.default_timeout = d;
        self
    }

    pub fn sla(&mut self, sla: WorkflowSla) -> &mut Self {
        self.workflow.sla = Some(sla);
        self
    }

    pub fn label(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.workflow.labels.insert(key.into(), value.into());
        self
    }

    // --- Task building ---

    pub fn task(&mut self, id: impl Into<String>) -> &mut Self {
        let id = id.into();
        self.workflow.tasks.insert(
            id.clone(),
            Task {
                id: id.clone(),
                typ: TaskType::Manual,
                join_type: JoinType::All,
                split_type: SplitType::All,
                ..Default::default()
            },
        );
        self.current_task = Some(id);
        self
    }

    fn task_mut(&mut self) -> &mut Task {
        let id = self
            .current_task
            .clone()
            .expect("task() must be called first");
        self.workflow.tasks.get_mut(&id).expect("task exists")
    }

    pub fn task_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.task_mut().name = name.into();
        self
    }

    pub fn task_type(&mut self, t: TaskType) -> &mut Self {
        self.task_mut().typ = t;
        self
    }

    pub fn manual(&mut self) -> &mut Self {
        self.task_mut().typ = TaskType::Manual;
        self
    }

    pub fn automatic(&mut self) -> &mut Self {
        self.task_mut().typ = TaskType::Automatic;
        self
    }

    pub fn decision(&mut self) -> &mut Self {
        self.task_mut().typ = TaskType::Decision;
        self
    }

    pub fn duration(&mut self, d: Duration) -> &mut Self {
        self.task_mut().estimated_duration = d;
        self
    }

    pub fn duration_range(&mut self, min: Duration, expected: Duration, max: Duration) -> &mut Self {
        let t = self.task_mut();
        t.min_duration = min;
        t.estimated_duration = expected;
        t.max_duration = max;
        self
    }

    pub fn timeout(&mut self, d: Duration) -> &mut Self {
        self.task_mut().timeout = d;
        self
    }

    pub fn requires(&mut self, resource_id: impl Into<String>) -> &mut Self {
        self.requires_n(resource_id, 1.0)
    }

    pub fn requires_n(&mut self, resource_id: impl Into<String>, quantity: f64) -> &mut Self {
        self.task_mut().required_resources.push(ResourceRequirement {
            resource_id: resource_id.into(),
            quantity,
            exclusive: false,
        });
        self
    }

    pub fn requires_exclusive(&mut self, resource_id: impl Into<String>) -> &mut Self {
        self.task_mut().required_resources.push(ResourceRequirement {
            resource_id: resource_id.into(),
            quantity: 1.0,
            exclusive: true,
        });
        self
    }

    pub fn produces(&mut self, resource_id: impl Into<String>, quantity: f64) -> &mut Self {
        self.task_mut().produced_resources.push(ResourceProduction {
            resource_id: resource_id.into(),
            quantity,
        });
        self
    }

    pub fn join_all(&mut self) -> &mut Self {
        self.task_mut().join_type = JoinType::All;
        self
    }

    pub fn join_any(&mut self) -> &mut Self {
        self.task_mut().join_type = JoinType::Any;
        self
    }

    pub fn join_n_of(&mut self, n: usize) -> &mut Self {
        let t = self.task_mut();
        t.join_type = JoinType::NOfM;
        t.join_count = n;
        self
    }

    pub fn split_all(&mut self) -> &mut Self {
        self.task_mut().split_type = SplitType::All;
        self
    }

    pub fn split_exclusive(&mut self) -> &mut Self {
        self.task_mut().split_type = SplitType::Exclusive;
        self
    }

    pub fn split_inclusive(&mut self) -> &mut Self {
        self.task_mut().split_type = SplitType::Inclusive;
        self
    }

    pub fn retry(&mut self, max_retries: u32, delay: Duration) -> &mut Self {
        let t = self.task_mut();
        t.max_retries = max_retries;
        t.retry_delay = delay;
        self
    }

    pub fn max_retries(&mut self, n: u32) -> &mut Self {
        self.task_mut().max_retries = n;
        self
    }

    pub fn on_failure(&mut self, action: FailureAction) -> &mut Self {
        self.task_mut().failure_action = action;
        self
    }

    pub fn when(&mut self, condition: TaskCondition) -> &mut Self {
        self.task_mut().condition = Some(condition);
        self
    }

    pub fn task_sla(
        &mut self,
        target: Duration,
        warning_pct: f64,
        critical_pct: f64,
        action: SlaBreachAction,
    ) -> &mut Self {
        self.task_mut().sla = Some(TaskSla {
            target_duration: target,
            warning_at: warning_pct,
            critical_at: critical_pct,
            breach_action: action,
            escalate_to: String::new(),
        });
        self
    }

    pub fn with_sla(&mut self, target: Duration, warning_pct: f64, critical_pct: f64) -> &mut Self {
        self.task_sla(target, warning_pct, critical_pct, SlaBreachAction::Alert)
    }

    pub fn task_label(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.task_mut().labels.insert(key.into(), value.into());
        self
    }

    // --- Dependency building ---

    pub fn connect(&mut self, from: impl Into<String>, to: impl Into<String>) -> &mut Self {
        self.workflow.dependencies.push(Dependency {
            from_task_id: from.into(),
            to_task_id: to.into(),
            typ: DependencyType::FinishToStart,
            lag: Duration::ZERO,
            condition: None,
        });
        self
    }

    pub fn connect_with_lag(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        lag: Duration,
    ) -> &mut Self {
        self.workflow.dependencies.push(Dependency {
            from_task_id: from.into(),
            to_task_id: to.into(),
            typ: DependencyType::FinishToStart,
            lag,
            condition: None,
        });
        self
    }

    pub fn start_to_start(&mut self, from: impl Into<String>, to: impl Into<String>) -> &mut Self {
        self.workflow.dependencies.push(Dependency {
            from_task_id: from.into(),
            to_task_id: to.into(),
            typ: DependencyType::StartToStart,
            lag: Duration::ZERO,
            condition: None,
        });
        self
    }

    pub fn finish_to_finish(&mut self, from: impl Into<String>, to: impl Into<String>) -> &mut Self {
        self.workflow.dependencies.push(Dependency {
            from_task_id: from.into(),
            to_task_id: to.into(),
            typ: DependencyType::FinishToFinish,
            lag: Duration::ZERO,
            condition: None,
        });
        self
    }

    pub fn sequence(&mut self, task_ids: &[&str]) -> &mut Self {
        for w in task_ids.windows(2) {
            self.connect(w[0], w[1]);
        }
        self
    }

    pub fn parallel(&mut self, from: impl Into<String>, to: &[&str]) -> &mut Self {
        let from = from.into();
        for t in to {
            self.connect(from.clone(), *t);
        }
        self
    }

    pub fn merge(&mut self, to: impl Into<String>, from: &[&str]) -> &mut Self {
        let to = to.into();
        for f in from {
            self.connect(*f, to.clone());
        }
        self
    }

    // --- Resource building ---

    pub fn resource(&mut self, id: impl Into<String>) -> &mut Self {
        let id = id.into();
        self.workflow.resources.insert(
            id.clone(),
            Resource {
                id: id.clone(),
                typ: ResourceType::Worker,
                ..Default::default()
            },
        );
        self.current_resource = Some(id);
        self
    }

    fn resource_mut(&mut self) -> &mut Resource {
        let id = self
            .current_resource
            .clone()
            .expect("resource() must be called first");
        self.workflow.resources.get_mut(&id).expect("resource exists")
    }

    pub fn resource_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.resource_mut().name = name.into();
        self
    }

    pub fn resource_type(&mut self, t: ResourceType) -> &mut Self {
        self.resource_mut().typ = t;
        self
    }

    pub fn worker(&mut self) -> &mut Self {
        self.resource_mut().typ = ResourceType::Worker;
        self
    }

    pub fn equipment(&mut self) -> &mut Self {
        self.resource_mut().typ = ResourceType::Equipment;
        self
    }

    pub fn capacity(&mut self, capacity: f64) -> &mut Self {
        self.resource_mut().capacity = capacity;
        self
    }

    pub fn max_concurrent(&mut self, max: u32) -> &mut Self {
        self.resource_mut().max_concurrent = max;
        self
    }

    // --- Start/end points ---

    pub fn start_at(&mut self, task_id: impl Into<String>) -> &mut Self {
        self.workflow.start_task_id = task_id.into();
        self
    }

    pub fn end_at(&mut self, task_ids: &[&str]) -> &mut Self {
        self.workflow
            .end_task_ids
            .extend(task_ids.iter().map(|s| s.to_string()));
        self
    }

    // --- Build ---

    /// Validates and returns the workflow, or the first validation error.
    pub fn build_validated(&mut self) -> Result<Workflow, String> {
        self.validate()?;
        Ok(std::mem::take(&mut self.workflow))
    }

    /// Returns the workflow without validating (go-pflow's `Build`,
    /// "validates but continues on error for convenience" — it in fact
    /// skips validation entirely; kept for parity with call sites that
    /// intend to validate separately or not at all).
    pub fn build(&mut self) -> Workflow {
        std::mem::take(&mut self.workflow)
    }

    fn validate(&self) -> Result<(), String> {
        let w = &self.workflow;
        if w.tasks.is_empty() {
            return Err("workflow has no tasks".to_string());
        }
        if w.start_task_id.is_empty() {
            return Err("workflow has no start task".to_string());
        }
        if !w.tasks.contains_key(&w.start_task_id) {
            return Err(format!("start task {:?} not found", w.start_task_id));
        }
        if w.end_task_ids.is_empty() {
            return Err("workflow has no end tasks".to_string());
        }
        for end_id in &w.end_task_ids {
            if !w.tasks.contains_key(end_id) {
                return Err(format!("end task {end_id:?} not found"));
            }
        }
        for dep in &w.dependencies {
            if !w.tasks.contains_key(&dep.from_task_id) {
                return Err(format!("dependency from unknown task {:?}", dep.from_task_id));
            }
            if !w.tasks.contains_key(&dep.to_task_id) {
                return Err(format!("dependency to unknown task {:?}", dep.to_task_id));
            }
        }
        for task in w.tasks.values() {
            for req in &task.required_resources {
                if !w.resources.contains_key(&req.resource_id) {
                    return Err(format!(
                        "task {:?} requires unknown resource {:?}",
                        task.id, req.resource_id
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_linear_workflow() {
        let mut b = WorkflowBuilder::new("order");
        b.task("receive").manual().duration(Duration::from_secs(120));
        b.task("validate").automatic().duration(Duration::from_secs(30));
        b.task("ship").manual().duration(Duration::from_secs(300));
        b.connect("receive", "validate");
        b.connect("validate", "ship");
        b.start_at("receive");
        b.end_at(&["ship"]);
        let wf = b.build_validated().unwrap();

        assert_eq!(wf.tasks.len(), 3);
        assert_eq!(wf.dependencies.len(), 2);
        assert_eq!(wf.start_task_id, "receive");
        assert_eq!(wf.end_task_ids, vec!["ship".to_string()]);
    }

    #[test]
    fn validation_catches_unknown_resource() {
        let mut b = WorkflowBuilder::new("wf");
        b.task("t").requires("nope");
        b.start_at("t");
        b.end_at(&["t"]);
        let err = b.build_validated().err().unwrap();
        assert!(err.contains("unknown resource"), "{err}");
    }

    #[test]
    fn validation_catches_missing_start() {
        let mut b = WorkflowBuilder::new("wf");
        b.task("t");
        b.end_at(&["t"]);
        let err = b.build_validated().err().unwrap();
        assert!(err.contains("no start task"), "{err}");
    }
}
