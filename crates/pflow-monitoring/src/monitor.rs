//! Ported from go-pflow's `monitoring/monitor.go`.
//!
//! go-pflow's `Start`/`Stop` run `periodicUpdate` on a `time.Ticker` in a
//! goroutine. This crate exposes [`Monitor::update_predictions`] and
//! [`Monitor::periodic_update`] as plain, callable-on-demand methods rather
//! than wrapping them in a background thread here: `pflow-engine::Engine`
//! already carries the "run a loop on an interval, stop it later" pattern
//! for this workspace (`std::thread` + a stop channel), and a
//! `Monitor`-owning caller can drive the same shape over these methods
//! without this crate needing a second, less test-friendly copy of it.

use crate::predictor::{predict_next_activity, predict_remaining_time};
use crate::types::{Alert, AlertSeverity, AlertType, Case, Monitor, Prediction};

impl Monitor {
    /// Begins monitoring a new case, starting at `start_time_ms`.
    pub fn start_case(&self, case_id: &str, start_time_ms: i64) -> Result<(), String> {
        let mut cases = self.cases.write().unwrap();
        if cases.contains_key(case_id) {
            return Err(format!("case {case_id} already exists"));
        }

        let case = Case {
            id: case_id.to_string(),
            start_time_ms,
            last_event_time_ms: start_time_ms,
            ..Default::default()
        };
        cases.insert(case_id.to_string(), case);
        drop(cases);

        self.stats.lock().unwrap().total_cases += 1;
        Ok(())
    }

    /// Records a new event for a case and updates predictions.
    pub fn record_event(
        &self,
        case_id: &str,
        activity: &str,
        timestamp_ms: i64,
        resource: &str,
        now_ms: i64,
    ) -> Result<(), String> {
        let mut cases = self.cases.write().unwrap();
        let case = cases
            .get_mut(case_id)
            .ok_or_else(|| format!("case {case_id} not found"))?;

        case.history.push(crate::types::Event {
            case_id: case_id.to_string(),
            activity: activity.to_string(),
            timestamp_ms,
            resource: resource.to_string(),
        });
        case.current_activity = activity.to_string();
        case.last_event_time_ms = timestamp_ms;

        if self.config.enable_alerts {
            // Faithfully ported from go-pflow, bug and all:
            // `c.LastEventTime` was just set to `timestamp` two lines
            // above, so this is comparing `now` against the event that
            // just arrived, not against the *previous* one — the "stuck"
            // check can only ever see a near-zero gap. Fixing it would
            // need `last_event_time_ms` read before the update above,
            // which changes observable alerting behaviour; left as go-pflow
            // has it, since this is a port, not an independent redesign.
            let since_last_event = now_ms - case.last_event_time_ms;
            if since_last_event > self.config.stuck_threshold_ms {
                let alert = Alert {
                    timestamp_ms: now_ms,
                    case_id: case_id.to_string(),
                    typ: AlertType::Stuck,
                    severity: AlertSeverity::Warning,
                    message: format!(
                        "Case has been inactive for {} min",
                        since_last_event / 60_000
                    ),
                    prediction: None,
                };
                drop(cases);
                self.trigger_alert(alert);
                cases = self.cases.write().unwrap();
            }
        }

        if self.config.enable_predictions {
            if let Some(case) = cases.get(case_id).cloned() {
                drop(cases);
                self.update_predictions(&case, now_ms);
                return Ok(());
            }
        }

        Ok(())
    }

    /// Marks a case as completed and removes it from active tracking.
    pub fn complete_case(&self, case_id: &str, completion_time_ms: i64) -> Result<(), String> {
        let mut cases = self.cases.write().unwrap();
        let case = cases
            .get(case_id)
            .ok_or_else(|| format!("case {case_id} not found"))?
            .clone();

        self.stats.lock().unwrap().completed_cases += 1;

        if let Some(pred) = &case.predictions {
            let predicted_time = pred.expected_completion_ms;
            let error = (completion_time_ms - predicted_time).abs();
            let threshold = pred.remaining_ms / 10;
            if error < threshold {
                // Placeholder, matching go-pflow's own "would be updated
                // properly with a moving average" comment.
                self.stats.lock().unwrap().prediction_accuracy = 0.8;
            }
        }

        cases.remove(case_id);
        Ok(())
    }

    /// Updates predictions for `case`, using simulation, and triggers SLA
    /// alerts as go-pflow's `updatePredictions` does. Takes an
    /// already-read `Case` snapshot rather than re-locking `self.cases`
    /// internally, so callers holding a write lock (like
    /// [`Monitor::record_event`]) can drop it first and avoid a
    /// self-deadlock — Rust's `RwLock` is not reentrant, unlike Go's
    /// pattern of a single method-local `m.mu.Lock()`.
    pub fn update_predictions(&self, case: &Case, now_ms: i64) {
        let (remaining_ms, confidence) = predict_remaining_time(case, &self.predictor, now_ms);
        let next_activities = predict_next_activity(case, &self.predictor);

        let mut prediction = Prediction {
            computed_at_ms: now_ms,
            remaining_ms,
            expected_completion_ms: now_ms + remaining_ms,
            confidence,
            next_activities,
            risk_score: 0.0,
        };

        if self.config.sla_threshold_ms > 0 {
            let elapsed = now_ms - case.start_time_ms;
            let total_expected = elapsed + remaining_ms;
            if total_expected > self.config.sla_threshold_ms {
                prediction.risk_score = 0.9;
                if self.config.enable_alerts {
                    self.trigger_alert(Alert {
                        timestamp_ms: now_ms,
                        case_id: case.id.clone(),
                        typ: AlertType::SlaViolation,
                        severity: AlertSeverity::Critical,
                        message: format!(
                            "Predicted completion ({} ms) exceeds SLA threshold ({} ms)",
                            total_expected, self.config.sla_threshold_ms
                        ),
                        prediction: Some(prediction.clone()),
                    });
                }
            } else {
                let ratio = total_expected as f64 / self.config.sla_threshold_ms as f64;
                prediction.risk_score = ratio;
                if ratio > 0.8 && self.config.enable_alerts {
                    self.trigger_alert(Alert {
                        timestamp_ms: now_ms,
                        case_id: case.id.clone(),
                        typ: AlertType::Delayed,
                        severity: AlertSeverity::Warning,
                        message: format!("Case at risk: {:.0}% of SLA threshold used", ratio * 100.0),
                        prediction: Some(prediction.clone()),
                    });
                }
            }
        }

        if let Some(c) = self.cases.write().unwrap().get_mut(&case.id) {
            c.predictions = Some(prediction);
        }
    }

