//! `sde` and `what_if` against pflow-native.
//!
//! `tests/fixtures/native/workbench_goldens.json` holds the `pflow_sde` and
//! `pflow_what_if` cases of pflow-xyz's `parity/native/mcp_goldens.json`
//! (native's answer to each hosted call; see the README there). This replays
//! every case through [`pflow_hosted::call`] and compares the way the hosted
//! server's own test does (pflow-xyz
//! `cmd/webserver/mcp_workbench_parity_test.go`):
//!
//! Exact unless a case sets a `tolerance`: strings, booleans, null,
//! structure, and every number, each of the same JSON type on both sides.
//! The SDE and the scenario SSA are seeded portable engines, so their answers
//! are the same doubles. Two normalisations, both for place order (native
//! lists places in document order, this crate as the hosted server does, in
//! id order): within each group of a time series the series are sorted by
//! name, the groups staying in their order; the place columns of the
//! what-if's scenario × place table are matched by header and the rows of a
//! finals table by place. The place list in an SDE refusal's capacity clause
//! is compared as a set.

use std::path::Path;

use serde_json::{Map, Value};

fn fixtures() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native"
    ))
}

fn load(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The local name of a hosted tool.
fn local(tool: &str) -> &str {
    tool.strip_prefix("pflow_").unwrap_or(tool)
}

#[test]
fn workbench_matches_native() {
    let golden = load(&fixtures().join("workbench_goldens.json"));
    let cases = golden["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 32, "16 pflow_sde and 16 pflow_what_if cases");
    let mut failures = vec![];
    let mut answered = std::collections::BTreeSet::new();
    let (mut refusals, mut answers) = (0, 0);
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let tool = c["tool"].as_str().unwrap();
        let fixture = c["fixture"].as_str().unwrap();
        let base = Path::new(fixture).file_name().unwrap();
        let net = load(&fixtures().join("nets").join(base));
        let mut args = Map::new();
        args.insert("net".into(), net);
        for (k, v) in c["args"].as_object().unwrap() {
            args.insert(k.clone(), v.clone());
        }
        let got = pflow_hosted::call(local(tool), &Value::Object(args));
        let tol = c.get("tolerance").and_then(Value::as_f64).unwrap_or(0.0);
        match (c.get("error").and_then(Value::as_str), got) {
            (Some(want), Err(e)) => {
                refusals += 1;
                if normalize_refusal(&e) != normalize_refusal(want) {
                    failures.push(format!(
                        "{name}: error differs:\n tool:   {e}\n native: {want}"
                    ));
                }
            }
            (Some(want), Ok(_)) => failures.push(format!(
                "{name}: native refused ({want}); the tool answered"
            )),
            (None, Err(e)) => failures.push(format!("{name}: tool error: {e}")),
            (None, Ok(out)) => {
                answers += 1;
                // Through text, as a client receives it.
                let act: Value =
                    serde_json::from_str(&serde_json::to_string(&out).unwrap()).unwrap();
                let act = if tool == "pflow_what_if" {
                    align_place_columns(&c["expect"], act)
                } else {
                    act
                };
                let exp = normalize_order(&c["expect"]);
                let act = normalize_order(&act);
                let mut d = vec![];
                json_diff(&exp, &act, "", tol, &mut d);
                if !d.is_empty() {
                    let more = d.len().saturating_sub(20);
                    d.truncate(20);
                    if more > 0 {
                        d.push(format!("… {more} more"));
                    }
                    failures.push(format!("{name}: differs from native:\n{}", d.join("\n")));
                }
                answered.insert(tool.to_string());
            }
        }
    }
    for tool in ["pflow_sde", "pflow_what_if"] {
        if !answered.contains(tool) {
            failures.push(format!("{tool} has no successful golden case"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert_eq!((answers, refusals), (22, 10), "every case replayed");
}

/// Integers exact, a number with a fraction within `tol` relative (exact
/// when 0); strings, booleans, null and structure exact, each value of the
/// same JSON type on both sides.
fn json_diff(exp: &Value, act: &Value, path: &str, tol: f64, out: &mut Vec<String>) {
    match exp {
        Value::Number(e) => {
            let Some(a) = act.as_f64().filter(|_| act.is_number()) else {
                out.push(format!("{path}: {exp} != {act}"));
                return;
            };
            let e = e.as_f64().unwrap();
            if e == a {
                return;
            }
            if (e == e.trunc() && a == a.trunc()) || tol == 0.0 {
                out.push(format!("{path}: {e} != {a}"));
                return;
            }
            let scale = e.abs().max(a.abs());
            if (e - a).abs() > tol * scale.max(1e-12) {
                out.push(format!(
                    "{path}: {e} != {a} (rel {:.2e} > {tol})",
                    (e - a).abs() / scale
                ));
            }
        }
        Value::Array(e) => {
            let Some(a) = act.as_array() else {
                out.push(format!("{path}: array vs {act}"));
                return;
            };
            if e.len() != a.len() {
                out.push(format!("{path}: length {} != {}", e.len(), a.len()));
                return;
            }
            for (i, (x, y)) in e.iter().zip(a).enumerate() {
                json_diff(x, y, &format!("{path}[{i}]"), tol, out);
            }
        }
        Value::Object(e) => {
            let Some(a) = act.as_object() else {
                out.push(format!("{path}: object vs {act}"));
                return;
            };
            let mut ek: Vec<&String> = e.keys().collect();
            let mut ak: Vec<&String> = a.keys().collect();
            ek.sort();
            ak.sort();
            if ek != ak {
                out.push(format!("{path}: keys differ (native {ek:?}, tool {ak:?})"));
            }
            for k in ek {
                if let Some(v) = a.get(k) {
                    json_diff(&e[k], v, &format!("{path}.{k}"), tol, out);
                }
            }
        }
        _ => {
            if exp != act {
                out.push(format!("{path}: {exp} != {act}"));
            }
        }
    }
}

/// Sorts the place list of an SDE refusal's capacity clause.
fn normalize_refusal(s: &str) -> String {
    const OPEN: &str = "capacity is declared on [";
    let Some(i) = s.find(OPEN) else {
        return s.to_string();
    };
    let from = i + OPEN.len();
    let Some(len) = s[from..].find(']') else {
        return s.to_string();
    };
    let mut names: Vec<&str> = s[from..from + len].split_whitespace().collect();
    names.sort();
    format!("{}{}{}", &s[..from], names.join(" "), &s[from + len..])
}

/// Reorders the place columns of the what-if's scenario × place table to
/// native's order, matching them by header. Nothing else is permuted.
fn align_place_columns(exp: &Value, mut act: Value) -> Value {
    let (Some(ec), Some(ac)) = (
        exp["table"]["columns"].as_array(),
        act["table"]["columns"].as_array(),
    ) else {
        return act;
    };
    if ec.len() != ac.len() || ec.is_empty() || ec[0] != "scenario" || ac[0] != "scenario" {
        return act;
    }
    let Some(perm) = ec
        .iter()
        .map(|c| ac.iter().position(|x| x == c))
        .collect::<Option<Vec<usize>>>()
    else {
        return act;
    };
    let rows: Vec<Value> = act["table"]["rows"].as_array().cloned().unwrap_or_default();
    let mut new_rows = vec![];
    for row in rows {
        let cells = row.as_array().cloned().unwrap_or_default();
        if cells.len() != perm.len() {
            return act;
        }
        new_rows.push(Value::Array(
            perm.iter().map(|&j| cells[j].clone()).collect(),
        ));
    }
    act["table"]["columns"] = Value::Array(ec.clone());
    act["table"]["rows"] = Value::Array(new_rows);
    act
}

/// Sorts what is listed per place: within each group of a time series (the
/// groups in the order they first appear) the series by name, and the rows
/// of a finals table (first column "place") by place.
fn normalize_order(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out: Map<String, Value> = m
                .iter()
                .map(|(k, e)| (k.clone(), normalize_order(e)))
                .collect();
            if out.get("kind").and_then(Value::as_str) == Some("timeseries") {
                if let Some(Value::Array(s)) = out.get_mut("series") {
                    let mut groups: Vec<String> = vec![];
                    let mut keyed: Vec<(usize, String, Value)> = s
                        .drain(..)
                        .map(|e| {
                            let g = e["group"].as_str().unwrap_or("").to_string();
                            let gi = match groups.iter().position(|x| *x == g) {
                                Some(i) => i,
                                None => {
                                    groups.push(g);
                                    groups.len() - 1
                                }
                            };
                            (gi, e["name"].as_str().unwrap_or("").to_string(), e)
                        })
                        .collect();
                    keyed.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
                    *s = keyed.into_iter().map(|(_, _, e)| e).collect();
                }
            }
            let place_table = out
                .get("columns")
                .and_then(Value::as_array)
                .and_then(|c| c.first())
                .is_some_and(|c| c == "place");
            if place_table {
                if let Some(Value::Array(rows)) = out.get_mut("rows") {
                    rows.sort_by(|a, b| {
                        let k = |r: &Value| r[0].as_str().unwrap_or("").to_string();
                        k(a).cmp(&k(b))
                    });
                }
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(normalize_order).collect()),
        _ => v.clone(),
    }
}
