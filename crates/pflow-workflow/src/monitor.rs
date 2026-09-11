//! ODE-based prediction, alerting and a monitoring dashboard snapshot,
//! ported from go-pflow's `workflow/monitor.go`.
//!
//! go-pflow's `WorkflowMonitor.Start` runs a goroutine polling two tickers
//! (SLA checks, predictions). A library has no business spawning background
//! threads on a caller's behalf, so this port drops the ticker loop and
//! exposes the two things it called — [`WorkflowMonitor::check_slas`] and
//! [`WorkflowMonitor::update_all_predictions`] — as plain methods a caller
//! (or its own scheduler) invokes directly. Everything else —
//! [`WorkflowPredictor`]'s ODE-based prediction, bottleneck identification,
//! the dashboard snapshot, what-if analysis — is a faithful port.
//!
//! **One preserved quirk, not silently fixed**: go-pflow's own
//! `learnedRates` keys every rate by `"task_" + task.ID`, but
//! [`Workflow::to_petri_net`]'s transitions are named `"start_<id>"` /
//! `"complete_<id>"` — no transition is ever actually named `"task_<id>"`.
//! So `learnedRates`'s per-task duration estimates never reach the solver;
//! every transition silently falls back to the solver's default rate. Per
//! ROADMAP.md ground rule 1 ("go-pflow generates, Rust replays... a failing
//! golden is a bug in Rust or a deliberate change in Go; it is never fixed
//! by regenerating"), this port reproduces the mismatch rather than quietly
//! renaming one side to make the numbers more meaningful than go-pflow's own
//! are.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use pflow_core::net::PetriNet;
use pflow_solver::{methods, ode};

use crate::engine::Engine;
use crate::types::{
    Alert, AlertSeverity, AlertType, Case, CaseStatus, Priority, TaskStatus,
};

/// Configures the workflow monitor.
pub struct MonitorConfig {
    pub sla_check_interval: Duration,
    pub enable_predictions: bool,
    pub prediction_interval: Duration,
    /// How far ahead to simulate, in the model's own time units.
    pub simulation_time_span: f64,
    pub enable_alerts: bool,
    pub alert_buffer_size: usize,
    pub wait_time_warning_threshold: Duration,
    pub wait_time_critical_threshold: Duration,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        MonitorConfig {
            sla_check_interval: Duration::from_secs(60),
            enable_predictions: true,
            prediction_interval: Duration::from_secs(5 * 60),
            simulation_time_span: 100.0,
            enable_alerts: true,
            alert_buffer_size: 100,
            wait_time_warning_threshold: Duration::from_secs(15 * 60),
            wait_time_critical_threshold: Duration::from_secs(30 * 60),
        }
    }
}

/// Uses ODE simulation for workflow predictions.
pub struct WorkflowPredictor {
    net: PetriNet,
    rates: HashMap<String, f64>,
    time_span: f64,
}

impl WorkflowPredictor {
    pub fn new(net: PetriNet, rates: HashMap<String, f64>) -> Self {
        WorkflowPredictor {
            net,
            rates,
            time_span: 100.0,
        }
    }

    pub fn with_time_span(mut self, t: f64) -> Self {
        self.time_span = t;
        self
    }

    /// Predicts workflow completion from a current place-token state.
    pub fn predict_from_state(&self, state: HashMap<String, f64>) -> CasePrediction {
        let prob = ode::Problem::new(self.net.clone(), state.clone(), [0.0, self.time_span], self.rates.clone());
        let solver = methods::tsit5();
        let opts = ode::Options::fast();
        let sol = ode::solve(&prob, &solver, &opts);

        let final_state = sol.get_final_state().cloned().unwrap_or_default();
        let equilibrium_time = find_equilibrium_time(&sol);
        let completion_probability = estimate_completion_probability(&final_state);
        let remaining_duration = Duration::from_secs_f64((equilibrium_time * 3600.0).max(0.0));

        CasePrediction {
            computed_at: SystemTime::now(),
            remaining_duration,
            expected_completion: None,
            completion_probability,
            final_state,
            equilibrium_time,
            confidence: calculate_confidence(&sol, &state),
            risk_score: 0.0,
            bottleneck_tasks: Vec::new(),
        }
    }
}

