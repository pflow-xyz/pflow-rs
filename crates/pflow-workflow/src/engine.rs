//! Case execution: dependency-join scheduling, resource acquisition/release,
//! retries and SLA tracking — ported from go-pflow's `workflow/engine.go`.
//!
//! **Not built on the compiled net.** go-pflow's own `Engine` constructs
//! `workflow.ToPetriNet()` but never actually queries it: `enableReadyTasks`
//! decides readiness with its own `areDependenciesMet`/`isDependencySatisfied`
//! switch over `JoinType`, and `AssignTask`/`CompleteTask` track resources
//! with a hand-rolled `ResourcePool`, not a marking. That is a deliberate
//! reading of ROADMAP.md ground rule 4, not an oversight: the rule is about
//! not *reimplementing the firing rule* (consume/read/inhibit/capacity), and
//! join/split semantics are not that rule — `to_meta_model`'s
//! `dep_<from>_to_<to>` transitions are structurally an OR-join regardless
//! of a task's declared `JoinType` (each dependency edge fires independently
//! into the shared `<task>_ready` place), so `JoinAll`/`JoinN` could not be
//! recovered from the compiled net's enablement even if this engine asked
//! it. This port keeps the same split: `to_meta_model`/`to_meta_subnet`
//! (`metasubnet.rs`) are the composable, provable structural view; this
//! engine is workflow-specific scheduling business logic that owns its own
//! state, exactly as go-pflow's does.
//!
//! Go's seven `On*` handler-registration methods (`OnTaskReady`,
//! `OnCaseFailed`, ...) each capture the engine in a closure that can call
//! back into it — safe under Go's GC and mutexes, not under Rust's
//! `&mut self`. This port collapses them into one [`EngineEvent`] enum and
//! a single `on_event` registration, with every field owned rather than
//! borrowed, so a handler can be called without holding any borrow of the
//! engine open.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use crate::types::{
    Alert, AlertSeverity, AlertType, Case, CaseStatus, DependencyType, ExecutionContext,
    FailureAction, JoinType, Priority, ResourceRequirement, TaskInstance, TaskMetrics,
    TaskStatus, Workflow,
};

/// Something the engine did, for [`Engine::on_event`] observers. Carries
/// owned data (never a borrow into the engine) so a handler can run without
/// holding any lock open — see the module doc.
pub enum EngineEvent {
    TaskReady { case_id: String, task_id: String },
    TaskStarted { case_id: String, task_id: String },
    TaskComplete { case_id: String, task_id: String },
    TaskFailed { case_id: String, task_id: String, error: String },
    CaseComplete { case_id: String },
    CaseFailed { case_id: String, error: String },
    Alert(Alert),
}

type EventHandler = Box<dyn FnMut(&EngineEvent)>;

struct ResourcePool {
    capacity: f64,
    available: f64,
    reserved: HashMap<String, f64>,
}

/// Executes workflow instances and manages their lifecycle.
pub struct Engine {
    workflow: Workflow,
    resources: HashMap<String, ResourcePool>,
    cases: HashMap<String, Case>,
    handlers: Vec<EventHandler>,
    now: Box<dyn Fn() -> SystemTime>,
}

impl Engine {
    pub fn new(workflow: Workflow) -> Self {
        let resources = workflow
            .resources
            .iter()
            .map(|(id, r)| {
                (
                    id.clone(),
                    ResourcePool {
                        capacity: r.capacity,
                        available: r.capacity,
                        reserved: HashMap::new(),
                    },
                )
            })
            .collect();

        Engine {
            workflow,
            resources,
            cases: HashMap::new(),
            handlers: Vec::new(),
            now: Box::new(SystemTime::now),
        }
    }

    /// Sets a custom time source (useful for tests).
    pub fn with_time_source(mut self, now: impl Fn() -> SystemTime + 'static) -> Self {
        self.now = Box::new(now);
        self
    }

    pub fn on_event(&mut self, handler: impl FnMut(&EngineEvent) + 'static) {
        self.handlers.push(Box::new(handler));
    }

    fn emit(&mut self, ev: EngineEvent) {
        for h in &mut self.handlers {
            h(&ev);
        }
    }

    pub fn workflow(&self) -> &Workflow {
        &self.workflow
    }

