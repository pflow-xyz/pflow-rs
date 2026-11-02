//! The single-net tools (`check`, `cid`, `incidence`, `invariants`,
//! `validate`, `verify`, `export`, `info`) against pflow.xyz/mcp.
//!
//! `tests/fixtures/hosted/*.json` are the hosted server's own answers
//! (see that directory's README); [`every_hosted_answer_is_reproduced`]
//! replays each one locally and asserts the answer is the same, except where
//! [`documented_difference`] says why it cannot be. The named tests below pin
//! one behaviour per tool on top of that.

use std::path::{Path, PathBuf};

use pflow_hosted::{call, TOOLS};
use serde_json::{json, Value};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn net(name: &str) -> Value {
    let path = fixtures().join("nets").join(name);
    serde_json::from_slice(
        &std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    )
    .unwrap()
}

fn with_net(name: &str, extra: Value) -> Value {
    let mut args = extra;
    args["net"] = net(name);
    args
}

fn ok(tool: &str, args: &Value) -> Value {
    call(tool, args).unwrap_or_else(|e| panic!("{tool}: {e}"))
}

/// go-pflow's validator ranges over Go maps, so the hosted order of issues
/// within one list varies between calls; compare them as sets.
fn sort_issues(v: &mut Value) {
    for key in ["errors", "warnings", "info"] {
        if let Some(Value::Array(list)) = v.get_mut(key) {
            list.sort_by_key(|x| x.to_string());
        }
    }
}

/// Why a hosted answer cannot be reproduced byte for byte, or `None`.
fn documented_difference(file: &str) -> Option<&'static str> {
    if file.starts_with("export-") && file.ends_with("-svg.json") {
        return Some("svg: pflow-visualization draws, at the net's own coordinates; pflow.xyz's internal/svg draws with the layout");
    }
    if file == "cid-bundle-solo.json" {
        return Some("cid: a composition seals its subnets list as a JSON-LD set, which the local sealer refuses");
    }
    None
}