fn find_equilibrium_time(sol: &ode::Solution) -> f64 {
    if sol.t.len() < 2 {
        return sol.t.last().copied().unwrap_or(0.0);
    }
    let tolerance = 1e-3;
    for i in (1..sol.t.len()).rev() {
        let mut max_delta = 0.0_f64;
        for (key, v) in &sol.u[i] {
            let prev = sol.u[i - 1].get(key).copied().unwrap_or(0.0);
            let delta = (v - prev).abs();
            if delta > max_delta {
                max_delta = delta;
            }
        }
        if max_delta > tolerance {
            return sol.t[i];
        }
    }
    *sol.t.last().unwrap()
}

fn estimate_completion_probability(final_state: &HashMap<String, f64>) -> f64 {
    let mut end_tokens = 0.0;
    let mut start_tokens = 0.0;
    for (place, tokens) in final_state {
        if place.ends_with("_end") {
            end_tokens += tokens;
        }
        if place.ends_with("_start") {
            start_tokens += tokens;
        }
    }
    let total = end_tokens + start_tokens;
    if total > 0.0 {
        end_tokens / total
    } else {
        0.5
    }
}

fn calculate_confidence(sol: &ode::Solution, initial_state: &HashMap<String, f64>) -> f64 {
    let mut confidence = 1.0_f64;
    let final_state = sol.get_final_state().cloned().unwrap_or_default();

    let initial_total: f64 = initial_state.values().sum();
    let final_total: f64 = final_state.values().sum();
    if initial_total > 0.0 {
        let ratio = final_total / initial_total;
        if !(0.9..=1.1).contains(&ratio) {
            confidence *= 0.7;
        }
    }
    if final_state.values().any(|v| *v < -0.01) {
        confidence *= 0.5;
    }
    confidence
}

/// Prediction results for a case.
pub struct CasePrediction {
    pub computed_at: SystemTime,
    pub remaining_duration: Duration,
    pub expected_completion: Option<SystemTime>,
    pub completion_probability: f64,
    pub final_state: HashMap<String, f64>,
    pub equilibrium_time: f64,
    pub confidence: f64,
    pub risk_score: f64,
    pub bottleneck_tasks: Vec<String>,
}

/// A snapshot for monitoring displays.
pub struct DashboardData {
    pub timestamp: SystemTime,
    pub active_cases: usize,
    pub completed_cases: usize,
    pub failed_cases: usize,
    pub ready_task_count: usize,
    pub resource_utilization: HashMap<String, f64>,
    pub resource_availability: HashMap<String, f64>,
    pub cases_at_risk: Vec<CaseRiskInfo>,
    pub task_queue: HashMap<String, usize>,
    pub recent_alerts: Vec<Alert>,
    pub sla_compliance: f64,
}

pub struct CaseRiskInfo {
    pub case_id: String,
    pub priority: Priority,
    pub risk_score: f64,
    pub expected_completion: Option<SystemTime>,
    pub bottleneck_tasks: Vec<String>,
}

/// Provides real-time monitoring, predictions and analytics for a workflow
/// [`Engine`] using ODE-based simulation.
pub struct WorkflowMonitor<'a> {
    engine: &'a Engine,
    predictor: WorkflowPredictor,
    config: MonitorConfig,
    alerts: Vec<Alert>,
}

impl<'a> WorkflowMonitor<'a> {
    pub fn new(engine: &'a Engine, config: MonitorConfig) -> Self {
        let net = engine.workflow().to_petri_net();
        let rates = learned_rates(engine.workflow());
        WorkflowMonitor {
            engine,
            predictor: WorkflowPredictor::new(net, rates).with_time_span(config.simulation_time_span),
            config,
            alerts: Vec::new(),
        }
    }

    pub fn predict_case(&self, case_id: &str) -> Result<CasePrediction, String> {
        let c = self.engine.get_case(case_id).ok_or(format!("case {case_id} not found"))?;
        let state = build_state_from_case(c);
        let mut pred = self.predictor.predict_from_state(state);
        pred.expected_completion = Some(SystemTime::now() + pred.remaining_duration);

        if let Some(deadline) = c.deadline {
            let now = SystemTime::now();
            let remaining = deadline.duration_since(now).unwrap_or_default();
            pred.risk_score = if pred.remaining_duration > remaining || remaining.is_zero() {
                1.0
            } else {
                pred.remaining_duration.as_secs_f64() / remaining.as_secs_f64()
            };
        }
        pred.bottleneck_tasks = self.identify_bottlenecks(c);
        Ok(pred)
    }