    // --- Case lifecycle ---

    pub fn start_case(
        &mut self,
        case_id: impl Into<String>,
        input: HashMap<String, String>,
        priority: Priority,
    ) -> Result<(), String> {
        let case_id = case_id.into();
        if self.cases.contains_key(&case_id) {
            return Err(format!("case {case_id} already exists"));
        }

        let now = (self.now)();
        let deadline = self.workflow.sla.as_ref().map(|sla| {
            now + *sla.by_priority.get(&priority).unwrap_or(&sla.default)
        });

        let mut case = Case {
            id: case_id.clone(),
            workflow_id: self.workflow.id.clone(),
            priority,
            status: CaseStatus::Running,
            created_at: Some(now),
            started_at: Some(now),
            deadline,
            variables: input.clone(),
            input,
            ..Default::default()
        };

        for (task_id, task) in &self.workflow.tasks {
            let deadline = task.sla.as_ref().map(|sla| now + sla.target_duration);
            case.task_instances.insert(
                task_id.clone(),
                TaskInstance {
                    id: format!("{case_id}_{task_id}"),
                    task_id: task_id.clone(),
                    case_id: case_id.clone(),
                    status: TaskStatus::Pending,
                    created_at: Some(now),
                    deadline,
                    ..Default::default()
                },
            );
        }

        self.cases.insert(case_id.clone(), case);
        self.enable_ready_tasks(&case_id);
        Ok(())
    }

    /// Checks each pending task's dependencies and marks the ones that are
    /// met as ready, skipping tasks whose `condition` says no. Ports
    /// `Engine.enableReadyTasks`.
    fn enable_ready_tasks(&mut self, case_id: &str) {
        let now = (self.now)();
        let workflow = &self.workflow;
        let case = self.cases.get(case_id).unwrap();

        let mut ready = Vec::new();
        let mut skipped = Vec::new();
        for (task_id, instance) in &case.task_instances {
            if !matches!(instance.status, TaskStatus::Pending) {
                continue;
            }
            if dependencies_met(workflow, case, task_id) {
                let task = &workflow.tasks[task_id];
                let should_skip = if let Some(cond) = &task.condition {
                    let ctx = ExecutionContext {
                        case,
                        task_instance: instance,
                        variables: &case.variables,
                        now,
                    };
                    !cond(&ctx)
                } else {
                    false
                };
                if should_skip {
                    skipped.push(task_id.clone());
                } else {
                    ready.push(task_id.clone());
                }
            }
        }

        let case = self.cases.get_mut(case_id).unwrap();
        for task_id in &skipped {
            let inst = case.task_instances.get_mut(task_id).unwrap();
            inst.status = TaskStatus::Skipped;
            inst.completed_at = Some(now);
            case.completed_tasks.push(task_id.clone());
        }
        for task_id in &ready {
            let inst = case.task_instances.get_mut(task_id).unwrap();
            inst.status = TaskStatus::Ready;
            inst.ready_at = Some(now);
            case.current_tasks.push(task_id.clone());
        }

        for task_id in ready {
            self.emit(EngineEvent::TaskReady {
                case_id: case_id.to_string(),
                task_id,
            });
        }
    }

    pub fn assign_task(
        &mut self,
        case_id: &str,
        task_id: &str,
        assignee: impl Into<String>,
    ) -> Result<(), String> {
        let now = (self.now)();
        let required = {
            let case = self.cases.get(case_id).ok_or(format!("case {case_id} not found"))?;
            let instance = case
                .task_instances
                .get(task_id)
                .ok_or(format!("task {task_id} not found in case {case_id}"))?;
            if !matches!(instance.status, TaskStatus::Ready) {
                return Err(format!("task {task_id} is not ready"));
            }
            self.workflow.tasks[task_id].required_resources.clone()
        };

        self.acquire_resources(case_id, &required)?;

        let case = self.cases.get_mut(case_id).unwrap();
        let instance = case.task_instances.get_mut(task_id).unwrap();
        instance.status = TaskStatus::Assigned;
        instance.assigned_to = assignee.into();
        instance.assigned_at = Some(now);
        Ok(())
    }

