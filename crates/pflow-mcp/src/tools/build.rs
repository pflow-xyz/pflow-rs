use super::{initial_state, parse_model, schema_to_petri_net};
use serde::Serialize;

#[derive(Serialize)]
struct BuildResult {
    name: String,
    places: Vec<PlaceInfo>,
    transitions: Vec<TransitionInfo>,
    arcs: Vec<ArcInfo>,
    initial_state: std::collections::HashMap<String, f64>,
}

#[derive(Serialize)]
struct PlaceInfo {
    id: String,
    initial: f64,
}

#[derive(Serialize)]
struct TransitionInfo {
    id: String,
}

#[derive(Serialize)]
struct ArcInfo {
    source: String,
    target: String,
    weight: f64,
}

pub fn run(model: &str) -> Result<String, String> {
    let schema = parse_model(model)?;
    let net = schema_to_petri_net(&schema);
    let state = initial_state(&net);

    let mut places: Vec<PlaceInfo> = net
        .places
        .iter()
        .map(|(id, p)| PlaceInfo {
            id: id.clone(),
            initial: p.token_count(),
        })
        .collect();
    places.sort_by(|a, b| a.id.cmp(&b.id));

    let mut transitions: Vec<TransitionInfo> = net
        .transitions
        .keys()
        .map(|id| TransitionInfo { id: id.clone() })
        .collect();
    transitions.sort_by(|a, b| a.id.cmp(&b.id));

    let mut arcs: Vec<ArcInfo> = net
        .arcs
        .iter()
        .map(|a| ArcInfo {
            source: a.source.clone(),
            target: a.target.clone(),
            weight: a.weight_sum(),
        })
        .collect();
    arcs.sort_by(|a, b| a.source.cmp(&b.source).then_with(|| a.target.cmp(&b.target)));

    let result = BuildResult {
        name: schema.name,
        places,
        transitions,
        arcs,
        initial_state: state,
    };

    serde_json::to_string_pretty(&result).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_dsl() {
        let dsl = r#"(schema test
            (states
                (state p1 :kind token :initial 5)
                (state p2 :kind token :initial 0)
            )
            (actions (action t1))
            (arcs
                (arc p1 -> t1)
                (arc t1 -> p2)
            )
        )"#;
        let result = run(dsl).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["name"], "test");
        assert_eq!(v["places"].as_array().unwrap().len(), 2);
        assert_eq!(v["transitions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_build_json() {
        let json = serde_json::json!({
            "name": "test",
            "version": "v1",
            "states": [
                {"id": "p1", "kind": "token", "initial": 3},
                {"id": "p2", "kind": "token", "initial": 0}
            ],
            "actions": [{"id": "t1"}],
            "arcs": [
                {"source": "p1", "target": "t1"},
                {"source": "t1", "target": "p2"}
            ]
        });
        let result = run(&json.to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["places"].as_array().unwrap().len(), 2);
    }
}