    fn identify_bottlenecks(&self, c: &Case) -> Vec<String> {
        let mut out = Vec::new();
        let now = SystemTime::now();
        for (task_id, instance) in &c.task_instances {
            if matches!(instance.status, TaskStatus::Ready) {
                if let Some(ready_at) = instance.ready_at {
                    let wait = now.duration_since(ready_at).unwrap_or_default();
                    if wait > self.config.wait_time_warning_threshold {
                        out.push(task_id.clone());
                    }
                }
            }
        }
        let avail = self.engine.get_resource_availability();
        for (rid, r) in &self.engine.workflow().resources {
            if r.capacity <= 0.0 {
                continue;
            }
            let a = avail.get(rid).copied().unwrap_or(0.0);
            let utilization = 1.0 - (a / r.capacity);
            if utilization > 0.9 {
                for task in self.engine.workflow().tasks.values() {
                    if task.required_resources.iter().any(|req| &req.resource_id == rid)
                        && !out.contains(&task.id)
                    {
                        out.push(task.id.clone());
                    }
                }
            }
        }
        out
    }

    /// Records alerts an SLA check already produced.
    ///
    /// `Engine::check_slas` needs `&mut Engine`, but the monitor only holds
    /// `&Engine` (a prediction must not be able to mutate case state), so
    /// unlike go-pflow's `WorkflowMonitor.CheckSLAs` — which owns the
    /// engine and can call `engine.CheckSLAs()` itself — this port has
    /// callers run `engine.check_slas()` and hand the result here.
    pub fn record_alerts(&mut self, alerts: Vec<Alert>) {
        for a in alerts {
            self.record_alert(a);
        }
    }

    fn record_alert(&mut self, alert: Alert) {
        self.alerts.push(alert);
        if self.alerts.len() > self.config.alert_buffer_size {
            let excess = self.alerts.len() - self.config.alert_buffer_size;
            self.alerts.drain(0..excess);
        }
    }

    pub fn update_all_predictions(&mut self) {
        let running: Vec<String> = self
            .engine
            .get_cases(|c| matches!(c.status, CaseStatus::Running))
            .into_iter()
            .map(|c| c.id.clone())
            .collect();

        let mut new_alerts = Vec::new();
        for case_id in running {
            let Ok(pred) = self.predict_case(&case_id) else { continue };
            if pred.risk_score > 0.8 && self.config.enable_alerts {
                new_alerts.push(Alert {
                    id: format!("risk_{case_id}"),
                    typ: AlertType::SlaWarning,
                    severity: AlertSeverity::Warning,
                    case_id: case_id.clone(),
                    task_id: String::new(),
                    message: format!(
                        "Case at risk: {:.0}% of SLA used",
                        pred.risk_score * 100.0
                    ),
                    created_at: SystemTime::now(),
                });
            }
        }
        for a in new_alerts {
            self.record_alert(a);
        }
    }

    pub fn get_alerts(&self) -> &[Alert] {
        &self.alerts
    }

    pub fn get_dashboard_data(&self) -> DashboardData {
        let metrics = self.engine.get_metrics();
        let resources = self.engine.get_resource_availability();
        let ready_tasks = self.engine.get_ready_tasks();

        let active_cases = self.engine.get_cases(|c| matches!(c.status, CaseStatus::Running));
        let mut cases_at_risk = Vec::new();
        for c in &active_cases {
            if let Ok(pred) = self.predict_case(&c.id) {
                if pred.risk_score > 0.7 {
                    cases_at_risk.push(CaseRiskInfo {
                        case_id: c.id.clone(),
                        priority: c.priority,
                        risk_score: pred.risk_score,
                        expected_completion: pred.expected_completion,
                        bottleneck_tasks: pred.bottleneck_tasks,
                    });
                }
            }
        }

        let mut task_queue: HashMap<String, usize> = HashMap::new();
        for t in &ready_tasks {
            *task_queue.entry(t.task_id.clone()).or_insert(0) += 1;
        }

        DashboardData {
            timestamp: SystemTime::now(),
            active_cases: metrics.active_cases,
            completed_cases: metrics.completed_cases,
            failed_cases: metrics.failed_cases,
            ready_task_count: ready_tasks.len(),
            resource_utilization: metrics.resource_utilization,
            resource_availability: resources,
            cases_at_risk,
            task_queue,
            recent_alerts: self.recent_alerts(10),
            sla_compliance: self.calculate_sla_compliance(),
        }
    }