    pub fn start_task(&mut self, case_id: &str, task_id: &str) -> Result<(), String> {
        let now = (self.now)();
        let was_ready = {
            let case = self.cases.get(case_id).ok_or(format!("case {case_id} not found"))?;
            let instance = case
                .task_instances
                .get(task_id)
                .ok_or(format!("task {task_id} not found in case {case_id}"))?;
            match instance.status {
                TaskStatus::Assigned => false,
                TaskStatus::Ready => true,
                _ => return Err(format!("task {task_id} cannot be started")),
            }
        };

        if was_ready {
            let required = self.workflow.tasks[task_id].required_resources.clone();
            self.acquire_resources(case_id, &required)?;
        }

        let case = self.cases.get_mut(case_id).unwrap();
        let instance = case.task_instances.get_mut(task_id).unwrap();
        instance.status = TaskStatus::Running;
        instance.started_at = Some(now);
        if let Some(ready_at) = instance.ready_at {
            instance.wait_duration = now.duration_since(ready_at).unwrap_or_default();
        }

        self.emit(EngineEvent::TaskStarted {
            case_id: case_id.to_string(),
            task_id: task_id.to_string(),
        });
        Ok(())
    }

    pub fn complete_task(
        &mut self,
        case_id: &str,
        task_id: &str,
        output: HashMap<String, String>,
    ) -> Result<(), String> {
        let now = (self.now)();
        {
            let case = self.cases.get(case_id).ok_or(format!("case {case_id} not found"))?;
            let instance = case
                .task_instances
                .get(task_id)
                .ok_or(format!("task {task_id} not found in case {case_id}"))?;
            if !matches!(instance.status, TaskStatus::Running) {
                return Err(format!("task {task_id} is not running"));
            }
        }

        let required = self.workflow.tasks[task_id].required_resources.clone();
        let produced = self.workflow.tasks[task_id].produced_resources.clone();

        let case = self.cases.get_mut(case_id).unwrap();
        let instance = case.task_instances.get_mut(task_id).unwrap();
        instance.status = TaskStatus::Completed;
        instance.completed_at = Some(now);
        instance.output = output.clone();
        if let Some(started_at) = instance.started_at {
            instance.work_duration = now.duration_since(started_at).unwrap_or_default();
        }
        if let Some(created_at) = instance.created_at {
            instance.total_duration = now.duration_since(created_at).unwrap_or_default();
        }

        self.release_resources(case_id, &required);
        self.produce_resources(&produced);

        let case = self.cases.get_mut(case_id).unwrap();
        case.current_tasks.retain(|t| t != task_id);
        case.completed_tasks.push(task_id.to_string());
        for (k, v) in output {
            case.variables.insert(k, v);
        }

        self.emit(EngineEvent::TaskComplete {
            case_id: case_id.to_string(),
            task_id: task_id.to_string(),
        });

        if self.is_case_complete(case_id) {
            self.complete_case(case_id);
        } else {
            self.enable_ready_tasks(case_id);
        }
        Ok(())
    }

    pub fn fail_task(&mut self, case_id: &str, task_id: &str, err: &str) -> Result<(), String> {
        let now = (self.now)();
        let task = {
            let case = self.cases.get(case_id).ok_or(format!("case {case_id} not found"))?;
            if !case.task_instances.contains_key(task_id) {
                return Err(format!("task {task_id} not found in case {case_id}"));
            }
            &self.workflow.tasks[task_id]
        };
        let max_retries = task.max_retries;
        let required = task.required_resources.clone();
        let failure_action = task.failure_action;

        let retry_count = {
            let case = self.cases.get_mut(case_id).unwrap();
            let instance = case.task_instances.get_mut(task_id).unwrap();
            instance.retry_count
        };

        if retry_count < max_retries {
            let case = self.cases.get_mut(case_id).unwrap();
            let instance = case.task_instances.get_mut(task_id).unwrap();
            instance.retry_count += 1;
            instance.status = TaskStatus::Ready;
            instance.error = err.to_string();
            self.release_resources(case_id, &required);
            return Ok(());
        }

        {
            let case = self.cases.get_mut(case_id).unwrap();
            let instance = case.task_instances.get_mut(task_id).unwrap();
            instance.status = TaskStatus::Failed;
            instance.completed_at = Some(now);
            instance.error = err.to_string();
        }
        self.release_resources(case_id, &required);

        self.emit(EngineEvent::TaskFailed {
            case_id: case_id.to_string(),
            task_id: task_id.to_string(),
            error: err.to_string(),
        });

        match failure_action {
            FailureAction::Skip => {
                let case = self.cases.get_mut(case_id).unwrap();
                case.current_tasks.retain(|t| t != task_id);
                case.completed_tasks.push(task_id.to_string());
                self.enable_ready_tasks(case_id);
            }
            FailureAction::Escalate => {
                self.emit_alert(Alert {
                    id: format!("alert_{case_id}_{task_id}"),
                    typ: AlertType::TaskFailed,
                    severity: AlertSeverity::Critical,
                    case_id: case_id.to_string(),
                    task_id: task_id.to_string(),
                    message: format!("Task {task_id} failed and escalated: {err}"),
                    created_at: now,
                });
            }
            _ => {
                self.fail_case(case_id, &format!("task {task_id} failed: {err}"));
            }
        }
        Ok(())
    }

