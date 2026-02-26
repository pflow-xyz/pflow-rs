use super::{initial_state, parse_model, schema_to_petri_net};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
struct AnalyzeResult {
    places: Vec<String>,
    transitions: Vec<String>,
    incidence: HashMap<String, TransitionEffect>,
    enabled: Vec<String>,
    initial_state: HashMap<String, f64>,
}

#[derive(Serialize)]
struct TransitionEffect {
    inputs: HashMap<String, f64>,
    outputs: HashMap<String, f64>,
    delta: HashMap<String, f64>,
}

pub fn run(model: &str) -> Result<String, String> {
    let schema = parse_model(model)?;
    let net = schema_to_petri_net(&schema);
    let state = initial_state(&net);

    let mut places: Vec<String> = net.places.keys().cloned().collect();
    places.sort();

    let mut transitions: Vec<String> = net.transitions.keys().cloned().collect();
    transitions.sort();

    let mut incidence = HashMap::new();

    for t_label in &transitions {
        let mut inputs: HashMap<String, f64> = HashMap::new();
        let mut outputs: HashMap<String, f64> = HashMap::new();
        let mut delta: HashMap<String, f64> = HashMap::new();

        for arc in net.input_arcs(t_label) {
            let w = arc.weight_sum();
            *inputs.entry(arc.source.clone()).or_default() += w;
            *delta.entry(arc.source.clone()).or_default() -= w;
        }

        for arc in net.output_arcs(t_label) {
            let w = arc.weight_sum();
            *outputs.entry(arc.target.clone()).or_default() += w;
            *delta.entry(arc.target.clone()).or_default() += w;
        }

        incidence.insert(
            t_label.clone(),
            TransitionEffect {
                inputs,
                outputs,
                delta,
            },
        );
    }

    // Check which transitions are enabled at initial marking
    let enabled: Vec<String> = transitions
        .iter()
        .filter(|t| {
            net.input_arcs(t)
                .iter()
                .all(|arc| state.get(&arc.source).copied().unwrap_or(0.0) >= arc.weight_sum())
        })
        .cloned()
        .collect();

    let result = AnalyzeResult {
        places,
        transitions,
        incidence,
        enabled,
        initial_state: state,
    };

    serde_json::to_string_pretty(&result).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze() {
        let dsl = r#"(schema test
            (states
                (state p1 :kind token :initial 5)
                (state p2 :kind token :initial 0)
            )
            (actions (action t1))
            (arcs (arc p1 -> t1) (arc t1 -> p2))
        )"#;
        let result = run(dsl).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["places"].as_array().unwrap().len(), 2);
        assert_eq!(v["transitions"].as_array().unwrap().len(), 1);
        assert!(v["enabled"].as_array().unwrap().contains(&serde_json::json!("t1")));
        // t1 should consume from p1 and produce to p2
        assert_eq!(v["incidence"]["t1"]["delta"]["p1"], -1.0);
        assert_eq!(v["incidence"]["t1"]["delta"]["p2"], 1.0);
    }

    #[test]
    fn test_analyze_not_enabled() {
        let dsl = r#"(schema test
            (states (state p1 :kind token :initial 0))
            (actions (action t1))
            (arcs (arc p1 -> t1))
        )"#;
        let result = run(dsl).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["enabled"].as_array().unwrap().is_empty());
    }
}
