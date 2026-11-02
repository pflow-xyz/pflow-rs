//! `edit` against pflow.xyz/mcp's `pflow_edit`: `tests/fixtures/hosted/edit.json`
//! holds the hosted answers (captured live, see that directory's README) for
//! each case's arguments; the local answer must equal them, CID included.

use serde_json::Value;

#[test]
fn edit_matches_hosted() {
    let cases: serde_json::Map<String, Value> =
        serde_json::from_str(include_str!("fixtures/hosted/edit.json")).unwrap();
    assert!(cases.len() >= 10);
    for (name, case) in &cases {
        let got = pflow_hosted::call("edit", &case["args"]);
        match (case.get("expect"), case.get("error")) {
            (Some(want), _) => assert_eq!(got.as_ref().unwrap(), want, "{name}"),
            (_, Some(Value::String(want))) => assert_eq!(&got.unwrap_err(), want, "{name}"),
            _ => panic!("{name}: no expect or error"),
        }
    }
}

#[test]
fn edit_refuses_bad_arguments() {
    let call = |a: Value| pflow_hosted::call("edit", &a).unwrap_err();
    assert_eq!(call(serde_json::json!({})), "ops: a list of {op, …}");
    assert_eq!(
        call(serde_json::json!({"net": 3, "ops": []})),
        "net: a JSON object (the editor's format)"
    );
    let many = vec![serde_json::json!({"op": "add_place"}); 501];
    assert_eq!(
        call(serde_json::json!({"ops": many})),
        "ops: at most 500 per call"
    );
}