    fn acquire_resources(
        &mut self,
        case_id: &str,
        requirements: &[ResourceRequirement],
    ) -> Result<(), String> {
        for req in requirements {
            let pool = self
                .resources
                .get(&req.resource_id)
                .ok_or(format!("resource {} not found", req.resource_id))?;
            if pool.available < req.quantity {
                return Err(format!(
                    "insufficient {}: need {:.2}, have {:.2}",
                    req.resource_id, req.quantity, pool.available
                ));
            }
        }
        for req in requirements {
            let pool = self.resources.get_mut(&req.resource_id).unwrap();
            pool.available -= req.quantity;
            *pool.reserved.entry(case_id.to_string()).or_insert(0.0) += req.quantity;
        }
        Ok(())
    }

    fn release_resources(&mut self, case_id: &str, requirements: &[ResourceRequirement]) {
        for req in requirements {
            let Some(pool) = self.resources.get_mut(&req.resource_id) else {
                continue;
            };
            pool.available += req.quantity;
            if let Some(r) = pool.reserved.get_mut(case_id) {
                *r -= req.quantity;
                if *r <= 0.0 {
                    pool.reserved.remove(case_id);
                }
            }
        }
    }

    fn produce_resources(&mut self, productions: &[crate::types::ResourceProduction]) {
        for prod in productions {
            if let Some(pool) = self.resources.get_mut(&prod.resource_id) {
                pool.available += prod.quantity;
            }
        }
    }

    fn is_case_complete(&self, case_id: &str) -> bool {
        if self.workflow.end_task_ids.is_empty() {
            return false;
        }
        let case = &self.cases[case_id];
        self.workflow.end_task_ids.iter().all(|end_id| {
            case.task_instances
                .get(end_id)
                .map(|i| matches!(i.status, TaskStatus::Completed | TaskStatus::Skipped))
                .unwrap_or(true)
        })
    }

    fn complete_case(&mut self, case_id: &str) {
        let now = (self.now)();
        let case = self.cases.get_mut(case_id).unwrap();
        case.status = CaseStatus::Completed;
        case.completed_at = Some(now);

        let end_ids = self.workflow.end_task_ids.clone();
        let case = self.cases.get_mut(case_id).unwrap();
        for end_id in end_ids {
            if let Some(instance) = case.task_instances.get(&end_id) {
                let output = instance.output.clone();
                case.output.extend(output);
            }
        }

        self.emit(EngineEvent::CaseComplete {
            case_id: case_id.to_string(),
        });
    }

    fn fail_case(&mut self, case_id: &str, err: &str) {
        let now = (self.now)();
        let case = self.cases.get_mut(case_id).unwrap();
        case.status = CaseStatus::Failed;
        case.completed_at = Some(now);
        for instance in case.task_instances.values_mut() {
            if matches!(
                instance.status,
                TaskStatus::Pending | TaskStatus::Ready | TaskStatus::Assigned | TaskStatus::Running
            ) {
                instance.status = TaskStatus::Cancelled;
                instance.completed_at = Some(now);
            }
        }

        self.emit(EngineEvent::CaseFailed {
            case_id: case_id.to_string(),
            error: err.to_string(),
        });
    }

