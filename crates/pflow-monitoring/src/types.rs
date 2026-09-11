//! Ported from go-pflow's `monitoring/types.go`.
//!
//! Timestamps are `i64` Unix-epoch milliseconds throughout rather than
//! `time.Time`: this crate has no clock dependency (matching
//! `pflow-eventsource`'s own choice), so callers supply "now" explicitly —
//! every function that reads it in go-pflow (`RecordEvent`,
//! `updatePredictions`, `StartCase`) takes it as a parameter here instead
//! of calling `time.Now()`/`time.Since()` internally.

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

use pflow_core::net::PetriNet;

use crate::predictor::Predictor;

/// An active process instance being monitored.
#[derive(Debug, Clone, Default)]
pub struct Case {
    pub id: String,
    pub start_time_ms: i64,
    pub current_activity: String,
    pub last_event_time_ms: i64,
    pub state: HashMap<String, f64>,
    pub history: Vec<Event>,
    /// go-pflow's `Attributes` is `map[string]interface{}`; this crate has
    /// no dynamic-JSON dependency of its own (unlike `pflow-eventsource`,
    /// which needs one for event payloads), so this is narrowed to string
    /// values. Nothing in `Monitor`/`Predictor` reads this field — it is
    /// carried for callers, exactly as it is in go-pflow.
    pub attributes: HashMap<String, String>,
    pub predictions: Option<Prediction>,
}

/// A single event in a case's history.
#[derive(Debug, Clone, Default)]
pub struct Event {
    pub case_id: String,
    pub activity: String,
    pub timestamp_ms: i64,
    pub resource: String,
}

/// Predicted outcomes for a case.
#[derive(Debug, Clone, Default)]
pub struct Prediction {
    pub computed_at_ms: i64,
    pub expected_completion_ms: i64,
    pub remaining_ms: i64,
    pub confidence: f64,
    pub next_activities: Vec<NextActivity>,
    pub risk_score: f64,
}

/// A predicted next activity.
#[derive(Debug, Clone, Default)]
pub struct NextActivity {
    pub activity: String,
    pub probability: f64,
    pub expected_time_ms: i64,
}

/// A triggered alert condition.
#[derive(Debug, Clone)]
pub struct Alert {
    pub timestamp_ms: i64,
    pub case_id: String,
    pub typ: AlertType,
    pub severity: AlertSeverity,
    pub message: String,
    pub prediction: Option<Prediction>,
}

impl std::fmt::Display for Alert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{:?}] {:?} - Case {}: {}",
            self.severity, self.typ, self.case_id, self.message
        )
    }
}

/// Categorizes alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertType {
    SlaViolation,
    Delayed,
    Stuck,
    UnexpectedPath,
    ResourceIssue,
}

/// Alert importance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// Called when an alert is triggered. go-pflow calls handlers in their own
/// goroutine (non-blocking); [`Monitor::trigger_alert`] calls them
/// synchronously — see that function's doc for why.
pub type AlertHandler = Box<dyn Fn(&Alert) + Send + Sync>;

/// Configures the monitoring system.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub prediction_interval_ms: i64,
    pub sla_threshold_ms: i64,
    pub stuck_threshold_ms: i64,
    pub confidence_level: f64,
    pub enable_predictions: bool,
    pub enable_alerts: bool,
}

impl Default for MonitorConfig {
    /// Sensible defaults, matching go-pflow's `DefaultMonitorConfig`
    /// (1 minute prediction interval, 4 hour SLA — "a common ER SLA" per
    /// the source comment — 30 minute stuck threshold, 0.7 confidence).
    fn default() -> Self {
        Self {
            prediction_interval_ms: 60_000,
            sla_threshold_ms: 4 * 60 * 60 * 1000,
            stuck_threshold_ms: 30 * 60 * 1000,
            confidence_level: 0.7,
            enable_predictions: true,
            enable_alerts: true,
        }
    }
}

/// Monitoring performance metrics.
#[derive(Debug, Clone, Default)]
pub struct Statistics {
    pub total_cases: usize,
    pub active_cases: usize,
    pub completed_cases: usize,
    pub total_alerts: usize,
    pub alerts_by_severity: HashMap<String, usize>,
    pub alerts_by_type: HashMap<String, usize>,
    pub prediction_accuracy: f64,
}

/// The main process monitoring engine.
pub struct Monitor {
    /// Carried for parity with go-pflow's `Monitor.rates`, which likewise
    /// exists only to have been the argument `NewPredictor` was built from
    /// — nothing in either language reads it back off the struct.
    #[allow(dead_code)]
    pub(crate) rates: HashMap<String, f64>,
    pub(crate) config: MonitorConfig,
    pub(crate) predictor: Predictor,
    pub(crate) cases: RwLock<HashMap<String, Case>>,
    pub(crate) handlers: Mutex<Vec<AlertHandler>>,
    pub(crate) stats: Mutex<Statistics>,
}

impl Monitor {
    /// Creates a new process monitor with learned model parameters.
    pub fn new(net: PetriNet, rates: HashMap<String, f64>, config: MonitorConfig) -> Self {
        let predictor = Predictor::new(net, rates.clone());
        Self {
            rates,
            config,
            predictor,
            cases: RwLock::new(HashMap::new()),
            handlers: Mutex::new(Vec::new()),
            stats: Mutex::new(Statistics::default()),
        }
    }

    /// Registers a function to be called on alerts.
    pub fn add_alert_handler(&self, handler: AlertHandler) {
        self.handlers.lock().unwrap().push(handler);
    }

    /// Retrieves a case by id.
    pub fn get_case(&self, case_id: &str) -> Option<Case> {
        self.cases.read().unwrap().get(case_id).cloned()
    }

    /// All currently active cases.
    pub fn get_active_cases(&self) -> Vec<Case> {
        self.cases.read().unwrap().values().cloned().collect()
    }

    /// Current monitoring statistics.
    pub fn get_statistics(&self) -> Statistics {
        let mut stats = self.stats.lock().unwrap().clone();
        stats.active_cases = self.cases.read().unwrap().len();
        stats
    }

    /// Sends an alert to every registered handler.
    ///
    /// go-pflow calls each handler in its own goroutine (`go handler(alert)`,
    /// "Non-blocking"). This crate calls handlers synchronously on the
    /// caller's thread instead: a handler here is a plain closure with no
    /// async runtime backing it (unlike a goroutine, which is cheap and
    /// scheduler-managed), so spawning an OS thread per alert would be a
    /// much heavier and behaviourally different substitution rather than an
    /// equivalent port. A caller that needs non-blocking delivery can make
    /// its own handler do so (e.g. push onto a channel and return).
    pub(crate) fn trigger_alert(&self, alert: Alert) {
        {
            let mut stats = self.stats.lock().unwrap();
            stats.total_alerts += 1;
            *stats
                .alerts_by_severity
                .entry(format!("{:?}", alert.severity))
                .or_insert(0) += 1;
            *stats
                .alerts_by_type
                .entry(format!("{:?}", alert.typ))
                .or_insert(0) += 1;
        }
        for handler in self.handlers.lock().unwrap().iter() {
            handler(&alert);
        }
    }
}
