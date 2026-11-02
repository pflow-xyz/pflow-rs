//! The wasm engine's `reachability` held to go-pflow's, the engine behind
//! pflow.xyz/mcp `pflow_reachability`. `tests/reachability/go/*.json` is that
//! tool's response for each net in `tests/reachability/nets/`, written by
//! `tests/reachability/gen` (go-pflow commit in `go/SOURCE`). A mismatch is a
//! bug in one engine or the other, never a reason to regenerate.

use std::fs;
use std::path::Path;

use pflow_wasm::Core;
use serde_json::{json, Value};

/// Keys whose values must be equal as they stand.
const EXACT: &[&str] = &[
    "states",
    "edges",
    "complete",
    "bounded",
    "deadlocks",
    "deadlock_free",
    "live",
    "quasi_live",
    "first_deadlock",
    "truncated",
    "bound",
    "note",
];

/// Go marshals a nil slice as null; Rust always has a list. Order is not part
/// of the contract for these sets (go-pflow sorts by name, pflow-rs keeps
/// declaration order), so compare them sorted.
fn set(v: &Value) -> Vec<String> {
    let mut s: Vec<String> = v
        .as_array()
        .map(|a| a.iter().map(|x| x.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    s.sort();
    s
}

/// A marking without its empty places: go-pflow omits none, but neither
/// engine's witness depends on how zeros are written.
fn marking(v: &Value) -> Value {
    match v.as_object() {
        Some(m) => Value::Object(
            m.iter()
                .filter(|(_, n)| **n != json!(0))
                .map(|(k, n)| (k.clone(), n.clone()))
                .collect(),
        ),
        None => v.clone(),
    }
}

/// Fires `trace` from the net's initial marking with pflow-rs's firing rule.
fn replay(net: &str, trace: &Value) -> Value {
    let mut c = Core::new(net).unwrap();
    for t in trace.as_array().unwrap() {
        c.fire_json(t.as_str().unwrap()).unwrap();
    }
    serde_json::from_str(&c.marking_json()).unwrap()
}

#[test]
fn verdicts_match_the_hosted_go_engine() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reachability");
    let mut seen = 0;
    let mut names: Vec<_> = fs::read_dir(dir.join("nets"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    for path in names {
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let net = fs::read_to_string(&path).unwrap();
        let go: Value = serde_json::from_str(
            &fs::read_to_string(dir.join("go").join(format!("{name}.json")))
                .unwrap_or_else(|_| panic!("{name}: no go golden; run tests/reachability/gen")),
        )
        .unwrap();
        let opts = json!({ "maxStates": go["max_states_asked"] }).to_string();
        let rs: Value =
            serde_json::from_str(&Core::new(&net).unwrap().reachability_json(&opts).unwrap())
                .unwrap();

        for k in EXACT {
            let (g, r) = (go.get(*k), rs.get(*k));
            let (g, r) = if *k == "first_deadlock" {
                (g.map(marking), r.map(marking))
            } else {
                (g.cloned(), r.cloned())
            };
            assert_eq!(r, g, "{name}: {k}");
        }
        assert_eq!(
            marking(&rs["max_tokens"]),
            marking(&go["max_tokens"]),
            "{name}: max_tokens"
        );
        assert_eq!(
            set(&rs["not_live"]),
            set(&go["not_live"]),
            "{name}: not_live"
        );
        assert_eq!(
            set(&rs["dead_transitions"]),
            set(&go["dead_transitions"]),
            "{name}: dead_transitions"
        );

        let (gw, rw) = (&go["witness"], &rs["witness"]);
        assert_eq!(gw.is_null(), rw.is_null(), "{name}: witness present");
        if !gw.is_null() {
            assert_eq!(rw["kind"], gw["kind"], "{name}: witness kind");
            assert_eq!(rw["region_size"], gw["region_size"], "{name}: region_size");
            assert_eq!(
                set(&rw["never_fires"]),
                set(&gw["never_fires"]),
                "{name}: never_fires"
            );
            // Both are shortest traces; each must replay to its own marking.
            assert_eq!(
                rw["trace"].as_array().unwrap().len(),
                gw["trace"].as_array().unwrap().len(),
                "{name}: trace length"
            );
            assert_eq!(
                marking(&replay(&net, &rw["trace"])),
                marking(&rw["marking"]),
                "{name}: rust witness replays"
            );
            assert_eq!(
                marking(&replay(&net, &gw["trace"])),
                marking(&gw["marking"]),
                "{name}: go witness replays"
            );
        }
        seen += 1;
    }
    assert!(seen >= 8, "only {seen} nets");
}
