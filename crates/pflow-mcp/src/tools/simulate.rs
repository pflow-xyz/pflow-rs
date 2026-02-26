use super::{default_rates, initial_state, parse_model, schema_to_petri_net};
use pflow_solver::{methods, solve, Options, Problem};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
struct SimulateResult {
    time_points: usize,
    labels: Vec<String>,
    time: Vec<f64>,
    series: HashMap<String, Vec<f64>>,
    final_state: HashMap<String, f64>,
}

pub fn run(model: &str, t_end: f64, max_points: usize) -> Result<String, String> {
    let schema = parse_model(model)?;
    let net = schema_to_petri_net(&schema);
    let state = initial_state(&net);
    let rates = default_rates(&schema);

    let prob = Problem::new(net, state, [0.0, t_end], rates);
    let sol = solve(&prob, &methods::tsit5(), &Options::default_opts());

    // Downsample if needed
    let total = sol.t.len();
    let step = if total > max_points {
        total / max_points
    } else {
        1
    };

    let mut time = Vec::new();
    let mut series: HashMap<String, Vec<f64>> = HashMap::new();
    for label in &sol.state_labels {
        series.insert(label.clone(), Vec::new());
    }

    for i in (0..total).step_by(step) {
        time.push(sol.t[i]);
        let st = &sol.u[i];
        for label in &sol.state_labels {
            if let Some(v) = series.get_mut(label) {
                v.push(*st.get(label).unwrap_or(&0.0));
            }
        }
    }

    // Always include the last point
    if total > 0 && (total - 1) % step != 0 {
        let last = total - 1;
        time.push(sol.t[last]);
        let st = &sol.u[last];
        for label in &sol.state_labels {
            if let Some(v) = series.get_mut(label) {
                v.push(*st.get(label).unwrap_or(&0.0));
            }
        }
    }

    let final_state = sol
        .get_final_state()
        .cloned()
        .unwrap_or_default();

    let result = SimulateResult {
        time_points: time.len(),
        labels: sol.state_labels.clone(),
        time,
        series,
        final_state,
    };

    serde_json::to_string_pretty(&result).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulate_basic() {
        let dsl = r#"(schema test
            (states
                (state A :kind token :initial 10)
                (state B :kind token :initial 0)
            )
            (actions (action t1))
            (arcs (arc A -> t1) (arc t1 -> B))
        )"#;
        let result = run(dsl, 10.0, 50).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["time_points"].as_u64().unwrap() > 0);
        assert!(v["time"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn test_simulate_downsamples() {
        let dsl = r#"(schema test
            (states
                (state A :kind token :initial 100)
                (state B :kind token :initial 0)
            )
            (actions (action t1))
            (arcs (arc A -> t1) (arc t1 -> B))
        )"#;
        let result = run(dsl, 100.0, 10).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        // Should have at most ~11 points (10 + possible last)
        assert!(v["time_points"].as_u64().unwrap() <= 12);
    }
}
