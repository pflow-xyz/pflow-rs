//! What the native goldens do not reach: `sde` and `what_if` fitting their
//! answer to the hosted result budget, and refusing arguments by name.

use serde_json::{json, Map, Value};

/// A chain of `n` places, `p0` holding 50 tokens, each `t<i>` moving one on.
fn chain(n: usize) -> Value {
    let mut places = Map::new();
    let mut transitions = Map::new();
    let mut arcs = vec![];
    for i in 0..n {
        let initial = if i == 0 { 50 } else { 0 };
        places.insert(
            format!("p{i:03}"),
            json!({"@type": "Place", "initial": [initial]}),
        );
    }
    for i in 0..n - 1 {
        let t = format!("t{i:03}");
        transitions.insert(t.clone(), json!({"@type": "Transition", "rate": 1.0}));
        arcs.push(
            json!({"@type": "Arrow", "source": format!("p{i:03}"), "target": t, "weight": [1]}),
        );
        arcs.push(json!({"@type": "Arrow", "source": t, "target": format!("p{:03}", i + 1), "weight": [1]}));
    }
    json!({
        "@context": "https://pflow.xyz/schema",
        "@type": "PetriNet",
        "token": ["https://pflow.xyz/tokens/black"],
        "places": places,
        "transitions": transitions,
        "arcs": arcs,
    })
}

fn call(tool: &str, args: Value) -> Result<Value, String> {
    pflow_hosted::call(tool, &args)
}

#[test]
fn default_points_shrink_to_fit_and_say_so() {
    let out = call("sde", json!({"net": chain(120), "runs": 2})).unwrap();
    let note = out["note"].as_str().expect("a note");
    assert!(note.starts_with("points is "), "{note}");
    assert!(note.contains("not the default 101"), "{note}");
    let x = out["timeseries"]["x"].as_array().unwrap().len();
    assert!((2..101).contains(&x), "{x} points");
    assert!(serde_json::to_string(&out).unwrap().len() < 70_000);
}

#[test]
fn fewer_series_need_no_shrinking() {
    let out = call(
        "sde",
        json!({"net": chain(120), "runs": 2, "places": ["p119"]}),
    )
    .unwrap();
    assert!(out.get("note").is_none());
    assert_eq!(out["timeseries"]["x"].as_array().unwrap().len(), 101);
    assert_eq!(out["timeseries"]["series"].as_array().unwrap().len(), 1);
    assert_eq!(out["final"].as_object().unwrap().len(), 120);
}

#[test]
fn explicit_points_that_cannot_fit_are_refused() {
    let err = call("sde", json!({"net": chain(120), "runs": 2, "points": 500})).unwrap_err();
    assert!(err.starts_with("points 500 "), "{err}");
    assert!(err.contains("61440-byte limit"), "{err}");
    assert!(err.contains("pass places to return fewer series"), "{err}");
}

#[test]
fn what_if_default_samples_shrink_to_fit() {
    let out = call(
        "what_if",
        json!({"net": chain(60), "place": "p000", "runs": 4}),
    )
    .unwrap();
    let note = out["note"].as_str().expect("a note");
    assert!(note.starts_with("samples is "), "{note}");
    let x = out["timeseries"]["x"].as_array().unwrap().len();
    assert!((2..60).contains(&x), "{x} samples");
    assert_eq!(out["scenarios"][1]["samples"], json!(x));
    assert_eq!(out["scenarios"][1]["name"], json!("+1 p000"));
    assert_eq!(out["from"], json!(50.0));
    assert_eq!(out["to"], json!(51.0));
}

#[test]
fn arguments_are_checked_by_name_and_type() {
    let err = call("sde", json!({"net": chain(3), "steps": 4})).unwrap_err();
    assert_eq!(
        err,
        r#"sde: unknown argument "steps"; takes ["dt", "net", "places", "points", "runs", "seed", "t_end"]"#
    );
    let err = call("sde", json!({"net": chain(3), "runs": 1.5})).unwrap_err();
    assert_eq!(err, r#"sde: argument "runs" should be an integer, got 1.5"#);
    let err = call("what_if", json!({"net": chain(3)})).unwrap_err();
    assert_eq!(err, r#"what_if: missing argument "place""#);
    let err = call("sde", json!({"t_end": 3})).unwrap_err();
    assert_eq!(err, "net: a JSON object (the editor's format) is required");
    let err = call("sde", json!({"net": chain(3), "places": ["nope"]})).unwrap_err();
    assert_eq!(
        err,
        r#"places: no place "nope"; places are ["p000", "p001", "p002"]"#
    );
    let err = call("sde", json!({"net": chain(3), "seed": -2})).unwrap_err();
    assert_eq!(err, "seed must be ≥ 0, got -2");
}

#[test]
fn every_tool_is_listed() {
    for tool in ["sde", "what_if"] {
        assert!(pflow_hosted::TOOLS.contains(&tool));
    }
}