    pub fn cancel_case(&mut self, case_id: &str) -> Result<(), String> {
        let now = (self.now)();
        {
            let case = self.cases.get(case_id).ok_or(format!("case {case_id} not found"))?;
            if !matches!(case.status, CaseStatus::Running) {
                return Err(format!("case {case_id} is not running"));
            }
        }
        let case = self.cases.get_mut(case_id).unwrap();
        case.status = CaseStatus::Cancelled;
        case.completed_at = Some(now);
        for instance in case.task_instances.values_mut() {
            if !matches!(instance.status, TaskStatus::Completed | TaskStatus::Skipped) {
                instance.status = TaskStatus::Cancelled;
                instance.completed_at = Some(now);
            }
        }

        let all_reqs: Vec<ResourceRequirement> = self
            .workflow
            .tasks
            .values()
            .flat_map(|t| t.required_resources.iter().cloned())
            .collect();
        self.release_resources(case_id, &all_reqs);
        Ok(())
    }

    pub fn suspend_case(&mut self, case_id: &str) -> Result<(), String> {
        let case = self.cases.get_mut(case_id).ok_or(format!("case {case_id} not found"))?;
        if !matches!(case.status, CaseStatus::Running) {
            return Err(format!("case {case_id} is not running"));
        }
        case.status = CaseStatus::Suspended;
        Ok(())
    }

    pub fn resume_case(&mut self, case_id: &str) -> Result<(), String> {
        let case = self.cases.get_mut(case_id).ok_or(format!("case {case_id} not found"))?;
        if !matches!(case.status, CaseStatus::Suspended) {
            return Err(format!("case {case_id} is not suspended"));
        }
        case.status = CaseStatus::Running;
        Ok(())
    }

    pub fn get_case(&self, case_id: &str) -> Option<&Case> {
        self.cases.get(case_id)
    }

    pub fn get_cases(&self, filter: impl Fn(&Case) -> bool) -> Vec<&Case> {
        self.cases.values().filter(|c| filter(c)).collect()
    }

    pub fn get_ready_tasks(&self) -> Vec<&TaskInstance> {
        self.cases
            .values()
            .filter(|c| matches!(c.status, CaseStatus::Running))
            .flat_map(|c| c.task_instances.values())
            .filter(|i| matches!(i.status, TaskStatus::Ready))
            .collect()
    }

    pub fn get_resource_availability(&self) -> HashMap<String, f64> {
        self.resources
            .iter()
            .map(|(id, pool)| (id.clone(), pool.available))
            .collect()
    }

    fn emit_alert(&mut self, alert: Alert) {
        self.emit(EngineEvent::Alert(alert));
    }

