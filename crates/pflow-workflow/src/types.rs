//! Workflow domain types, ported field-for-field from go-pflow's
//! `workflow/types.go`.
//!
//! Callbacks (`TaskCondition`, `TaskCallback`) are Rust closures, the same
//! adaptation `pflow-statemachine::types::Guard` makes — they have no
//! structural form and are evaluated by [`crate::engine::Engine`], not by
//! the compiled net (see `metasubnet.rs`'s module doc).

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Classifies tasks by execution model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskType {
    #[default]
    Manual,
    Automatic,
    Decision,
    Subflow,
}

/// The lifecycle state of a task instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskStatus {
    #[default]
    Pending,
    Ready,
    Assigned,
    Running,
    Completed,
    Failed,
    Skipped,
    Cancelled,
    TimedOut,
    Escalated,
}

/// How task dependencies work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DependencyType {
    #[default]
    FinishToStart,
    StartToStart,
    FinishToFinish,
    StartToFinish,
}

/// How multiple incoming dependencies are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JoinType {
    #[default]
    All,
    Any,
    NOfM,
}

/// How task completion triggers successors. Kept for structural parity with
/// go-pflow; nothing in the engine or `to_meta_model` currently branches on
/// it (go-pflow's own `ToPetriNet`/`ToMetaModel` don't either — every
/// dependency edge fires independently regardless of a task's `SplitType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplitType {
    #[default]
    All,
    Exclusive,
    Inclusive,
}

/// Priority levels for tasks and cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Priority {
    Critical = 0,
    High = 1,
    #[default]
    Medium = 2,
    Low = 3,
}

/// Evaluates whether a task should execute.
pub type TaskCondition = Box<dyn Fn(&ExecutionContext) -> bool>;
/// Called at task lifecycle events.
pub type TaskCallback = Box<dyn Fn(&ExecutionContext, &mut TaskInstance)>;

/// What to do when a task fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailureAction {
    #[default]
    None,
    Retry,
    Skip,
    Abort,
    Escalate,
    Compensate,
}

/// What a task needs from a resource pool.
#[derive(Debug, Clone)]
pub struct ResourceRequirement {
    pub resource_id: String,
    pub quantity: f64,
    pub exclusive: bool,
}

/// What a task produces into a resource pool on completion.
#[derive(Debug, Clone)]
pub struct ResourceProduction {
    pub resource_id: String,
    pub quantity: f64,
}

/// A unit of work.
#[derive(Default)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub description: String,
    pub typ: TaskType,

    pub estimated_duration: Duration,
    pub min_duration: Duration,
    pub max_duration: Duration,
    pub timeout: Duration,

    pub required_resources: Vec<ResourceRequirement>,
    pub produced_resources: Vec<ResourceProduction>,

    pub join_type: JoinType,
    pub join_count: usize,
    pub split_type: SplitType,

    pub max_retries: u32,
    pub retry_delay: Duration,
    pub failure_action: FailureAction,

    pub sla: Option<TaskSla>,
    pub condition: Option<TaskCondition>,

    pub on_start: Option<TaskCallback>,
    pub on_complete: Option<TaskCallback>,
    pub on_fail: Option<TaskCallback>,

    pub labels: HashMap<String, String>,
}

/// A connection between two tasks.
pub struct Dependency {
    pub from_task_id: String,
    pub to_task_id: String,
    pub typ: DependencyType,
    pub lag: Duration,
    pub condition: Option<TaskCondition>,
}

/// A constrained capacity pool.
#[derive(Debug, Clone, Default)]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub description: String,
    pub typ: ResourceType,

    pub capacity: f64,

    pub max_concurrent: u32,
    pub cost_per_unit: f64,
    pub cost_per_hour: f64,

    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResourceType {
    #[default]
    Worker,
    Equipment,
    System,
    License,
    Slot,
}

