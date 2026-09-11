//! Parameter sweep result types and objective functions, ported from
//! go-pflow's `results/sweep.go`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::types::Results;

/// Results from a parameter sweep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepResults {
    pub version: String,
    #[serde(rename = "baseModel")]
    pub base_model: String,
    pub objective: String,
    pub parameters: Vec<ParameterSweep>,
    pub variants: Vec<VariantResult>,
    pub best: Option<VariantResult>,
    pub worst: Option<VariantResult>,
    pub summary: SweepSummary,
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub recommended: HashMap<String, String>,
}

/// A swept parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterSweep {
    pub name: String,
    /// "rate" or "initial"
    #[serde(rename = "type")]
    pub kind: String,
    pub values: Vec<f64>,
    pub min: f64,
    pub max: f64,
}

/// Results for one parameter combination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantResult {
    pub id: i64,
    pub parameters: HashMap<String, f64>,
    pub metrics: Metrics,
    pub score: f64,
    pub rank: i64,
    #[serde(
        rename = "resultsFile",
        skip_serializing_if = "String::is_empty",
        default
    )]
    pub results_file: String,
}

/// Key metrics extracted from a simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Metrics {
    #[serde(rename = "maxPeak")]
    pub max_peak: f64,
    #[serde(rename = "maxPeakVar")]
    pub max_peak_var: String,
    #[serde(rename = "maxPeakTime")]
    pub max_peak_time: f64,

    #[serde(rename = "finalState")]
    pub final_state: HashMap<String, f64>,

    #[serde(rename = "steadyReached")]
    pub steady_reached: bool,
    #[serde(rename = "steadyTime", skip_serializing_if = "is_zero", default)]
    pub steady_time: f64,

    pub conserved: bool,

    #[serde(rename = "computeTime")]
    pub compute_time: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// Overview of a sweep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepSummary {
    #[serde(rename = "totalVariants")]
    pub total_variants: i64,
    #[serde(rename = "successCount")]
    pub success_count: i64,
    #[serde(rename = "failureCount")]
    pub failure_count: i64,
    #[serde(rename = "bestScore")]
    pub best_score: f64,
    #[serde(rename = "worstScore")]
    pub worst_score: f64,
    #[serde(rename = "scoreRange")]
    pub score_range: f64,
}

/// Evaluates how good a result is (lower is better).
pub type ObjectiveFunc = fn(&Results) -> Result<f64, String>;

/// `minimize_peak`: the largest peak across all variables.
pub fn minimize_peak(r: &Results) -> Result<f64, String> {
    max_peak_value(r)
}

/// `maximize_peak`: negated largest peak, so minimizing the score maximizes
/// the peak.
pub fn maximize_peak(r: &Results) -> Result<f64, String> {
    max_peak_value(r).map(|v| -v)
}

fn max_peak_value(r: &Results) -> Result<f64, String> {
    let peaks = r
        .analysis
        .as_ref()
        .map(|a| a.peaks.as_slice())
        .unwrap_or(&[]);
    if peaks.is_empty() {
        return Err("no peaks found".to_string());
    }
    let mut max_peak = 0.0_f64;
    for p in peaks {
        if p.value > max_peak {
            max_peak = p.value;
        }
    }
    Ok(max_peak)
}

/// `minimize_final`: sum of the final state (useful for minimizing residual).
pub fn minimize_final(r: &Results) -> Result<f64, String> {
    Ok(r.data.summary.final_state.values().sum())
}

/// `maximize_throughput`: negated final value of a `Completed`/`Output`/`Done`
/// place, so minimizing the score maximizes throughput.
pub fn maximize_throughput(r: &Results) -> Result<f64, String> {
    for (name, value) in &r.data.summary.final_state {
        if name == "Completed" || name == "Output" || name == "Done" {
            return Ok(-value);
        }
    }
    Err("no throughput variable found".to_string())
}

/// `minimize_time_to_steady`: time to reach steady state, or `f64::MAX` if
/// it never did.
pub fn minimize_time_to_steady(r: &Results) -> Result<f64, String> {
    match &r.analysis {
        Some(a) => match &a.steady_state {
            Some(ss) if ss.reached => Ok(ss.time),
            _ => Ok(f64::MAX),
        },
        None => Ok(f64::MAX),
    }
}

/// Looks up an objective by go-pflow's `Objectives` map key.
pub fn objective_by_name(name: &str) -> Option<ObjectiveFunc> {
    match name {
        "minimize_peak" => Some(minimize_peak),
        "maximize_peak" => Some(maximize_peak),
        "minimize_final" => Some(minimize_final),
        "maximize_throughput" => Some(maximize_throughput),
        "minimize_time_to_steady" => Some(minimize_time_to_steady),
        _ => None,
    }
}

/// Extracts key metrics from simulation results.
pub fn extract_metrics(r: &Results) -> Metrics {
    let mut m = Metrics {
        final_state: r.data.summary.final_state.clone(),
        compute_time: r.metadata.compute_time,
        ..Default::default()
    };

    if let Some(a) = &r.analysis {
        for p in &a.peaks {
            if p.value > m.max_peak {
                m.max_peak = p.value;
                m.max_peak_var = p.variable.clone();
                m.max_peak_time = p.time;
            }
        }

        if let Some(ss) = &a.steady_state {
            m.steady_reached = ss.reached;
            if m.steady_reached {
                m.steady_time = ss.time;
            }
        }

        if let Some(c) = &a.conservation {
            m.conserved = c.total_tokens.conserved;
        }
    }

    m
}

