//! `pflow verify` — same shorthand/object property grammar as
//! `pflow-mcp`'s `petri_verify` (and petri-pilot's `pkg/mcp/verify.go`).
//! The parser is intentionally duplicated rather than shared: it is CLI/MCP
//! request-shaping glue, not library logic ground rule 4 applies to.

use crate::model_io::{load_model, write_output};
use crate::Args;
use pflow_verify::property::{Kind, Property};
use pflow_verify::Verifier;
use std::collections::HashMap;

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &[]);
    let path = args.positional.first().ok_or("model file required")?;
    let props_arg = args.positional.get(1).ok_or("properties required: a JSON array, a file path, or a single shorthand string")?;

    let model = load_model(path)?;
    let props_src = if props_arg.trim_start().starts_with('[') {
        props_arg.clone()
    } else if std::path::Path::new(props_arg).exists() {
        std::fs::read_to_string(props_arg).map_err(|e| format!("read {props_arg}: {e}"))?
    } else {
        serde_json::to_string(&[props_arg.as_str()]).unwrap()
    };
    let props = parse_properties(&props_src)?;
    if props.is_empty() {
        return Err("no properties given".to_string());
    }

    let max_states = args.get("max-states").and_then(|s| s.parse().ok());
    let mut v = Verifier::new(&model);
    if let Some(n) = max_states {
        v = v.with_max_states(n);
    }
    let report = v.check(&props);

    let verdicts: Vec<serde_json::Value> = report
        .verdicts
        .iter()
        .map(|verdict| {
            serde_json::json!({
                "property": if verdict.property.name.is_empty() { format!("{:?}", verdict.property.kind) } else { verdict.property.name.clone() },
                "status": match verdict.status {
                    pflow_verify::property::Status::Proved => "proved",
                    pflow_verify::property::Status::Refuted => "refuted",
                    pflow_verify::property::Status::Unknown => "unknown",
                },
                "method": match verdict.method {
                    pflow_verify::property::Method::Structural => "structural",
                    pflow_verify::property::Method::Exhaustive => "exhaustive",
                    pflow_verify::property::Method::Witness => "witness",
                    pflow_verify::property::Method::Partial => "partial",
                    pflow_verify::property::Method::None => "none",
                },
                "detail": verdict.detail,
                "evidence": verdict.evidence,
                "counterexample": verdict.counterexample.as_ref().map(|c| serde_json::json!({
                    "trace": c.trace, "marking": c.marking, "explanation": c.explanation,
                })),
            })
        })
        .collect();

    let out = serde_json::json!({
        "verdicts": verdicts,
        "proved": report.proved,
        "refuted": report.refuted,
        "unknown": report.unknown,
        "ok": report.ok,
        "stateCount": report.state_count,
        "truncated": report.truncated,
        "summary": report.summary(),
    });

    write_output(&out, args.get("output"))?;
    if !report.ok {
        return Err(String::new());
    }
    Ok(())
}

fn parse_properties(src: &str) -> Result<Vec<Property>, String> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(src).map_err(|e| format!("properties must be a JSON array: {e}"))?;
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
        .map(|(k, v)| v.as_i64().map(|n| (k.clone(), n)).ok_or_else(|| format!("bad token count for {k:?}")))
        .collect()
}

fn parse_shorthand(spec: &str) -> Result<Property, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("empty property".to_string());
    }
    match spec.to_lowercase().as_str() {
        "deadlock-free" | "deadlockfree" => return Ok(Property { kind: Some(Kind::DeadlockFree), name: spec.to_string(), ..Default::default() }),
        "bounded" => return Ok(Property { kind: Some(Kind::Bounded), name: spec.to_string(), ..Default::default() }),
        "live" | "liveness" => return Ok(Property { kind: Some(Kind::Live), name: spec.to_string(), ..Default::default() }),
        "terminating" | "terminates" => return Ok(Property { kind: Some(Kind::Terminating), name: spec.to_string(), ..Default::default() }),
        "conserves" | "conservation" => return Ok(Property { kind: Some(Kind::Conserves), name: spec.to_string(), ..Default::default() }),
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
            let n: i64 = body[idx + 2..].trim().parse().map_err(|_| format!("bad bound after '<=' in {body:?}"))?;
            bound = n;
            places_part = &body[..idx];
        }
        let places: Vec<String> = places_part.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
        if places.is_empty() {
            return Err("mutex requires at least one place".to_string());
        }
        return Ok(Property { kind: Some(Kind::MutualExclusion), name: spec.to_string(), places, bound, ..Default::default() });
    }
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
