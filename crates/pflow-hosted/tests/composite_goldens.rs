//! Goldens for `bundle_net`, `bundle_prove` and `bundle_parts`.
//!
//! These tools exist in pflow-rs first; the files here are what go-pflow's
//! port (and pflow-xyz's hosted tools) are held to. `composite.json` holds
//! the compositions (built with `bundle_parts`), and for each the answers of
//! `bundle_net`, `bundle_prove` and `bundle_check_proof`.
//!
//! Regenerate (a reviewed change to the tools, never to make a test pass):
//! `REGEN_COMPOSITE=1 cargo test -p pflow-hosted --test composite_goldens`.

use pflow_hosted::call;
use serde_json::{json, Value};
use std::path::PathBuf;

fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/composite/composite.json")
}

fn cases() -> Vec<(&'static str, Value)> {
    let inst = |l: &[(&str, &str)]| -> Value { json!(l.iter().map(|(id, p)| json!({"id": id, "part": p})).collect::<Vec<_>>()) };
    let tok = |a: (&str, &str), b: (&str, &str)| json!({"kind":"token","from":{"subnet":a.0,"port":a.1},"to":{"subnet":b.0,"port":b.1}});
    let ev = |a: (&str, &str), b: (&str, &str)| json!({"kind":"event","from":{"subnet":a.0,"port":a.1},"to":{"subnet":b.0,"port":b.1}});
    let gd = |a: (&str, &str), b: (&str, &str)| json!({"kind":"guard","from":{"subnet":a.0,"port":a.1},"to":{"subnet":b.0,"port":b.1}});
    vec![
        ("default", json!({"instances": inst(&[("orders","queue"),("workers","resource-pool"),("payment","saga")]),
            "links": [ev(("orders","dequeue"),("workers","acquire")), ev(("orders","dequeue"),("payment","do:reserve")), ev(("workers","release"),("payment","commit"))]})),
        ("queue-chain", json!({"instances": inst(&[("a","queue"),("b","queue")]), "links": [tok(("a","out"),("b","in"))]})),
        ("pool-failover", json!({"instances": inst(&[("pool","resource-pool"),("machines","failover")]), "links": [tok(("machines","up"),("pool","available"))]})),
        ("bucket-pool", json!({"instances": inst(&[("pool","resource-pool"),("limiter","token_bucket")]), "links": [tok(("limiter","tokens"),("pool","available"))]})),
        ("saga-queue", json!({"instances": inst(&[("q","queue"),("pay","saga")]), "links": [ev(("q","dequeue"),("pay","do:reserve"))]})),
        ("batch-line", json!({"instances": inst(&[("q","queue"),("pack","batcher"),("ship","kanban")]), "links": [tok(("q","out"),("pack","in")), tok(("pack","batches"),("ship","ready"))]})),
        ("light-gate", json!({"instances": inst(&[("q","queue"),("light","traffic_light")]), "links": [gd(("q","dequeue"),("light","green"))]})),
        ("breaker-bucket", json!({"instances": inst(&[("q","queue"),("cb","circuit_breaker"),("tb","token_bucket"),("lock","mutex")]),
            "links": [ev(("q","dequeue"),("cb","call")), ev(("q","dequeue"),("tb","admit")), ev(("q","dequeue"),("lock","lock"))]})),
        ("epidemic", json!({"instances": inst(&[("flu","sir")]), "links": []})),
    ]
}

fn build() -> Value {
    let mut out = vec![];
    for (name, spec) in cases() {
        let b = call("bundle_parts", &spec).unwrap_or_else(|e| panic!("{name}: {e}"))["bundle"].clone();
        let net = call("bundle_net", &json!({"bundle": b})).unwrap();
        let proof = call("bundle_prove", &json!({"bundle": b})).unwrap();
        let check = call("bundle_check_proof", &json!({"bundle": b, "proof": proof})).unwrap();
        assert_eq!(check["valid"], true, "{name}: {}", check["failed"]);
        out.push(json!({"name": name, "spec": spec, "bundle": b, "net": net, "prove": proof, "check_valid": true}));
    }
    json!({
        "about": "Reference answers of pflow-rs's bundle_net / bundle_prove / bundle_check_proof / bundle_parts. See composite_goldens.rs.",
        "catalog": call("bundle_parts", &json!({})).unwrap(),
        "cases": out,
    })
}

#[test]
fn goldens_replay() {
    let got = build();
    if std::env::var("REGEN_COMPOSITE").is_ok() {
        std::fs::write(path(), serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
        return;
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(path()).expect("run with REGEN_COMPOSITE=1 once")).unwrap();
    assert_eq!(got["catalog"], want["catalog"], "catalog");
    for (g, w) in got["cases"].as_array().unwrap().iter().zip(want["cases"].as_array().unwrap()) {
        assert_eq!(g, w, "case {}", g["name"]);
    }
}

/// Records written by another implementation (go-pflow) must check here.
#[test]
fn foreign_records_check() {
    let Ok(p) = std::env::var("FOREIGN_RECORDS") else { return };
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    for c in v["cases"].as_array().unwrap() {
        let r = call("bundle_check_proof", &json!({"bundle": c["bundle"], "proof": c["prove"]})).unwrap();
        assert_eq!(r["valid"], true, "{}: {}", c["name"], r["failed"]);
    }
}