    /// The latest prediction for a case, computing a new one if stale or
    /// missing.
    pub fn predict_completion(&self, case_id: &str, now_ms: i64) -> Result<Option<Prediction>, String> {
        let case = {
            let cases = self.cases.read().unwrap();
            cases
                .get(case_id)
                .cloned()
                .ok_or_else(|| format!("case {case_id} not found"))?
        };

        let stale = match &case.predictions {
            None => true,
            Some(p) => now_ms - p.computed_at_ms > self.config.prediction_interval_ms,
        };
        if stale {
            self.update_predictions(&case, now_ms);
        }

        Ok(self.cases.read().unwrap().get(case_id).and_then(|c| c.predictions.clone()))
    }

    /// Updates predictions for every active case, matching go-pflow's
    /// `periodicUpdate` — the loop a caller drives on a timer (see the
    /// module doc).
    pub fn periodic_update(&self, now_ms: i64) {
        if !self.config.enable_predictions {
            return;
        }
        let case_ids: Vec<String> = self.cases.read().unwrap().keys().cloned().collect();
        for id in case_ids {
            if let Some(case) = self.cases.read().unwrap().get(&id).cloned() {
                self.update_predictions(&case, now_ms);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MonitorConfig;
    use pflow_core::net::PetriNet;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn workflow_net() -> PetriNet {
        PetriNet::build()
            .place("start", 0.0)
            .place("end", 0.0)
            .transition("finish")
            .arc("start", "finish", 1.0)
            .arc("finish", "end", 1.0)
            .done()
    }

    #[test]
    fn start_case_refuses_a_duplicate_id() {
        let monitor = Monitor::new(workflow_net(), HashMap::new(), MonitorConfig::default());
        monitor.start_case("c1", 0).unwrap();
        assert!(monitor.start_case("c1", 0).is_err());
        assert_eq!(monitor.get_statistics().total_cases, 1);
    }

    #[test]
    fn record_event_updates_history_and_predictions() {
        let mut rates = HashMap::new();
        rates.insert("finish".to_string(), 1.0);
        let monitor = Monitor::new(workflow_net(), rates, MonitorConfig::default());
        monitor.start_case("c1", 0).unwrap();
        monitor.record_event("c1", "finish", 1000, "", 1000).unwrap();

        let case = monitor.get_case("c1").unwrap();
        assert_eq!(case.history.len(), 1);
        assert_eq!(case.current_activity, "finish");
        assert!(case.predictions.is_some());
    }

    #[test]
    fn record_event_on_an_unknown_case_errors() {
        let monitor = Monitor::new(workflow_net(), HashMap::new(), MonitorConfig::default());
        assert!(monitor.record_event("nope", "finish", 0, "", 0).is_err());
    }

    #[test]
    fn complete_case_removes_it_from_active_tracking() {
        let monitor = Monitor::new(workflow_net(), HashMap::new(), MonitorConfig::default());
        monitor.start_case("c1", 0).unwrap();
        monitor.complete_case("c1", 1000).unwrap();
        assert!(monitor.get_case("c1").is_none());
        assert_eq!(monitor.get_statistics().completed_cases, 1);
    }

    #[test]
    fn sla_violation_triggers_a_critical_alert() {
        let config = MonitorConfig {
            sla_threshold_ms: 1, // trivially violated
            ..Default::default()
        };
        let monitor = Monitor::new(workflow_net(), HashMap::new(), config);
        monitor.start_case("c1", 0).unwrap();

        let fired: Arc<Mutex<Vec<AlertType>>> = Arc::new(Mutex::new(Vec::new()));
        let fired_clone = Arc::clone(&fired);
        monitor.add_alert_handler(Box::new(move |alert| {
            fired_clone.lock().unwrap().push(alert.typ);
        }));

        monitor.record_event("c1", "finish", 100, "", 100).unwrap();

        assert!(fired.lock().unwrap().contains(&AlertType::SlaViolation));
        assert_eq!(monitor.get_statistics().total_alerts, 1);
    }

    #[test]
    fn predict_completion_recomputes_when_stale() {
        let mut rates = HashMap::new();
        rates.insert("finish".to_string(), 1.0);
        let config = MonitorConfig {
            prediction_interval_ms: -1, // always stale
            ..Default::default()
        };
        let monitor = Monitor::new(workflow_net(), rates, config);
        monitor.start_case("c1", 0).unwrap();

        let pred = monitor.predict_completion("c1", 1000).unwrap();
        assert!(pred.is_some());
    }
}
