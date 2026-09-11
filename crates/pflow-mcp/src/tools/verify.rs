//! `petri_verify`: checks declarative properties against a model. Mirrors
//! petri-pilot's `pkg/mcp/verify.go` — same shorthand grammar
//! (`"deadlock-free"`, `"reachable:a=1,b=1"`, `"mutex:a,b<=2"`, a bare
//! linear expression) and object form (`{"kind":"invariant","expr":"..."}`),
//! same JSON response shape (`Report` plus a `summary` string).

use super::convert::parse_any_model;
use pflow_verify::property::{Kind, Property};
use pflow_verify::Verifier;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
struct VerdictOut {
    property: String,
    status: &'static str,
    method: &'static str,
    detail: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    evidence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    counterexample: Option<CounterexampleOut>,
}

#[derive(Serialize)]
struct CounterexampleOut {
    trace: Vec<String>,
    marking: HashMap<String, i64>,
    explanation: String,
}

#[derive(Serialize)]
struct ReportOut {
    verdicts: Vec<VerdictOut>,
    proved: usize,
    refuted: usize,
    unknown: usize,
    ok: bool,
    state_count: usize,
    truncated: bool,
    summary: String,
}

pub fn run(model: &str, properties_json: &str, max_states: Option<usize>) -> Result<String, String> {
    let model = parse_any_model(model)?;
    let props = parse_properties(properties_json)?;
    if props.is_empty() {
        return Err("no properties given — supply at least one, e.g. [\"deadlock-free\"]".to_string());
    }

    let mut v = Verifier::new(&model);
    if let Some(n) = max_states {
        v = v.with_max_states(n);
    }
    let report = v.check(&props);

    let verdicts = report
        .verdicts
        .iter()
        .map(|verdict| VerdictOut {
            property: property_label(&verdict.property),
            status: match verdict.status {
                pflow_verify::property::Status::Proved => "proved",
                pflow_verify::property::Status::Refuted => "refuted",
                pflow_verify::property::Status::Unknown => "unknown",
            },
            method: match verdict.method {
                pflow_verify::property::Method::Structural => "structural",
                pflow_verify::property::Method::Exhaustive => "exhaustive",
                pflow_verify::property::Method::Witness => "witness",
                pflow_verify::property::Method::Partial => "partial",
                pflow_verify::property::Method::None => "none",
            },
            detail: verdict.detail.clone(),
            evidence: verdict.evidence.clone(),
            counterexample: verdict.counterexample.as_ref().map(|c| CounterexampleOut {
                trace: c.trace.clone(),
                marking: c.marking.clone(),
                explanation: c.explanation.clone(),
            }),
        })
        .collect();

    let out = ReportOut {
        verdicts,
        proved: report.proved,
        refuted: report.refuted,
        unknown: report.unknown,
        ok: report.ok,
        state_count: report.state_count,
        truncated: report.truncated,
        summary: report.summary(),
    };

    serde_json::to_string_pretty(&out).map_err(|e| format!("Serialization error: {e}"))
}

fn property_label(p: &Property) -> String {
    if !p.name.is_empty() {
        p.name.clone()
    } else {
        format!("{:?}", p.kind)
    }
}

/// Parses the JSON array `petri_verify` accepts: entries are either
/// shorthand strings or `{"kind":...}` objects.
fn parse_properties(src: &str) -> Result<Vec<Property>, String> {
    let raw: Vec<serde_json::Value> =
        serde_json::from_str(src).map_err(|e| format!("properties must be a JSON array: {e}"))?;

    raw.into_iter()
        .enumerate()
        .map(|(i, entry)| {
            if let Some(s) = entry.as_str() {
                parse_shorthand(s).map_err(|e| format!("property {i} ({s:?}): {e}"))
            } else {
                parse_object(&entry).map_err(|e| format!("property {i}: {e}"))
            }
        })
        .collect()
}

