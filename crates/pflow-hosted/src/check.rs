//! `check`: hosted `pflow_check` (`mcpCheck`). The server's JSON-LD sanity
//! check (`validateJSONLD`: an `@context`, nesting at most 50 deep, no control
//! characters in keys) plus the size go-pflow's reader sees. go-pflow's
//! `parser.FromJSON` refuses only a non-object, which `net` can never be here,
//! so `parse` is never set, as on the hosted server.

use serde_json::{json, Map, Value};

use crate::net::{net_arg, petri_net};

pub(crate) fn run(args: &Value) -> Result<Value, String> {
    let doc = net_arg(args)?;
    let mut out = Map::new();
    out.insert("ok".into(), json!(true));
    if let Err(e) = validate_jsonld(&doc) {
        out.insert("ok".into(), json!(false));
        out.insert("jsonld".into(), json!(e));
    }
    let net = petri_net(&doc);
    out.insert("places".into(), json!(net.places.len()));
    out.insert("transitions".into(), json!(net.transitions.len()));
    out.insert("arcs".into(), json!(net.arcs.len()));
    Ok(Value::Object(out))
}

/// pflow-xyz `validateJSONLD`, messages verbatim.
pub(crate) fn validate_jsonld(doc: &Map<String, Value>) -> Result<(), String> {
    match doc.get("@context") {
        None => return Err("missing @context field".into()),
        Some(Value::String(_) | Value::Object(_) | Value::Array(_)) => {}
        Some(_) => return Err("@context must be a string, object, or array".into()),
    }
    let root = Value::Object(doc.clone());
    if too_deep(&root, 0, 50) {
        return Err("document exceeds maximum nesting depth of 50".into());
    }
    if bad_key(&root) {
        return Err("keys cannot contain control characters".into());
    }
    Ok(())
}

fn too_deep(v: &Value, current: usize, max: usize) -> bool {
    if current > max {
        return true;
    }
    match v {
        Value::Object(m) => m.values().any(|x| too_deep(x, current + 1, max)),
        Value::Array(a) => a.iter().any(|x| too_deep(x, current + 1, max)),
        _ => false,
    }
}

fn bad_key(v: &Value) -> bool {
    match v {
        Value::Object(m) => m
            .iter()
            .any(|(k, x)| k.chars().any(|c| (c as u32) < 32 || c as u32 == 127) || bad_key(x)),
        Value::Array(a) => a.iter().any(bad_key),
        _ => false,
    }
}