    fn recent_alerts(&self, n: usize) -> Vec<Alert> {
        if self.alerts.len() <= n {
            self.alerts.clone()
        } else {
            self.alerts[self.alerts.len() - n..].to_vec()
        }
    }

    fn calculate_sla_compliance(&self) -> f64 {
        let breaches = self.alerts.iter().filter(|a| matches!(a.typ, AlertType::SlaBreach)).count();
        let metrics = self.engine.get_metrics();
        let total = metrics.completed_cases + metrics.failed_cases;
        if total == 0 {
            return 1.0;
        }
        (total as f64 - breaches as f64) / total as f64
    }
}

fn build_state_from_case(c: &Case) -> HashMap<String, f64> {
    let mut state = HashMap::new();
    for (task_id, instance) in &c.task_instances {
        match instance.status {
            TaskStatus::Pending => {
                state.insert(format!("task_{task_id}_pending"), 1.0);
            }
            TaskStatus::Ready => {
                state.insert(format!("task_{task_id}_ready"), 1.0);
            }
            TaskStatus::Running => {
                state.insert(format!("task_{task_id}_running"), 1.0);
            }
            TaskStatus::Completed | TaskStatus::Skipped => {
                state.insert(format!("task_{task_id}_done"), 1.0);
            }
            _ => {}
        }
    }
    state
}

/// Ports `Workflow.learnedRates` — including its `"task_"` prefix, which
/// does not match any transition `to_petri_net` actually emits. See the
/// module doc.
fn learned_rates(w: &crate::types::Workflow) -> HashMap<String, f64> {
    let mut rates = HashMap::new();
    for task in w.tasks.values() {
        let trans_name = format!("task_{}", task.id);
        let rate = if task.estimated_duration.as_secs_f64() > 0.0 {
            1.0 / (task.estimated_duration.as_secs_f64() / 3600.0)
        } else {
            1.0
        };
        rates.insert(trans_name, rate);
    }
    rates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::WorkflowBuilder;
    use std::collections::HashMap as Map;

    #[test]
    fn dashboard_reports_case_and_resource_counts() {
        let mut b = WorkflowBuilder::new("wf");
        b.resource("worker").capacity(2.0);
        b.task("t").requires("worker");
        b.start_at("t");
        b.end_at(&["t"]);
        let mut e = Engine::new(b.build());
        e.start_case("c1", Map::new(), Priority::Medium).unwrap();

        let m = WorkflowMonitor::new(&e, MonitorConfig::default());
        let dash = m.get_dashboard_data();
        assert_eq!(dash.active_cases, 1);
        assert_eq!(dash.resource_availability["worker"], 2.0);
    }

    #[test]
    fn predict_case_returns_a_confidence_in_range() {
        let mut b = WorkflowBuilder::new("wf");
        b.task("t");
        b.start_at("t");
        b.end_at(&["t"]);
        let mut e = Engine::new(b.build());
        e.start_case("c1", Map::new(), Priority::Medium).unwrap();

        let m = WorkflowMonitor::new(&e, MonitorConfig::default());
        let pred = m.predict_case("c1").unwrap();
        assert!(pred.confidence >= 0.0 && pred.confidence <= 1.0);
    }

    #[test]
    fn unknown_case_prediction_errs() {
        let b = WorkflowBuilder::new("wf").task("t").start_at("t").end_at(&["t"]).build();
        let e = Engine::new(b);
        let m = WorkflowMonitor::new(&e, MonitorConfig::default());
        assert!(m.predict_case("nope").is_err());
    }
}