fn parse_object(v: &serde_json::Value) -> Result<Property, String> {
    let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("").trim();
    let name = v.get("name").and_then(|k| k.as_str()).unwrap_or("").to_string();
    match kind {
        "deadlock-free" => Ok(Property { kind: Some(Kind::DeadlockFree), name, ..Default::default() }),
        "bounded" => Ok(Property { kind: Some(Kind::Bounded), name, ..Default::default() }),
        "live" => Ok(Property { kind: Some(Kind::Live), name, ..Default::default() }),
        "terminating" => Ok(Property { kind: Some(Kind::Terminating), name, ..Default::default() }),
        "conserves" => Ok(Property { kind: Some(Kind::Conserves), name, ..Default::default() }),
        "reachable" | "unreachable" => {
            let target = parse_target(v.get("target"))?;
            if target.is_empty() {
                return Err(format!("{kind} requires a non-empty \"target\" marking"));
            }
            let k = if kind == "reachable" { Kind::Reachable } else { Kind::Unreachable };
            Ok(Property { kind: Some(k), name, target, ..Default::default() })
        }
        "invariant" => {
            let expr = v.get("expr").and_then(|e| e.as_str()).unwrap_or("").to_string();
            if expr.trim().is_empty() {
                return Err("invariant requires an \"expr\"".to_string());
            }
            pflow_verify::property::parse_expr(&expr).map_err(|e| e.to_string())?;
            Ok(Property { kind: Some(Kind::Invariant), name, expr, ..Default::default() })
        }
        "mutual-exclusion" => {
            let places: Vec<String> = v
                .get("places")
                .and_then(|p| p.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            if places.is_empty() {
                return Err("mutual-exclusion requires \"places\"".to_string());
            }
            let bound = v.get("bound").and_then(|b| b.as_i64()).unwrap_or(0);
            Ok(Property { kind: Some(Kind::MutualExclusion), name, places, bound, ..Default::default() })
        }
        "" => Err("missing \"kind\"".to_string()),
        other => Err(format!("unknown kind {other:?}")),
    }
}

fn parse_target(v: Option<&serde_json::Value>) -> Result<HashMap<String, i64>, String> {
    let obj = match v.and_then(|v| v.as_object()) {
        Some(o) => o,
        None => return Ok(HashMap::new()),
    };
    obj.iter()
        .map(|(k, v)| {
            v.as_i64()
                .map(|n| (k.clone(), n))
                .ok_or_else(|| format!("bad token count for {k:?}"))
        })
        .collect()
}

fn parse_shorthand(spec: &str) -> Result<Property, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("empty property".to_string());
    }

    match spec.to_lowercase().as_str() {
        "deadlock-free" | "deadlockfree" => {
            return Ok(Property { kind: Some(Kind::DeadlockFree), name: spec.to_string(), ..Default::default() })
        }
        "bounded" => {
            return Ok(Property { kind: Some(Kind::Bounded), name: spec.to_string(), ..Default::default() })
        }
        "live" | "liveness" => {
            return Ok(Property { kind: Some(Kind::Live), name: spec.to_string(), ..Default::default() })
        }
        "terminating" | "terminates" => {
            return Ok(Property { kind: Some(Kind::Terminating), name: spec.to_string(), ..Default::default() })
        }
        "conserves" | "conservation" => {
            return Ok(Property { kind: Some(Kind::Conserves), name: spec.to_string(), ..Default::default() })
        }
        _ => {}
    }

    if let Some(body) = spec.strip_prefix("reachable:") {
        let target = parse_marking_shorthand(body)?;
        return Ok(Property { kind: Some(Kind::Reachable), name: spec.to_string(), target, ..Default::default() });
    }
    if let Some(body) = spec.strip_prefix("unreachable:") {
        let target = parse_marking_shorthand(body)?;
        return Ok(Property { kind: Some(Kind::Unreachable), name: spec.to_string(), target, ..Default::default() });
    }
    if let Some(body) = spec.strip_prefix("mutex:") {
        let mut bound = 1i64;
        let mut places_part = body;
        if let Some(idx) = body.find("<=") {
            let n: i64 = body[idx + 2..]
                .trim()
                .parse()
                .map_err(|_| format!("bad bound after '<=' in {body:?}"))?;
            bound = n;
            places_part = &body[..idx];
        }
        let places: Vec<String> = places_part
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        if places.is_empty() {
            return Err("mutex requires at least one place".to_string());
        }
        return Ok(Property {
            kind: Some(Kind::MutualExclusion),
            name: spec.to_string(),
            places,
            bound,
            ..Default::default()
        });
    }

    // Fall through: a bare linear expression, e.g. "a + 2*b == 10".
    pflow_verify::property::parse_expr(spec).map_err(|e| e.to_string())?;
    Ok(Property { kind: Some(Kind::Invariant), name: spec.to_string(), expr: spec.to_string(), ..Default::default() })
}

fn parse_marking_shorthand(s: &str) -> Result<HashMap<String, i64>, String> {
    let mut target = HashMap::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut kv = part.splitn(2, '=');
        let key = kv.next().unwrap_or("").trim();
        let val = kv.next().ok_or_else(|| format!("expected place=tokens, got {part:?}"))?;
        let n: i64 = val.trim().parse().map_err(|_| format!("bad token count in {part:?}"))?;
        target.insert(key.to_string(), n);
    }
    if target.is_empty() {
        return Err("empty marking — expected place=tokens pairs".to_string());
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsl() -> &'static str {
        r#"(schema test
            (states
                (state p1 :kind token :initial 3)
                (state p2 :kind token :initial 0)
            )
            (actions (action t1))
            (arcs (arc p1 -> t1) (arc t1 -> p2))
        )"#
    }

    #[test]
    fn verify_bounded_and_deadlock() {
        let result = run(dsl(), r#"["bounded"]"#, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["proved"], 1);
        assert_eq!(v["verdicts"][0]["status"], "proved");
    }

    #[test]
    fn verify_shorthand_reachable() {
        let result = run(dsl(), r#"["reachable:p2=3"]"#, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["verdicts"][0]["status"], "proved");
    }

    #[test]
    fn verify_conserves_via_object_form() {
        let result = run(dsl(), r#"[{"kind":"conserves","name":"total"}]"#, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["verdicts"][0]["property"], "total");
    }

    #[test]
    fn verify_rejects_empty_properties() {
        let err = run(dsl(), "[]", None).unwrap_err();
        assert!(err.contains("no properties"));
    }
}