/// Sorts variants by score (ascending — lower is better) and assigns ranks.
pub fn rank_variants(variants: &mut [VariantResult]) {
    variants.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (i, v) in variants.iter_mut().enumerate() {
        v.rank = i as i64 + 1;
    }
}

/// Generates human-readable recommendations comparing the best and worst
/// variants.
pub fn generate_recommendations(sweep: &SweepResults) -> HashMap<String, String> {
    let mut rec = HashMap::new();

    let best = match &sweep.best {
        Some(b) => b,
        None => return rec,
    };

    if let Some(worst) = &sweep.worst {
        for (param, &best_val) in &best.parameters {
            let worst_val = worst.parameters.get(param).copied().unwrap_or(0.0);
            if best_val != worst_val {
                let diff = best_val - worst_val;
                let pct = (diff / worst_val) * 100.0;

                let direction = if best_val > worst_val {
                    "increase"
                } else {
                    "decrease"
                };

                rec.insert(
                    param.clone(),
                    format!(
                        "{direction} by {:.1}% ({worst_val:.6} \u{2192} {best_val:.6})",
                        pct.abs()
                    ),
                );
            }
        }

        let best_metric = best.metrics.max_peak;
        let worst_metric = worst.metrics.max_peak;
        let improvement = ((worst_metric - best_metric) / worst_metric) * 100.0;

        rec.insert(
            "improvement".to_string(),
            format!(
                "{improvement:.1}% reduction in peak ({worst_metric:.2} \u{2192} {best_metric:.2})"
            ),
        );
    }

    rec
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variant(id: i64, score: f64) -> VariantResult {
        VariantResult {
            id,
            parameters: HashMap::new(),
            metrics: Metrics::default(),
            score,
            rank: 0,
            results_file: String::new(),
        }
    }

    #[test]
    fn rank_variants_sorts_ascending_and_ranks_from_one() {
        let mut variants = vec![variant(0, 3.0), variant(1, 1.0), variant(2, 2.0)];
        rank_variants(&mut variants);
        assert_eq!(
            variants.iter().map(|v| v.id).collect::<Vec<_>>(),
            vec![1, 2, 0]
        );
        assert_eq!(
            variants.iter().map(|v| v.rank).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn objective_by_name_covers_go_pflow_keys() {
        for name in [
            "minimize_peak",
            "maximize_peak",
            "minimize_final",
            "maximize_throughput",
            "minimize_time_to_steady",
        ] {
            assert!(
                objective_by_name(name).is_some(),
                "missing objective {name}"
            );
        }
        assert!(objective_by_name("nonexistent").is_none());
    }

    #[test]
    fn minimize_final_sums_final_state() {
        let mut r = empty_results();
        r.data.summary.final_state.insert("A".into(), 2.0);
        r.data.summary.final_state.insert("B".into(), 3.0);
        assert_eq!(minimize_final(&r).unwrap(), 5.0);
    }

    #[test]
    fn maximize_throughput_finds_completed_place() {
        let mut r = empty_results();
        r.data.summary.final_state.insert("Completed".into(), 7.0);
        assert_eq!(maximize_throughput(&r).unwrap(), -7.0);
    }

    #[test]
    fn maximize_throughput_errors_without_a_named_place() {
        let r = empty_results();
        assert!(maximize_throughput(&r).is_err());
    }

    #[test]
    fn generate_recommendations_reports_parameter_direction() {
        let mut best = variant(0, 1.0);
        best.parameters.insert("rate".into(), 2.0);
        best.metrics.max_peak = 5.0;
        let mut worst = variant(1, 9.0);
        worst.parameters.insert("rate".into(), 1.0);
        worst.metrics.max_peak = 10.0;

        let sweep = SweepResults {
            version: "1.0.0".into(),
            base_model: "m".into(),
            objective: "minimize_peak".into(),
            parameters: vec![],
            variants: vec![],
            best: Some(best),
            worst: Some(worst),
            summary: SweepSummary {
                total_variants: 2,
                success_count: 2,
                failure_count: 0,
                best_score: 1.0,
                worst_score: 9.0,
                score_range: 8.0,
            },
            recommended: HashMap::new(),
        };

        let rec = generate_recommendations(&sweep);
        assert!(rec["rate"].starts_with("increase by 100.0%"));
        assert!(rec["improvement"].contains("reduction in peak"));
    }

    fn empty_results() -> Results {
        Results {
            version: "1.0.0".into(),
            metadata: super::super::types::Metadata {
                timestamp: String::new(),
                solver: String::new(),
                status: String::new(),
                error: String::new(),
                compute_time: 0.0,
            },
            model: super::super::types::Model {
                name: String::new(),
                places: vec![],
                transitions: vec![],
                arcs: 0,
                structure: None,
            },
            simulation: super::super::types::Simulation {
                timespan: [0.0, 0.0],
                initial_state: HashMap::new(),
                rates: HashMap::new(),
                options: None,
            },
            data: super::super::types::Data {
                summary: super::super::types::Summary {
                    points: 0,
                    final_time: 0.0,
                    final_state: HashMap::new(),
                },
                timeseries: super::super::types::Timeseries {
                    time: super::super::types::TimeData::default(),
                    variables: HashMap::new(),
                },
            },
            analysis: None,
            events: vec![],
        }
    }
}
