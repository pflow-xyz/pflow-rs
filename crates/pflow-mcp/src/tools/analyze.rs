use pflow_core::ArcKind;
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

        // A read or inhibitor arc gates the firing and moves no tokens, so it
        // has no entry in the incidence matrix.
        for arc in net.input_arcs(t_label) {
            if net.arc_kind(arc) == ArcKind::Normal {
                let w = arc.weight_sum();
                *inputs.entry(arc.source.clone()).or_default() += w;
                *delta.entry(arc.source.clone()).or_default() -= w;
            }
        }

        for arc in net.output_arcs(t_label) {
            if net.arc_kind(arc) == ArcKind::Normal {
                let w = arc.weight_sum();
                *outputs.entry(arc.target.clone()).or_default() += w;
                *delta.entry(arc.target.clone()).or_default() += w;
            }
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
            let tokens = |p: &str| state.get(p).copied().unwrap_or(0.0);
            net.input_arcs(t).iter().all(|arc| match net.arc_kind(arc) {
                ArcKind::Inhibitor => tokens(&arc.source) < arc.weight_sum(),
                _ => tokens(&arc.source) >= arc.weight_sum(),
            }) && net.output_arcs(t).iter().all(|arc| match net.arc_kind(arc) {
                ArcKind::Read => tokens(&arc.target) >= arc.weight_sum(),
                _ => true,
            })
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

    /// A gate is not a flow: `g` has no entry in `t`'s incidence row, and it
    /// decides whether `t` is enabled.
    #[test]
    fn contextual_arcs_gate_and_move_nothing() {
        let shape_a = |initial: u32, arc: &str| {
            format!(
                r#"{{"@type":"PetriNet",
                    "places":{{"a":{{"initial":[5]}},"g":{{"initial":[{initial}]}}}},
                    "transitions":{{"t":{{}}}},
                    "arcs":[{{"source":"a","target":"t","weight":[1]}},{arc}]}}"#
            )
        };
        const READ: &str = r#"{"source":"t","target":"g","weight":[1],"inhibitTransition":true}"#;
        const INHIBITOR: &str = r#"{"source":"g","target":"t","weight":[1],"inhibitTransition":true}"#;
        let enabled = |initial, arc| {
            let v: serde_json::Value =
                serde_json::from_str(&run(&shape_a(initial, arc)).unwrap()).unwrap();
            assert!(v["incidence"]["t"]["delta"].get("g").is_none(), "{v}");
            assert_eq!(v["incidence"]["t"]["delta"]["a"].as_f64(), Some(-1.0), "{v}");
            !v["enabled"].as_array().unwrap().is_empty()
        };
        assert!(enabled(1, READ));
        assert!(!enabled(0, READ));
        assert!(!enabled(1, INHIBITOR));
        assert!(enabled(0, INHIBITOR));
    }
}