/// Service level agreement for a task.
#[derive(Debug, Clone)]
pub struct TaskSla {
    pub target_duration: Duration,
    pub warning_at: f64,
    pub critical_at: f64,
    pub breach_action: SlaBreachAction,
    pub escalate_to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlaBreachAction {
    #[default]
    Alert,
    Escalate,
    Reassign,
    Abort,
}

/// Service level agreement for an entire workflow.
#[derive(Debug, Clone, Default)]
pub struct WorkflowSla {
    pub by_priority: HashMap<Priority, Duration>,
    pub default: Duration,
    pub warning_at: f64,
    pub critical_at: f64,
}

impl std::hash::Hash for Priority {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (*self as i32).hash(state);
    }
}

/// A running instance of a task.
#[derive(Default)]
pub struct TaskInstance {
    pub id: String,
    pub task_id: String,
    pub case_id: String,
    pub status: TaskStatus,

    pub created_at: Option<SystemTime>,
    pub ready_at: Option<SystemTime>,
    pub started_at: Option<SystemTime>,
    pub completed_at: Option<SystemTime>,
    pub deadline: Option<SystemTime>,

    pub assigned_to: String,
    pub assigned_at: Option<SystemTime>,

    pub retry_count: u32,
    pub error: String,
    pub output: HashMap<String, String>,

    pub wait_duration: Duration,
    pub work_duration: Duration,
    pub total_duration: Duration,
}

/// A running workflow instance.
#[derive(Default)]
pub struct Case {
    pub id: String,
    pub workflow_id: String,
    pub priority: Priority,
    pub status: CaseStatus,

    pub created_at: Option<SystemTime>,
    pub started_at: Option<SystemTime>,
    pub completed_at: Option<SystemTime>,
    pub deadline: Option<SystemTime>,

    pub current_tasks: Vec<String>,
    pub completed_tasks: Vec<String>,
    pub task_instances: HashMap<String, TaskInstance>,

    pub input: HashMap<String, String>,
    pub output: HashMap<String, String>,
    pub variables: HashMap<String, String>,

    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseStatus {
    #[default]
    Created,
    Running,
    Completed,
    Failed,
    Cancelled,
    Suspended,
}

/// Runtime context for callbacks and conditions.
pub struct ExecutionContext<'a> {
    pub case: &'a Case,
    pub task_instance: &'a TaskInstance,
    pub variables: &'a HashMap<String, String>,
    pub now: SystemTime,
}

/// A workflow alert/notification.
#[derive(Debug, Clone)]
pub struct Alert {
    pub id: String,
    pub typ: AlertType,
    pub severity: AlertSeverity,
    pub case_id: String,
    pub task_id: String,
    pub message: String,
    pub created_at: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertType {
    SlaWarning,
    SlaBreach,
    TaskFailed,
    TaskTimeout,
    ResourceLow,
    CaseStuck,
    Deadlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// A complete workflow definition.
#[derive(Default)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,

    pub tasks: HashMap<String, Task>,
    pub dependencies: Vec<Dependency>,
    pub start_task_id: String,
    pub end_task_ids: Vec<String>,

    pub resources: HashMap<String, Resource>,

    pub sla: Option<WorkflowSla>,

    pub default_priority: Priority,
    pub default_timeout: Duration,

    pub labels: HashMap<String, String>,
}

/// Aggregates workflow performance data.
#[derive(Default)]
pub struct Metrics {
    pub total_cases: usize,
    pub active_cases: usize,
    pub completed_cases: usize,
    pub failed_cases: usize,

    pub avg_case_duration: Duration,

    pub task_metrics: HashMap<String, TaskMetrics>,

    pub sla_compliance: f64,
    pub sla_breaches: usize,

    pub resource_utilization: HashMap<String, f64>,
}

#[derive(Default)]
pub struct TaskMetrics {
    pub task_id: String,
    pub execution_count: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub retry_count: u32,
}

pub(crate) fn sorted_keys<V>(m: &HashMap<String, V>) -> Vec<String> {
    let mut out: Vec<String> = m.keys().cloned().collect();
    out.sort();
    out
}
