//! The example set (`verdicts/cases.json`) run natively, held to what the
//! WASM build recorded (`verdicts/recorded.json`, the reference).
//!
//! Every op is exact, bit for bit, except the ODE (`simulate`): its adaptive
//! step control calls `f64::powf`, which natively is the platform libm's `pow`
//! (glibc, Apple libm) and in the module is Rust's own libm, compiled in. They
//! can differ in the last ulp, which moves a step and so every later point.
//! Natively, a `simulate` mismatch is printed, not failed; the module, which
//! carries its `pow` with it, is the reference on every host
//! (docs/reproducible-engine.md, "Float drift").

use serde_json::Value;
use std::path::Path;

const NATIVE_DRIFT_OPS: &[&str] = &["simulate"];

#[test]
fn native_engine_matches_the_recorded_wasm_verdicts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("verdicts");
    let cases = std::fs::read(dir.join("cases.json")).unwrap();
    let recorded: Value =
        serde_json::from_slice(&std::fs::read(dir.join("recorded.json")).unwrap()).unwrap();
    let got: Value = serde_json::from_str(&pflow_engine::cases::run_all(&cases, &|p| {
        std::fs::read(dir.join(p)).map_err(|e| e.to_string())
    }))
    .unwrap();
    let ops: std::collections::HashMap<String, String> =
        serde_json::from_slice::<Value>(&cases).unwrap()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["id"].as_str().unwrap().into(), c["op"].as_str().unwrap().into()))
            .collect();
    let want = recorded["results"].as_object().unwrap();
    let have = got["results"].as_object().unwrap();
    assert_eq!(
        want.keys().collect::<Vec<_>>(),
        have.keys().collect::<Vec<_>>(),
        "the case ids differ from recorded.json: re-record (check-verdicts.mjs --record)"
    );
    let mut bad = Vec::new();
    for (id, w) in want {
        if have[id] == *w {
            continue;
        }
        if NATIVE_DRIFT_OPS.contains(&ops[id].as_str()) {
            eprintln!("native drift (powf; the module is the reference): {id}");
        } else {
            bad.push(format!("{id}: recorded {w}, native {}", have[id]));
        }
    }
    assert!(bad.is_empty(), "native differs from the WASM reference:\n{}", bad.join("\n"));
}

#[test]
fn a_wrong_cid_is_refused() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pflow-hosted/tests/fixtures/nets");
    let bytes = std::fs::read(dir.join("coffee-shop.jsonld")).unwrap();
    let right = "z4EBG9jDsuUdvVN4bUXRHxoDG3p274FfhcJd8BiruRcE1jLCHi8";
    assert_eq!(pflow_engine::cid_of(&bytes).unwrap(), right);
    let m = pflow_engine::Model::load(&bytes, right).unwrap();
    let env: Value = serde_json::from_str(&m.run("check", "").unwrap()).unwrap();
    assert_eq!(env["cid"], right);
    // One changed weight is another model.
    let mut doc: Value = serde_json::from_slice(&bytes).unwrap();
    doc["arcs"][0]["weight"][0] = 2.into();
    let edited = serde_json::to_vec(&doc).unwrap();
    let err = pflow_engine::Model::load(&edited, right).err().unwrap();
    assert!(err.starts_with("cid mismatch"), "{err}");
}

#[test]
fn a_hosted_op_may_not_bring_its_own_net() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pflow-hosted/tests/fixtures/nets");
    let bytes = std::fs::read(dir.join("sir.jsonld")).unwrap();
    let m = pflow_engine::Model::load(&bytes, &pflow_engine::cid_of(&bytes).unwrap()).unwrap();
    assert!(m.run("check", r#"{"net":{}}"#).is_err());
}

#[test]
fn the_golden_cids_are_pflow_xyz_s() {
    // pflow-xyz parity/golden.json: the cross-language CID contract.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pflow-hosted/tests/fixtures/nets");
    let golden: Value =
        serde_json::from_slice(&std::fs::read(dir.join("golden.json")).unwrap()).unwrap();
    for (file, cid) in golden.as_object().unwrap() {
        if file.starts_with('_') {
            continue;
        }
        let bytes = std::fs::read(dir.join(file)).unwrap();
        assert_eq!(pflow_engine::cid_of(&bytes).unwrap(), cid.as_str().unwrap(), "{file}");
    }
}
