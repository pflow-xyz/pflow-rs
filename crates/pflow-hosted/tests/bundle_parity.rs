//! The composition tools against pflow-native: every `pflow_bundle_*` case of
//! pflow-xyz's `parity/native/mcp_goldens.json` (copied, with its fixtures,
//! under `tests/fixtures/native/`; see the README there), replayed through
//! [`pflow_hosted::call`] and compared as pflow-xyz's
//! `cmd/webserver/mcp_models_parity_test.go` compares the hosted tools:
//!
//! - answers exactly: strings, booleans, null, structure and every integer,
//!   each of the same JSON type on both sides (a bundle has no
//!   floating-point answer). The one key not compared is `notes` on
//!   flatten and incidence, the one key the tools add to native's answer;
//! - a refusal by native's words: the tool names itself in front
//!   (`pflow_bundle_x: `) and may add a hint behind (`. subnet …`), and the
//!   words between must be native's;
//! - a `composition` case runs on the bundle native built (the goldens'
//!   `compositions`) and again on the one `bundle_example` builds from the
//!   same instances and links: both must give native's answer;
//! - `bundle_example` is compared field by field: everything but `bundle`
//!   exactly, and the bundle's subnets (places, transitions, arcs, ports,
//!   constraints) and links against native's.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pflow_compose::Bundle;
use serde_json::{json, Map, Value};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native")
}

fn load(name: &str) -> Value {
    let path = fixtures().join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// The local tool a hosted one is.
fn local(tool: &str) -> &str {
    tool.strip_prefix("pflow_").expect("a pflow_ tool")
}

/// Differences between `want` and `got`, by path.
fn diff(want: &Value, got: &Value, path: &str, out: &mut Vec<String>) {
    match (want, got) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for k in keys {
                let p = format!("{path}.{k}");
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => diff(x, y, &p, out),
                    (Some(x), None) => out.push(format!("{p}: missing (native: {x})")),
                    (None, Some(y)) => out.push(format!("{p}: extra ({y})")),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                out.push(format!("{path}: {} items, native has {}", b.len(), a.len()));
            }
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                diff(x, y, &format!("{path}[{i}]"), out);
            }
        }
        (Value::Number(a), Value::Number(b)) => {
            let same = match (a.as_i64(), b.as_i64(), a.as_u64(), b.as_u64()) {
                (Some(x), Some(y), _, _) => x == y,
                (_, _, Some(x), Some(y)) => x == y,
                _ => a.is_f64() && b.is_f64() && a.as_f64() == b.as_f64(),
            };
            if !same {
                out.push(format!("{path}: {b}, native {a}"));
            }
        }
        _ if want == got => {}
        _ => out.push(format!("{path}: {got}, native {want}")),
    }
}

fn assert_same(what: &str, want: &Value, got: &Value) {
    let mut d = vec![];
    diff(want, got, "", &mut d);
    if !d.is_empty() {
        let n = d.len();
        d.truncate(20);
        panic!("{what}: differs from native ({n}):\n{}", d.join("\n"));
    }
}

/// The words of a refusal between the tool's name and its hint.
fn native_words<'a>(tool: &str, err: &'a str, want: &str) -> &'a str {
    let rest = err
        .strip_prefix(&format!("{tool}: "))
        .unwrap_or_else(|| panic!("the refusal does not name {tool}: {err}"));
    match rest.strip_prefix(want) {
        Some(tail) if tail.is_empty() || tail.starts_with(". ") => &rest[..want.len()],
        _ => rest,
    }
}

fn compare(what: &str, case: &Value, got: Result<Value, String>) {
    let tool = case["tool"].as_str().unwrap();
    if let Some(want) = case.get("error").and_then(Value::as_str) {
        let err = match got {
            Ok(_) => panic!("{what}: native refused ({want}); the tool answered"),
            Err(e) => e,
        };
        let words = native_words(tool, &err, want);
        assert_eq!(
            words, want,
            "{what}: error differs:\n tool:   {err}\n native: {want}"
        );
        return;
    }
    let mut got = got.unwrap_or_else(|e| panic!("{what}: tool error: {e}"));
    if tool == "pflow_bundle_flatten" || tool == "pflow_bundle_incidence" {
        if let Some(m) = got.as_object_mut() {
            m.remove("notes");
        }
    }
    assert_same(what, &case["expect"], &got);
}

fn place_view(p: &pflow_metamodel::Place) -> Value {
    json!({
        "id": p.id, "kind": if p.is_token() { "token" } else { "data" },
        "initial": p.initial, "capacity": p.capacity, "exported": p.exported,
        "resource": p.resource, "description": p.description,
    })
}

