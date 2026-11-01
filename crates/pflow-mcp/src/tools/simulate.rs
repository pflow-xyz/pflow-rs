use super::{default_rates, initial_state, parse_model, schema_to_petri_net};
use pflow_solver::{methods, solve, ContextualArcs, Options, Problem};
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

pub fn run(
    model: &str,
    t_end: f64,
    max_points: usize,
    contextual: ContextualArcs,
) -> Result<String, String> {
    let schema = parse_model(model)?;
    let net = schema_to_petri_net(&schema);
    let state = initial_state(&net);
    let rates = default_rates(&schema);

    let prob = Problem::with_contextual(net, state, [0.0, t_end], rates, contextual)
        .map_err(|e| e.to_string())?;
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
    if total > 0 && !(total - 1).is_multiple_of(step) {
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
        let result = run(dsl, 10.0, 50, ContextualArcs::Refuse).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["time_points"].as_u64().unwrap() > 0);
        assert!(!v["time"].as_array().unwrap().is_empty());
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
        let result = run(dsl, 100.0, 10, ContextualArcs::Refuse).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        // Should have at most ~11 points (10 + possible last)
        assert!(v["time_points"].as_u64().unwrap() <= 12);
    }

    /// A Shape A net: a -> t -> b, with `extra` as t's one contextual arc.
    fn shape_a(extra: &str) -> String {
        format!(
            r#"{{"@type":"PetriNet",
                "places":{{"a":{{"initial":[10],"x":0,"y":0}},"b":{{"initial":[0],"x":0,"y":0}},
                           "g":{{"initial":[1],"x":0,"y":0}}}},
                "transitions":{{"t":{{"x":0,"y":0}}}},
                "arcs":[{{"source":"a","target":"t","weight":[1]}},
                        {{"source":"t","target":"b","weight":[1]}},
                        {extra}]}}"#
        )
    }

    const READ: &str = r#"{"source":"t","target":"g","weight":[1],"inhibitTransition":true}"#;
    const INHIBITOR: &str = r#"{"source":"g","target":"t","weight":[1],"inhibitTransition":true}"#;

    fn final_state(result: &str, place: &str) -> f64 {
        let v: serde_json::Value = serde_json::from_str(result).unwrap();
        v["final_state"][place].as_f64().unwrap()
    }

    /// The MCP path reaches the solver through `Model` and `Schema`, and both
    /// conversions used to lose the arc's kind before the refusal could see it.
    #[test]
    fn contextual_arcs_are_refused_through_the_tool() {
        let e = run(&shape_a(READ), 10.0, 50, ContextualArcs::Refuse).unwrap_err();
        assert!(e.contains("1 read arc(s) gate a firing without consuming"), "{e}");
        let e = run(&shape_a(INHIBITOR), 10.0, 50, ContextualArcs::Refuse).unwrap_err();
        assert!(e.contains("1 inhibitor arc(s) block a firing"), "{e}");
    }

    #[test]
    fn contextual_opt_outs_reach_the_solver_through_the_tool() {
        let r = run(&shape_a(READ), 10.0, 50, ContextualArcs::SelfLoop).unwrap();
        assert!((final_state(&r, "g") - 1.0).abs() < 1e-9);
        assert!(final_state(&r, "b") > 9.0);
        let r = run(&shape_a(INHIBITOR), 10.0, 50, ContextualArcs::Drop).unwrap();
        assert!((final_state(&r, "g") - 1.0).abs() < 1e-9);
        assert!(run(&shape_a(INHIBITOR), 10.0, 50, ContextualArcs::SelfLoop).is_err());
    }

    /// `schema_to_petri_net` used to write every arc with weight 1.
    #[test]
    fn declared_arc_weights_reach_the_solver() {
        let schema = r#"{"name":"w","states":[{"id":"a","kind":"token","initial":10},
                                              {"id":"b","kind":"token","initial":0}],
                         "actions":[{"id":"t"}],
                         "arcs":[{"source":"a","target":"t"},
                                 {"source":"t","target":"b","weight":3}]}"#;
        let r = run(schema, 50.0, 50, ContextualArcs::Refuse).unwrap();
        assert!((final_state(&r, "b") - 30.0).abs() < 1e-3);
    }
}
