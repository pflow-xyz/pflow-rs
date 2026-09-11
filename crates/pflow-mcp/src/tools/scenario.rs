//! `petri_scenario`: a what-if. Overrides the initial marking, transition
//! rates and/or a rate schedule, then runs the model's own default
//! stochastic engine ([`pflow_solver::stochastic`]) forward — the same
//! engine `pflow_stochastic` uses under the hood, so an operator question
//! ("what if one more barista") gets an answer computed the same way the
//! net says it would happen. Mirrors petri-pilot's `pkg/mcp/scenario.go`
//! (SSA path; `engine: "ode"` is not offered here — see the module doc on
//! why the ODE path is out of scope for this phase).
//!
//! `scenarios`, when given, runs each named override on one seed and
//! reports them side by side — the only way to tell a real difference from
//! the dice, per petri-pilot's own doc comment on the tool.

use super::convert::parse_any_model;
use pflow_metamodel::{Model, RateSegment};
use pflow_solver::stochastic::{simulate, Options};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Deserialize)]
pub struct ScenarioOverride {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub marking: HashMap<String, i64>,
    #[serde(default)]
    pub rates: HashMap<String, f64>,
    #[serde(default)]
    pub schedule: HashMap<String, Vec<RateSegment>>,
}

#[derive(Serialize)]
struct ScenarioRun {
    name: String,
    final_state: HashMap<String, f64>,
    throughput: HashMap<String, f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    depleted: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    contended: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    caveats: Vec<String>,
}

#[derive(Serialize)]
struct ScenarioResult {
    method: String,
    hours: f64,
    samples: usize,
    realizations: usize,
    seed: u64,
    runs: Vec<ScenarioRun>,
}

/// Runs one override against `model` and returns the operator summary.
fn run_one(model: &Model, name: &str, ov: &ScenarioOverride, hours: f64, samples: usize, realizations: usize, seed: u64) -> Result<ScenarioRun, String> {
    let marking = pflow_solver::stochastic::start_from(model, &ov.marking);
    let opts = Options {
        horizon: hours,
        samples,
        rates: ov.rates.clone(),
        seed,
        realizations,
        guard: None,
        schedule: ov.schedule.clone(),
        on_fire: None,
    }
    .with_defaults(model);

    let res = simulate(model, &marking, &opts).map_err(|e| format!("{name}: {e}"))?;

    let throughput = res.metrics.as_ref().map(|m| m.throughput.clone()).unwrap_or_default();
    let depleted = res.depleted.iter().map(|d| format!("{} at t={:.3}{}", d.place, d.at, if d.recovered { " (recovered)" } else { "" })).collect();
    let contended: Vec<String> = res
        .contended
        .iter()
        .map(|c| format!("{}: {:.1}% of the run", c.place, c.fraction * 100.0))
        .collect();

    Ok(ScenarioRun {
        name: name.to_string(),
        final_state: res.final_.clone(),
        throughput,
        depleted,
        contended,
        caveats: res.caveats.clone(),
    })
}

/// Mirrors `petri_scenario`'s own many independent overrides
/// (marking/rates/schedule/scenarios/hours/samples/realizations/seed) —
/// bundling them into a struct here would just move the same field list one
/// layer over, not reduce it.
#[allow(clippy::too_many_arguments)]
pub fn run(
    model: &str,
    marking: HashMap<String, i64>,
    rates: HashMap<String, f64>,
    schedule: HashMap<String, Vec<RateSegment>>,
    scenarios: Vec<ScenarioOverride>,
    hours: f64,
    samples: usize,
    realizations: usize,
    seed: u64,
) -> Result<String, String> {
    let model = parse_any_model(model)?;

    let runs = if !scenarios.is_empty() {
        scenarios
            .iter()
            .map(|s| {
                let name = if s.name.is_empty() { "scenario".to_string() } else { s.name.clone() };
                run_one(&model, &name, s, hours, samples, realizations, seed)
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let ov = ScenarioOverride { name: "default".to_string(), marking, rates, schedule };
        vec![run_one(&model, "default", &ov, hours, samples, realizations, seed)?]
    };

    let out = ScenarioResult {
        method: "ssa".to_string(),
        hours,
        samples,
        realizations,
        seed,
        runs,
    };

    serde_json::to_string_pretty(&out).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsl() -> &'static str {
        r#"(schema shop
            (states
                (state staff :kind token :initial 2)
                (state served :kind token :initial 0)
            )
            (actions (action serve))
            (arcs (arc staff -> serve) (arc serve -> served))
        )"#
    }

    #[test]
    fn default_scenario_runs() {
        let result = run(dsl(), HashMap::new(), HashMap::new(), HashMap::new(), Vec::new(), 1.0, 10, 1, 1).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["runs"].as_array().unwrap().len(), 1);
        assert_eq!(v["runs"][0]["name"], "default");
    }

    #[test]
    fn named_scenarios_compare() {
        let scenarios = vec![
            ScenarioOverride { name: "baseline".into(), marking: HashMap::new(), rates: HashMap::new(), schedule: HashMap::new() },
            ScenarioOverride {
                name: "one more staff".into(),
                marking: [("staff".to_string(), 3i64)].into_iter().collect(),
                rates: HashMap::new(),
                schedule: HashMap::new(),
            },
        ];
        let result = run(dsl(), HashMap::new(), HashMap::new(), HashMap::new(), scenarios, 1.0, 10, 2, 7).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["runs"].as_array().unwrap().len(), 2);
        assert_eq!(v["runs"][1]["name"], "one more staff");
    }
}