#[test]
fn every_hosted_answer_is_reproduced() {
    let dir = fixtures().join("hosted");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|f| f.ends_with(".json"))
        .collect();
    files.sort();
    assert!(files.len() >= 24, "only {} hosted answers", files.len());

    let mut checked = 0;
    for file in &files {
        let case: Value = serde_json::from_slice(&std::fs::read(dir.join(file)).unwrap()).unwrap();
        let tool = case["tool"]
            .as_str()
            .unwrap()
            .strip_prefix("pflow_")
            .unwrap();
        let args = with_net(case["net"].as_str().unwrap(), case["arguments"].clone());
        let local = call(tool, &args);

        if let Some(why) = documented_difference(file) {
            match (tool, &local) {
                ("export", Ok(v)) => {
                    assert_eq!(v["format"], "svg", "{file}");
                    let svg = v["svg"].as_str().unwrap();
                    assert!(
                        svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"),
                        "{file}"
                    );
                    assert_eq!(case["answer"]["format"], "svg", "{file}");
                }
                ("cid", Err(e)) => assert!(e.contains("https://pflow.xyz/mcp"), "{file}: {e}"),
                _ => panic!("{file}: expected the documented difference ({why}), got {local:?}"),
            }
            continue;
        }

        match (case.get("error"), local) {
            (Some(want), Err(got)) => assert_eq!(want.as_str().unwrap(), got, "{file}"),
            (Some(want), Ok(got)) => {
                panic!("{file}: hosted refused ({want}), local answered {got}")
            }
            (None, Err(got)) => panic!("{file}: hosted answered, local refused: {got}"),
            (None, Ok(mut got)) => {
                let mut want = case["answer"].clone();
                if tool == "validate" {
                    sort_issues(&mut want);
                    sort_issues(&mut got);
                }
                assert_eq!(want, got, "{file}");
            }
        }
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn tools_are_listed_and_unknown_ones_refused() {
    for t in [
        "check",
        "cid",
        "incidence",
        "invariants",
        "validate",
        "verify",
        "export",
        "info",
    ] {
        assert!(TOOLS.contains(&t), "{t}");
    }
    assert!(call("no_such_tool", &json!({})).is_err());
    assert_eq!(
        call("check", &json!({})).unwrap_err(),
        "net: a JSON object (the editor's format) is required"
    );
}

#[test]
fn check_reports_a_missing_context_and_counts() {
    let v = ok(
        "check",
        &json!({"net": {"places": {"p": {}}, "transitions": {"t": {}}, "arcs": [{"source": "p", "target": "t"}]}}),
    );
    assert_eq!(
        v,
        json!({"ok": false, "jsonld": "missing @context field", "places": 1, "transitions": 1, "arcs": 1})
    );
}

/// The cross-language CID contract: pflow-xyz's `parity/golden.json`.
#[test]
fn cid_matches_pflow_xyz_golden() {
    let golden = net("golden.json");
    let mut n = 0;
    for (fixture, cid) in golden.as_object().unwrap() {
        if fixture.starts_with('_') {
            continue;
        }
        let got = ok("cid", &with_net(fixture, json!({})));
        assert_eq!(got, json!({"cid": cid}), "{fixture}");
        // The top-level @id is not part of the seal.
        let mut args = with_net(fixture, json!({}));
        args["net"]["@id"] = json!("zSomethingElse");
        assert_eq!(ok("cid", &args)["cid"], *cid, "{fixture} with @id");
        n += 1;
    }
    assert_eq!(n, 6);
}

#[test]
fn cid_of_a_bare_wrapper_is_its_part() {
    let v = ok("cid", &with_net("local/bare.json", json!({})));
    assert_eq!(
        v,
        json!({"cid": "z4EBG9jA7f4q9yx4QjzH3RDPQXiqjnNTz3ALdpjHD3CRTbNrcCD", "kind": "net", "reduced": true})
    );
}

#[test]
fn incidence_unfolds_colors_and_skips_tests() {
    let v = ok("incidence", &with_net("gated.jsonld", json!({})));
    assert_eq!(v["places"], json!(["Done", "Lock", "Queue"]));
    let v = ok("incidence", &with_net("colored.jsonld", json!({})));
    assert_eq!(v["places"][0], "Done.https://pflow.xyz/tokens/blue");
    assert_eq!(v["transitions"], json!(["combine", "convert", "work"]));
}

#[test]
fn invariants_render_as_go_pflow_does() {
    let v = ok("invariants", &with_net("sir.jsonld", json!({})));
    assert_eq!(v["p_invariants"][0]["equation"], "I + R + S == 10");
    assert_eq!(
        v["t_invariants"],
        Value::Null,
        "Go marshals an empty list as null"
    );
    assert_eq!(v["conservative"], true);
    assert_eq!(
        call("invariants", &with_net("sir.jsonld", json!({"kind": "x"}))).unwrap_err(),
        "kind: p, t or both"
    );
}

#[test]
fn validate_bounds_its_state_budget() {
    let v = ok(
        "validate",
        &with_net("sir.jsonld", json!({"reachability": true})),
    );
    assert_eq!(v["reachability"]["reachable"], 65);
    let e = call(
        "validate",
        &with_net(
            "sir.jsonld",
            json!({"reachability": true, "max_states": 20001}),
        ),
    )
    .unwrap_err();
    assert_eq!(e, "max_states: a number in [1, 20000]");
}

#[test]
fn verify_answers_an_unknown_kind_as_unknown() {
    let v = ok(
        "verify",
        &with_net(
            "sir.jsonld",
            json!({"properties": [{"kind": "nope", "name": "n"}]}),
        ),
    );
    assert_eq!(
        v["verdicts"][0],
        json!({"property": {"kind": "nope", "name": "n"}, "status": "unknown", "method": "", "detail": "unknown property kind \"nope\""})
    );
    assert_eq!(v["ok"], false);
}

#[test]
fn verify_refuses_a_base_name_target_on_a_colored_net() {
    let args = with_net(
        "colored.jsonld",
        json!({"properties": [{"kind": "reachable", "target": {"Pool": 30}}]}),
    );
    let e = call("verify", &args).unwrap_err();
    assert!(
        e.contains("https://pflow.xyz/mcp") && e.contains("Pool.https://pflow.xyz/tokens/red"),
        "{e}"
    );
}

#[test]
fn export_json_carries_the_seal_and_svg_draws_every_node() {
    let v = ok("export", &with_net("tie.jsonld", json!({})));
    assert_eq!(
        v["cid"],
        ok("cid", &with_net("tie.jsonld", json!({})))["cid"]
    );
    let v = ok(
        "export",
        &with_net(
            "sir.jsonld",
            json!({"format": "svg", "layout": "hierarchical"}),
        ),
    );
    let svg = v["svg"].as_str().unwrap();
    for id in ["S", "I", "R", "infect", "recover"] {
        assert!(svg.contains(&format!(">{id}<")), "{id} missing");
    }
}

#[test]
fn info_lists_marking_and_enabled() {
    let v = ok("info", &with_net("sir.jsonld", json!({})));
    assert_eq!(v["marking"], json!({"I": 1, "R": 0, "S": 9}));
    assert_eq!(v["enabled"], json!(["infect", "recover"]));
    assert_eq!(v["colors"], 1);
}
