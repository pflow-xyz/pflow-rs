//! `petri_conformance`: replays a real event log against a model and
//! reports fitness (can the model reproduce what happened?) and precision
//! (does the model allow behaviour never observed?), via
//! [`pflow_mining::conformance`]. Mirrors petri-pilot's
//! `pkg/mcp/conformance.go`.

use super::convert::{model_to_petri_net, parse_any_model};
use pflow_eventlog::{Event, EventLog};
use pflow_mining::conformance::check_full_conformance;
use serde::Serialize;

#[derive(Serialize)]
struct ConformanceOut {
    fitness: f64,
    precision: f64,
    f_score: f64,
    total_traces: usize,
    fitting_traces: usize,
    fitting_percent: f64,
    missing_tokens: i64,
    remaining_tokens: i64,
    escaping_edges: usize,
    total_enabled: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    non_fitting_traces: Vec<TraceOut>,
}

#[derive(Serialize)]
struct TraceOut {
    case_id: String,
    fitness: f64,
    missing_activities: Vec<String>,
}

/// One log event, tolerant of the same key aliases petri-pilot accepts:
/// `case`/`caseId`/`case_id`/`trace`, `activity`/`action`/`task`/`event`/`transition`.
#[derive(serde::Deserialize)]
struct RawEvent {
    #[serde(default)]
    case: String,
    #[serde(default, rename = "caseId")]
    case_id_camel: String,
    #[serde(default)]
    case_id: String,
    #[serde(default)]
    trace: String,
    #[serde(default)]
    activity: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    task: String,
    #[serde(default)]
    event: String,
    #[serde(default)]
    transition: String,
    #[serde(default)]
    resource: String,
}

impl RawEvent {
    fn case(&self) -> String {
        [&self.case, &self.case_id_camel, &self.case_id, &self.trace]
            .into_iter()
            .find(|s| !s.is_empty())
            .cloned()
            .unwrap_or_default()
    }
    fn activity(&self) -> String {
        [&self.activity, &self.action, &self.task, &self.event, &self.transition]
            .into_iter()
            .find(|s| !s.is_empty())
            .cloned()
            .unwrap_or_default()
    }
}

fn parse_log(json: &str) -> Result<EventLog, String> {
    let raw: Vec<RawEvent> = serde_json::from_str(json).map_err(|e| format!("invalid log JSON: {e}"))?;
    let mut log = EventLog::new();
    for (i, r) in raw.iter().enumerate() {
        let case = r.case();
        let activity = r.activity();
        if case.is_empty() || activity.is_empty() {
            return Err(format!(
                "event {i} needs a case id and an activity (got case={case:?}, activity={activity:?})"
            ));
        }
        let mut ev = Event::new(case, activity, i as i64);
        ev.resource = r.resource.clone();
        log.add_event(ev);
    }
    Ok(log)
}

pub fn run(model: &str, log_json: &str, include_traces: bool) -> Result<String, String> {
    let model = parse_any_model(model)?;
    let net = model_to_petri_net(&model);
    let log = parse_log(log_json)?;

    let full = check_full_conformance(&log, &net);

    let non_fitting = if include_traces {
        full.fitness
            .non_fitting_traces()
            .into_iter()
            .map(|t| TraceOut {
                case_id: t.case_id.clone(),
                fitness: t.fitness,
                missing_activities: t.missing_activities.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };

    let out = ConformanceOut {
        fitness: full.fitness.fitness,
        precision: full.precision.precision,
        f_score: full.f_score,
        total_traces: full.fitness.total_traces,
        fitting_traces: full.fitness.fitting_traces,
        fitting_percent: full.fitness.fitting_percent,
        missing_tokens: full.fitness.missing_tokens,
        remaining_tokens: full.fitness.remaining_tokens,
        escaping_edges: full.precision.escaping_edges,
        total_enabled: full.precision.total_enabled,
        non_fitting_traces: non_fitting,
    };

    serde_json::to_string_pretty(&out).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsl() -> &'static str {
        r#"(schema order
            (states
                (state received :kind token :initial 1)
                (state shipped :kind token :initial 0)
            )
            (actions (action ship))
            (arcs (arc received -> ship) (arc ship -> shipped))
        )"#
    }

    #[test]
    fn perfect_fit_scores_one() {
        let log = r#"[{"case":"o1","activity":"ship"}]"#;
        let result = run(dsl(), log, true).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["fitness"], 1.0);
        assert_eq!(v["total_traces"], 1);
    }

    #[test]
    fn unknown_activity_lowers_fitness() {
        let log = r#"[{"case":"o1","activity":"ship"},{"case":"o1","activity":"cancel"}]"#;
        let result = run(dsl(), log, true).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["fitness"].as_f64().unwrap() <= 1.0);
    }

    #[test]
    fn rejects_missing_case_or_activity() {
        let err = run(dsl(), r#"[{"activity":"ship"}]"#, false).unwrap_err();
        assert!(err.contains("case id"));
    }
}