    /// Checks every active case and task for SLA violations, emitting
    /// alerts and returning them.
    pub fn check_slas(&mut self) -> Vec<Alert> {
        let now = (self.now)();
        let mut alerts = Vec::new();

        let running_case_ids: Vec<String> = self
            .cases
            .values()
            .filter(|c| matches!(c.status, CaseStatus::Running))
            .map(|c| c.id.clone())
            .collect();

        for case_id in running_case_ids {
            let case = &self.cases[&case_id];
            if let (Some(deadline), Some(started_at), Some(sla)) =
                (case.deadline, case.started_at, &self.workflow.sla)
            {
                let remaining = deadline.duration_since(now).unwrap_or_default();
                let total = deadline.duration_since(started_at).unwrap_or_default();
                let elapsed = elapsed_fraction(total, remaining);

                if elapsed >= sla.critical_at {
                    alerts.push(Alert {
                        id: format!("sla_critical_{case_id}"),
                        typ: AlertType::SlaBreach,
                        severity: AlertSeverity::Critical,
                        case_id: case_id.clone(),
                        task_id: String::new(),
                        message: format!(
                            "Case {case_id} is at critical SLA threshold ({:.0}%)",
                            elapsed * 100.0
                        ),
                        created_at: now,
                    });
                } else if elapsed >= sla.warning_at {
                    alerts.push(Alert {
                        id: format!("sla_warning_{case_id}"),
                        typ: AlertType::SlaWarning,
                        severity: AlertSeverity::Warning,
                        case_id: case_id.clone(),
                        task_id: String::new(),
                        message: format!(
                            "Case {case_id} is approaching SLA deadline ({:.0}%)",
                            elapsed * 100.0
                        ),
                        created_at: now,
                    });
                }
            }

            let mut escalate: Vec<String> = Vec::new();
            for (task_id, instance) in &case.task_instances {
                if !matches!(instance.status, TaskStatus::Running | TaskStatus::Ready) {
                    continue;
                }
                let Some(task) = self.workflow.tasks.get(task_id) else { continue };
                let (Some(task_sla), Some(deadline)) = (&task.sla, instance.deadline) else {
                    continue;
                };
                let Some(start_time) = instance.started_at.or(instance.ready_at) else {
                    continue;
                };
                let remaining = deadline.duration_since(now).unwrap_or_default();
                let total = deadline.duration_since(start_time).unwrap_or_default();
                let elapsed = elapsed_fraction(total, remaining);

                if elapsed >= task_sla.critical_at {
                    alerts.push(Alert {
                        id: format!("task_sla_critical_{case_id}_{task_id}"),
                        typ: AlertType::SlaBreach,
                        severity: AlertSeverity::Critical,
                        case_id: case_id.clone(),
                        task_id: task_id.clone(),
                        message: format!(
                            "Task {task_id} is at critical SLA threshold ({:.0}%)",
                            elapsed * 100.0
                        ),
                        created_at: now,
                    });
                    if matches!(task_sla.breach_action, crate::types::SlaBreachAction::Escalate) {
                        escalate.push(task_id.clone());
                    }
                } else if elapsed >= task_sla.warning_at {
                    alerts.push(Alert {
                        id: format!("task_sla_warning_{case_id}_{task_id}"),
                        typ: AlertType::SlaWarning,
                        severity: AlertSeverity::Warning,
                        case_id: case_id.clone(),
                        task_id: task_id.clone(),
                        message: format!(
                            "Task {task_id} is approaching SLA deadline ({:.0}%)",
                            elapsed * 100.0
                        ),
                        created_at: now,
                    });
                }
            }
            if !escalate.is_empty() {
                let case = self.cases.get_mut(&case_id).unwrap();
                for task_id in escalate {
                    case.task_instances.get_mut(&task_id).unwrap().status = TaskStatus::Escalated;
                }
            }
        }

        for a in alerts.clone() {
            self.emit_alert(a);
        }
        alerts
    }

    pub fn get_metrics(&self) -> crate::types::Metrics {
        let mut m = crate::types::Metrics::default();
        let mut durations: Vec<Duration> = Vec::new();

        for c in self.cases.values() {
            m.total_cases += 1;
            match c.status {
                CaseStatus::Running => m.active_cases += 1,
                CaseStatus::Completed => {
                    m.completed_cases += 1;
                    if let (Some(s), Some(e)) = (c.started_at, c.completed_at) {
                        durations.push(e.duration_since(s).unwrap_or_default());
                    }
                }
                CaseStatus::Failed => m.failed_cases += 1,
                _ => {}
            }

            for (task_id, instance) in &c.task_instances {
                let tm = m.task_metrics.entry(task_id.clone()).or_insert_with(|| TaskMetrics {
                    task_id: task_id.clone(),
                    ..Default::default()
                });
                match instance.status {
                    TaskStatus::Completed => {
                        tm.execution_count += 1;
                        tm.success_count += 1;
                    }
                    TaskStatus::Failed => {
                        tm.execution_count += 1;
                        tm.failure_count += 1;
                    }
                    _ => {}
                }
                tm.retry_count += instance.retry_count;
            }
        }

        if !durations.is_empty() {
            let total: Duration = durations.iter().sum();
            m.avg_case_duration = total / durations.len() as u32;
        }

        for (id, pool) in &self.resources {
            if pool.capacity > 0.0 {
                m.resource_utilization
                    .insert(id.clone(), 1.0 - (pool.available / pool.capacity));
            }
        }

        m
    }
}

fn elapsed_fraction(total: Duration, remaining: Duration) -> f64 {
    if total.is_zero() {
        return 1.0;
    }
    let elapsed = total.saturating_sub(remaining);
    elapsed.as_secs_f64() / total.as_secs_f64()
}