/// A bundle's subnets reduced to what native's views hold.
fn subnet_views(bundle: &Value) -> Value {
    let b: Bundle = serde_json::from_value(bundle.clone()).expect("the example's bundle reads");
    let views: Vec<Value> = b
        .subnets
        .iter()
        .map(|s| {
            let ports: Vec<Value> = s
                .ports
                .iter()
                .map(|p| {
                    let kind = p
                        .kind
                        .map(|k| serde_json::to_value(k).unwrap())
                        .unwrap_or(json!(""));
                    json!({
                        "id": p.id, "kind": kind,
                        "target": if p.is_transition() { "transition" } else { "place" },
                        "place": p.place, "transition": p.transition, "schema": p.schema,
                    })
                })
                .collect();
            let m = &s.model;
            json!({
                "id": s.id, "net_type": s.net_type.to_string(), "model_name": m.name,
                "ports": ports,
                "places": m.places.iter().map(place_view).collect::<Vec<_>>(),
                "transitions": m.transitions.iter().map(|t| json!({"id": t.id, "description": t.description})).collect::<Vec<_>>(),
                "arcs": m.arcs.iter().map(|a| json!({"from": a.from, "to": a.to, "weight": a.weight, "type": match a.typ {
                    pflow_metamodel::ArcType::Normal => "",
                    pflow_metamodel::ArcType::Inhibitor => "inhibitor",
                    pflow_metamodel::ArcType::Read => "read",
                }})).collect::<Vec<_>>(),
                "constraints": m.constraints.iter().map(|c| json!({"id": c.id, "expr": c.expr})).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!(views)
}

/// The cases the goldens hold, by tool (`wantBundleCases`).
const WANT: [(&str, usize); 7] = [
    ("pflow_bundle_example", 8),
    ("pflow_bundle_validate", 15),
    ("pflow_bundle_add_link", 30),
    ("pflow_bundle_candidates", 39),
    ("pflow_bundle_flatten", 15),
    ("pflow_bundle_run", 35),
    ("pflow_bundle_incidence", 12),
];

#[test]
fn bundle_tools_match_native() {
    let g = load("bundle_goldens.json");
    let comps = g["compositions"].as_object().expect("compositions");
    let cases = g["cases"].as_array().expect("cases");
    let mut passed: BTreeMap<&str, usize> = BTreeMap::new();
    let mut answered: BTreeMap<&str, usize> = BTreeMap::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let tool = case["tool"].as_str().unwrap();
        let args = case
            .get("args")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if tool == "pflow_bundle_example" {
            let got = pflow_hosted::call(local(tool), &Value::Object(args));
            if case.get("error").is_some() {
                compare(name, case, got);
            } else {
                let mut got = got.unwrap_or_else(|e| panic!("{name}: tool error: {e}"));
                let bundle = got
                    .as_object_mut()
                    .unwrap()
                    .remove("bundle")
                    .expect("a bundle");
                assert_same(
                    &format!("{name}: the catalog and the scenarios"),
                    &case["expect"],
                    &got,
                );
                assert_same(
                    &format!("{name}: the subnets"),
                    &case["subnets"],
                    &subnet_views(&bundle),
                );
                let links = bundle.get("links").cloned().unwrap_or(json!([]));
                assert_same(&format!("{name}: the links"), &case["links"], &links);
            }
        } else if let Some(file) = case.get("bundle_file").and_then(Value::as_str) {
            let base = file.rsplit('/').next().unwrap();
            let mut a: Map<String, Value> = args.clone();
            a.insert("bundle".into(), load(base));
            let got = pflow_hosted::call(local(tool), &Value::Object(a));
            compare(&format!("{name} (on the document)"), case, got);
        } else {
            let cname = case["composition"]
                .as_str()
                .expect("a composition or a bundle_file");
            let comp = &comps[cname];
            // native's own subnets, as native serialised them
            let mut a = args.clone();
            a.insert("bundle".into(), comp["bundle"].clone());
            let got = pflow_hosted::call(local(tool), &Value::Object(a.clone()));
            compare(&format!("{name} (on native's bundle)"), case, got);
            // and the ones bundle_example builds from the same instances and links
            let built = pflow_hosted::call("bundle_example", &comp["args"])
                .unwrap_or_else(|e| panic!("{name}: bundle_example: {e}"));
            a.insert("bundle".into(), built["bundle"].clone());
            let got = pflow_hosted::call(local(tool), &Value::Object(a));
            compare(&format!("{name} (on bundle_example's bundle)"), case, got);
        }
        *passed.entry(tool).or_default() += 1;
        if case.get("error").is_none() {
            *answered.entry(tool).or_default() += 1;
        }
    }
    for (tool, want) in WANT {
        assert_eq!(
            passed.get(tool).copied().unwrap_or(0),
            want,
            "{tool}: cases in the goldens"
        );
        assert!(
            answered.get(tool).copied().unwrap_or(0) > 0,
            "{tool} has no successful case"
        );
        assert!(
            pflow_hosted::TOOLS.contains(&local(tool)),
            "{tool} is not in TOOLS"
        );
    }
    eprintln!("bundle parity, cases passed by tool: {passed:?}");
}

/// The bundle each named composition holds is the one bundle_example builds.
#[test]
fn example_builds_natives_compositions() {
    let g = load("bundle_goldens.json");
    for (name, comp) in g["compositions"].as_object().unwrap() {
        let built = pflow_hosted::call("bundle_example", &comp["args"]).unwrap();
        assert_same(
            &format!("composition {name}"),
            &comp["bundle"],
            &built["bundle"],
        );
    }
}
