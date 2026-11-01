use super::{parse_model, schema_to_petri_net};
use pflow_solver::ssa::{simulate, SsaModel, SsaOptions};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
struct PlaceSeries {
    values: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stddev: Option<Vec<f64>>,
}

#[derive(Serialize)]
struct StochasticResult {
    method: &'static str,
    horizon: f64,
    samples: usize,
    realizations: usize,
    seed: u64,
    places: Vec<String>,
    times: Vec<f64>,
    series: HashMap<String, PlaceSeries>,
    final_state: HashMap<String, f64>,
}

/// Run the portable Gillespie SSA on a schema (DSL or JSON). Place order is
/// the schema's token-state declaration order, transition order the action
/// order; rates default to 1.0.
///
/// A smoke surface, not the parity gate (`tests/ssa_parity.rs` is). Arc weights
/// and read and inhibitor arcs carry through from any wire shape that can state
/// them. Capacity, rate and delay do not: `Schema` has no field for them
/// (`convert::model_to_schema`).
pub fn run(
    model: &str,
    horizon: f64,
    samples: usize,
    realizations: usize,
    seed: u64,
    rates: &HashMap<String, f64>,
) -> Result<String, String> {
    let schema = parse_model(model)?;
    let net = schema_to_petri_net(&schema);
    let place_order: Vec<String> = schema
        .states
        .iter()
        .filter(|s| s.is_token())
        .map(|s| s.id.clone())
        .collect();
    let transition_order: Vec<String> = schema.actions.iter().map(|a| a.id.clone()).collect();
    let ssa_model = SsaModel::from_petri_net(&net, &place_order, &transition_order, rates)?;
    let opts = SsaOptions {
        horizon,
        samples,
        realizations,
        seed,
    };
    let res = simulate(&ssa_model, &opts).map_err(|e| e.to_string())?;

    let mut series = HashMap::new();
    let mut final_state = HashMap::new();
    for (p, id) in res.places.iter().enumerate() {
        series.insert(
            id.clone(),
            PlaceSeries {
                values: res.values[p].clone(),
                stddev: res.stddev.as_ref().map(|s| s[p].clone()),
            },
        );
        final_state.insert(id.clone(), res.final_[p]);
    }
    let out = StochasticResult {
        method: "ssa-portable",
        horizon,
        samples,
        realizations,
        seed,
        places: res.places,
        times: res.times,
        series,
        final_state,
    };
    serde_json::to_string_pretty(&out).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stochastic_basic() {
        let dsl = r#"(schema chain
            (states
                (state a :kind token :initial 100)
                (state b :kind token :initial 0)
                (state c :kind token :initial 0)
            )
            (actions (action ab) (action bc))
            (arcs (arc a -> ab) (arc ab -> b) (arc b -> bc) (arc bc -> c))
        )"#;
        let result = run(dsl, 10.0, 11, 3, 42, &HashMap::new()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["times"].as_array().unwrap().len(), 11);
        // Spec §3.8 chain ensemble (seed 42, R=3): a.values[1] and final.
        assert_eq!(
            v["series"]["a"]["values"][1].as_f64().unwrap(),
            35.666666666666664
        );
        assert_eq!(v["final_state"]["c"].as_f64().unwrap(), 100.0);
    }

    const SHAPE_A: &str = r#"{"@type":"PetriNet",
        "places":{"a":{"initial":[20]},"b":{"initial":[0]},"g":{"initial":[GATE]}},
        "transitions":{"t":{}},
        "arcs":[{"source":"a","target":"t","weight":[1]},
                {"source":"t","target":"b","weight":[1]},
                {"source":"t","target":"g","weight":[1],"inhibitTransition":true}]}"#;

    fn finals(model: &str) -> (f64, f64, f64) {
        let r = run(model, 50.0, 11, 4, 7, &HashMap::new()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&r).unwrap();
        let f = |p: &str| v["final_state"][p].as_f64().unwrap();
        (f("a"), f("b"), f("g"))
    }

    /// A read arc through the whole tool path: Shape A to `Model` (a read arc
    /// reversed to place -> transition), to `Schema`, to `PetriNet` (drawn
    /// transition -> place again), to `SsaModel` (flipped once more).
    #[test]
    fn test_stochastic_read_arc_from_shape_a() {
        assert_eq!(finals(&SHAPE_A.replace("GATE", "1")), (0.0, 20.0, 1.0));
        assert_eq!(finals(&SHAPE_A.replace("GATE", "0")), (20.0, 0.0, 0.0));
    }

    #[test]
    fn test_stochastic_read_arc_from_shape_b_and_schema() {
        let shape_b = |gate: u32| {
            format!(
                r#"{{"name":"g",
                    "places":[{{"id":"a","initial":20}},{{"id":"b"}},{{"id":"g","initial":{gate}}}],
                    "transitions":[{{"id":"t"}}],
                    "arcs":[{{"from":"a","to":"t"}},{{"from":"t","to":"b"}},
                            {{"from":"g","to":"t","type":"read"}}]}}"#
            )
        };
        assert_eq!(finals(&shape_b(1)), (0.0, 20.0, 1.0));
        assert_eq!(finals(&shape_b(0)), (20.0, 0.0, 0.0));

        let schema = |gate: u32| {
            format!(
                r#"{{"name":"g",
                    "states":[{{"id":"a","kind":"token","initial":20}},
                              {{"id":"b","kind":"token","initial":0}},
                              {{"id":"g","kind":"token","initial":{gate}}}],
                    "actions":[{{"id":"t"}}],
                    "arcs":[{{"source":"a","target":"t"}},{{"source":"t","target":"b"}},
                            {{"source":"g","target":"t","type":"read"}}]}}"#
            )
        };
        assert_eq!(finals(&schema(1)), (0.0, 20.0, 1.0));
        assert_eq!(finals(&schema(0)), (20.0, 0.0, 0.0));
    }

    #[test]
    fn test_stochastic_inhibitor_arc_blocks() {
        let inhibited = SHAPE_A.replace(
            r#""source":"t","target":"g""#,
            r#""source":"g","target":"t""#,
        );
        assert_eq!(finals(&inhibited.replace("GATE", "1")), (20.0, 0.0, 1.0));
        assert_eq!(finals(&inhibited.replace("GATE", "0")), (0.0, 20.0, 0.0));
    }
}