/// Ports `Engine.areDependenciesMet`.
fn dependencies_met(workflow: &Workflow, case: &Case, task_id: &str) -> bool {
    let task = &workflow.tasks[task_id];
    let incoming: Vec<&crate::types::Dependency> = workflow
        .dependencies
        .iter()
        .filter(|d| d.to_task_id == task_id)
        .collect();

    if incoming.is_empty() {
        return task_id == workflow.start_task_id;
    }

    let satisfied = incoming
        .iter()
        .filter(|dep| dependency_satisfied(case, dep))
        .count();

    match task.join_type {
        JoinType::All => satisfied == incoming.len(),
        JoinType::Any => satisfied > 0,
        JoinType::NOfM => satisfied >= task.join_count,
    }
}

/// Ports `Engine.isDependencySatisfied`. Dependency `Condition` closures are
/// evaluated the same as a task `Condition` (see `enable_ready_tasks`).
fn dependency_satisfied(case: &Case, dep: &crate::types::Dependency) -> bool {
    let Some(from_instance) = case.task_instances.get(&dep.from_task_id) else {
        return false;
    };
    if let Some(cond) = &dep.condition {
        let ctx = ExecutionContext {
            case,
            task_instance: from_instance,
            variables: &case.variables,
            now: SystemTime::now(),
        };
        if !cond(&ctx) {
            return false;
        }
    }
    match dep.typ {
        DependencyType::FinishToStart => {
            matches!(from_instance.status, TaskStatus::Completed | TaskStatus::Skipped)
        }
        DependencyType::StartToStart => matches!(
            from_instance.status,
            TaskStatus::Running | TaskStatus::Completed | TaskStatus::Skipped
        ),
        DependencyType::FinishToFinish | DependencyType::StartToFinish => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::WorkflowBuilder;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    fn linear_workflow() -> Workflow {
        let mut b = WorkflowBuilder::new("order");
        b.task("receive").manual();
        b.task("ship").manual();
        b.connect("receive", "ship");
        b.start_at("receive");
        b.end_at(&["ship"]);
        b.build()
    }

    #[test]
    fn start_case_enables_the_start_task() {
        let mut e = Engine::new(linear_workflow());
        e.start_case("c1", HashMap::new(), Priority::Medium).unwrap();
        let c = e.get_case("c1").unwrap();
        assert!(matches!(c.task_instances["receive"].status, TaskStatus::Ready));
        assert!(matches!(c.task_instances["ship"].status, TaskStatus::Pending));
    }

    #[test]
    fn completing_every_task_completes_the_case() {
        let mut e = Engine::new(linear_workflow());
        e.start_case("c1", HashMap::new(), Priority::Medium).unwrap();
        e.start_task("c1", "receive").unwrap();
        e.complete_task("c1", "receive", HashMap::new()).unwrap();

        let c = e.get_case("c1").unwrap();
        assert!(matches!(c.task_instances["ship"].status, TaskStatus::Ready));

        e.start_task("c1", "ship").unwrap();
        e.complete_task("c1", "ship", HashMap::new()).unwrap();

        let c = e.get_case("c1").unwrap();
        assert!(matches!(c.status, CaseStatus::Completed));
    }

    #[test]
    fn join_all_waits_for_every_predecessor() {
        let mut b = WorkflowBuilder::new("wf");
        b.task("a").manual();
        b.task("b").manual();
        b.task("c").manual().join_all();
        b.connect("a", "c");
        b.connect("b", "c");
        b.start_at("a");
        b.end_at(&["c"]);
        // "a" is the only start task, so seed "b" as ready via a synthetic
        // start; simplest is to drive both branches by hand.
        let wf = b.build();
        let mut e = Engine::new(wf);
        e.start_case("case", HashMap::new(), Priority::Medium).unwrap();
        e.start_task("case", "a").unwrap();
        e.complete_task("case", "a", HashMap::new()).unwrap();
        // "c" not ready yet: "b" never started (not reachable from "a" in
        // this deliberately disconnected graph), proving AND-join withholds
        // readiness rather than firing on the first predecessor.
        let c = e.get_case("case").unwrap();
        assert!(matches!(c.task_instances["c"].status, TaskStatus::Pending));
    }

    #[test]
    fn resource_acquire_release_round_trips() {
        let mut b = WorkflowBuilder::new("wf");
        b.resource("worker").capacity(1.0);
        b.task("t").requires("worker");
        b.start_at("t");
        b.end_at(&["t"]);
        let mut e = Engine::new(b.build());
        e.start_case("c1", HashMap::new(), Priority::Medium).unwrap();
        assert_eq!(e.get_resource_availability()["worker"], 1.0);

        e.start_task("c1", "t").unwrap();
        assert_eq!(e.get_resource_availability()["worker"], 0.0);

        e.complete_task("c1", "t", HashMap::new()).unwrap();
        assert_eq!(e.get_resource_availability()["worker"], 1.0);
    }

    #[test]
    fn insufficient_resources_refuse_start() {
        let mut b = WorkflowBuilder::new("wf");
        b.resource("worker").capacity(1.0);
        b.task("a").requires("worker");
        b.task("b").requires("worker");
        b.start_at("a");
        b.end_at(&["a"]);
        let mut e = Engine::new(b.build());
        e.start_case("c1", HashMap::new(), Priority::Medium).unwrap();
        e.start_task("c1", "a").unwrap();

        // "b" is never ready in this workflow (no edge from "a"), but we can
        // still exercise resource exhaustion directly via assign_task-style
        // acquisition on a second case sharing the same pool.
        let mut b2 = WorkflowBuilder::new("wf2");
        b2.resource("worker").capacity(0.0);
        b2.task("solo").requires("worker");
        b2.start_at("solo");
        b2.end_at(&["solo"]);
        let mut e2 = Engine::new(b2.build());
        e2.start_case("c1", HashMap::new(), Priority::Medium).unwrap();
        let err = e2.start_task("c1", "solo").unwrap_err();
        assert!(err.contains("insufficient"), "{err}");
    }

    #[test]
    fn failed_task_with_no_retries_aborts_the_case() {
        let wf = linear_workflow();
        let mut e = Engine::new(wf);
        e.start_case("c1", HashMap::new(), Priority::Medium).unwrap();
        e.start_task("c1", "receive").unwrap();
        e.fail_task("c1", "receive", "boom").unwrap();

        let c = e.get_case("c1").unwrap();
        assert!(matches!(c.status, CaseStatus::Failed));
    }

    #[test]
    fn failed_task_retries_before_aborting() {
        let mut b = WorkflowBuilder::new("wf");
        b.task("t").retry(1, Duration::ZERO);
        b.start_at("t");
        b.end_at(&["t"]);
        let mut e = Engine::new(b.build());
        e.start_case("c1", HashMap::new(), Priority::Medium).unwrap();
        e.start_task("c1", "t").unwrap();
        e.fail_task("c1", "t", "boom").unwrap();

        let c = e.get_case("c1").unwrap();
        assert!(matches!(c.task_instances["t"].status, TaskStatus::Ready));
        assert_eq!(c.task_instances["t"].retry_count, 1);
    }

    #[test]
    fn events_fire_with_owned_data() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let log2 = log.clone();
        let mut e = Engine::new(linear_workflow());
        e.on_event(move |ev| {
            if let EngineEvent::TaskReady { task_id, .. } = ev {
                log2.borrow_mut().push(task_id.clone());
            }
        });
        e.start_case("c1", HashMap::new(), Priority::Medium).unwrap();
        assert_eq!(*log.borrow(), vec!["receive".to_string()]);
    }

    #[test]
    fn cancel_case_releases_resources_and_cancels_tasks() {
        let mut b = WorkflowBuilder::new("wf");
        b.resource("worker").capacity(1.0);
        b.task("t").requires("worker");
        b.start_at("t");
        b.end_at(&["t"]);
        let mut e = Engine::new(b.build());
        e.start_case("c1", HashMap::new(), Priority::Medium).unwrap();
        e.start_task("c1", "t").unwrap();
        e.cancel_case("c1").unwrap();

        let c = e.get_case("c1").unwrap();
        assert!(matches!(c.status, CaseStatus::Cancelled));
        assert!(matches!(c.task_instances["t"].status, TaskStatus::Cancelled));
        assert_eq!(e.get_resource_availability()["worker"], 1.0);
    }
}
