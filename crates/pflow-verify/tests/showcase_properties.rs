//! Byte-for-field parity of `Verifier` against go-pflow's verify golden.
//!
//! `tests/fixtures/showcase/properties.json` is a byte-identical copy of
//! `go-pflow/verify/testdata/showcase/properties.json` — see the README
//! there. It is `cmd/properties-goldens` run against the showcase's
//! `cafe-order.json`: the same representative property set
//! (bounded/deadlock-free/live/terminating/conserves, the model's own
//! `one_state` invariant, a mutual-exclusion, two reachable and one
//! unreachable target) that `tests/cafe_order.rs` already exercises a
//! subset of, here checked field for field against go-pflow's own verdicts
//! — `status`, `method`, `detail`, `evidence` and any `counterexample` —
//! not just the statuses. This is `pflow-rs/ROADMAP.md` Phase 2's exit
//! criterion ("`petri_verify`-equivalent output for `cafe-order.json`
//! matches Go field for field"), which was previously held open pending a
//! go-pflow-produced golden; `cmd/properties-goldens` is that golden.
//!
//! Skips quietly without a `pflow-xyz` checkout as a sibling of this repo,
//! the same as `tests/cafe_order.rs` and `pflow-compose`'s bundle golden.

use std::collections::HashMap;
use std::path::PathBuf;

use pflow_metamodel::Model;
use pflow_verify::{Kind, Method, Property, Status, Verifier};
use serde_json::Value;

fn showcase_path() -> Option<PathBuf> {
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../pflow-xyz/examples/showcase/cafe-order.json");
    candidate.exists().then_some(candidate)
}

fn golden() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/showcase/properties.json");
    let text = std::fs::read_to_string(path).expect("tests/fixtures/showcase golden exists");
    serde_json::from_str(&text).expect("golden is valid JSON")
}

fn status_str(s: Status) -> &'static str {
    match s {
        Status::Proved => "proved",
        Status::Refuted => "refuted",
        Status::Unknown => "unknown",
    }
}

fn method_str(m: Method) -> &'static str {
    match m {
        Method::Structural => "structural",
        Method::Exhaustive => "exhaustive",
        Method::Witness => "witness",
        Method::Partial => "partial",
        Method::None => "none",
    }
}

fn target_from(v: &Value) -> HashMap<String, i64> {
    v.as_object()
        .expect("target object")
        .iter()
        .map(|(k, n)| (k.clone(), n.as_i64().expect("target value is an integer")))
        .collect()
}

/// The same representative property set `go-pflow/cmd/properties-goldens`
/// runs, built in the same order so `report.verdicts[i]` lines up 1:1 with
/// the golden's `properties[i]`/`report.verdicts[i]`.
fn showcase_properties(golden_props: &[Value]) -> Vec<Property> {
    golden_props
        .iter()
        .map(|p| {
            let kind = match p["kind"].as_str().unwrap() {
                "bounded" => Kind::Bounded,
                "deadlock-free" => Kind::DeadlockFree,
                "live" => Kind::Live,
                "terminating" => Kind::Terminating,
                "conserves" => Kind::Conserves,
                "invariant" => Kind::Invariant,
                "mutual-exclusion" => Kind::MutualExclusion,
                "reachable" => Kind::Reachable,
                "unreachable" => Kind::Unreachable,
                other => panic!("unknown property kind {other:?}"),
            };
            Property {
                kind: Some(kind),
                name: p["name"].as_str().unwrap_or_default().to_string(),
                expr: p.get("expr").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                target: p
                    .get("target")
                    .map(target_from)
                    .unwrap_or_default(),
                places: p
                    .get("places")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect())
                    .unwrap_or_default(),
                bound: p.get("bound").and_then(|v| v.as_i64()).unwrap_or(0),
            }
        })
        .collect()
}

#[test]
fn cafe_order_verify_matches_go_pflow_field_for_field() {
    let Some(path) = showcase_path() else {
        eprintln!("pflow-xyz not checked out alongside pflow-rs; skipping");
        return;
    };
    let raw = std::fs::read_to_string(&path).expect("read cafe-order.json");
    let model: Model = serde_json::from_str(&raw).expect("cafe-order.json deserializes as a Model");

    let g = golden();
    assert_eq!(model.places.len(), g["places"].as_u64().unwrap() as usize);
    assert_eq!(model.transitions.len(), g["transitions"].as_u64().unwrap() as usize);
    assert_eq!(model.arcs.len(), g["arcs"].as_u64().unwrap() as usize);

    let golden_props = g["properties"].as_array().expect("properties array");
    let props = showcase_properties(golden_props);

    let v = Verifier::new(&model);
    let report = v.check(&props);

    let want_report = &g["report"];
    let want_verdicts = want_report["verdicts"].as_array().expect("report.verdicts");
    assert_eq!(
        report.verdicts.len(),
        want_verdicts.len(),
        "verdict count must match go-pflow's"
    );

    for (i, (got, want)) in report.verdicts.iter().zip(want_verdicts).enumerate() {
        let label = golden_props[i]["label"].as_str().unwrap_or_default();
        assert_eq!(
            got.property.name,
            want["property"]["name"].as_str().unwrap_or_default(),
            "verdict[{i}] ({label}): property.name"
        );
        assert_eq!(
            status_str(got.status),
            want["status"].as_str().unwrap(),
            "verdict[{i}] ({label}): status"
        );
        assert_eq!(
            method_str(got.method),
            want["method"].as_str().unwrap(),
            "verdict[{i}] ({label}): method"
        );
        assert_eq!(
            got.detail,
            want["detail"].as_str().unwrap_or_default(),
            "verdict[{i}] ({label}): detail"
        );
        assert_eq!(
            got.evidence,
            want["evidence"].as_str().unwrap_or_default(),
            "verdict[{i}] ({label}): evidence"
        );

        match (&got.counterexample, want.get("counterexample")) {
            (Some(ce), Some(wce)) => {
                let want_trace: Vec<String> = wce["trace"]
                    .as_array()
                    .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect())
                    .unwrap_or_default();
                assert_eq!(ce.trace, want_trace, "verdict[{i}] ({label}): counterexample.trace");
                let want_marking = target_from(&wce["marking"]);
                assert_eq!(
                    ce.marking, want_marking,
                    "verdict[{i}] ({label}): counterexample.marking"
                );
                assert_eq!(
                    ce.explanation,
                    wce["explanation"].as_str().unwrap_or_default(),
                    "verdict[{i}] ({label}): counterexample.explanation"
                );
            }
            (None, None) | (None, Some(Value::Null)) => {}
            (got_ce, want_ce) => panic!(
                "verdict[{i}] ({label}): counterexample presence mismatch: got {got_ce:?}, want {want_ce:?}"
            ),
        }
    }

    assert_eq!(report.proved, want_report["proved"].as_u64().unwrap() as usize);
    assert_eq!(report.refuted, want_report["refuted"].as_u64().unwrap() as usize);
    assert_eq!(report.unknown, want_report["unknown"].as_u64().unwrap() as usize);
    assert_eq!(report.ok, want_report["ok"].as_bool().unwrap());
    assert_eq!(
        report.state_count,
        want_report["state_count"].as_u64().unwrap() as usize
    );
    assert_eq!(report.truncated, want_report["truncated"].as_bool().unwrap());
    assert_eq!(report.summary(), g["summary"].as_str().unwrap());
}
