//! `cid`: hosted `pflow_cid` (`mcpCID` over pflow-xyz `internal/model`'s
//! `Identify`). A net's CID is the URDNA2015 seal of the document with its
//! top-level `@id` dropped (`pflow_tokenmodel::compute_cid`, byte-exact with
//! `internal/seal`); a composition (`PetriNetBundle`) or plan (`SearchPlan`)
//! is checked structurally first (`model.Check`, refusals verbatim), and a
//! composition that is a bare wrapper around one stored net is that net.
//!
//! `compute_cid` is not a general JSON-LD processor: it knows the one
//! `https://pflow.xyz/schema` context and the shapes a pflow net uses. A
//! document outside that (another `@context`, a JSON array outside the
//! context's `@list` keys — every composition has one in `subnets` — a
//! compact-IRI key, a JSON-LD keyword the expander does not model) is
//! refused, naming the hosted server, rather than given a CID that differs
//! from pflow.xyz's.

use serde_json::{json, Map, Value};

use crate::net::net_arg;

/// The one context `compute_cid` (and `internal/seal`'s preloaded loader)
/// knows.
const CONTEXT: &str = "https://pflow.xyz/schema";

/// The `@list` containers of that context.
const LIST_KEYS: &[&str] = &["arcs", "token", "weight", "capacity", "initial", "parents"];

const TYPE_COMPOSITION: &str = "PetriNetBundle";
const TYPE_PLAN: &str = "SearchPlan";

pub(crate) fn run(args: &Value) -> Result<Value, String> {
    let doc = net_arg(args)?;
    let kind = kind(&doc);
    check(&doc).map_err(|e| format!("does not seal: {e}"))?;
    match kind {
        "composition" => {
            if let Some(part) = bare_wrapper(&doc) {
                // Hosted seals the wrapper first and fails only on a context
                // it cannot load; the answer is then the part's CID.
                match doc.get("@context") {
                    None => {}
                    Some(Value::String(c)) if c == CONTEXT => {}
                    Some(_) => {
                        return Err(refusal(
                            "a composition with a @context other than https://pflow.xyz/schema",
                        ))
                    }
                }
                return Ok(json!({"cid": part, "kind": "net", "reduced": true}));
            }
            Err(refusal(
                "a composition (PetriNetBundle) seals its subnets list as a JSON-LD set, which the local sealer does not implement",
            ))
        }
        "plan" => Ok(json!({"cid": seal(&doc)?, "kind": "plan", "reduced": false})),
        _ => Ok(json!({"cid": seal(&doc)?})),
    }
}

/// The CID `internal/seal.SealJSONLD` gives `doc` (top-level `@id` dropped),
/// or a refusal when the local sealer cannot reproduce it byte for byte.
pub(crate) fn seal(doc: &Map<String, Value>) -> Result<String, String> {
    match doc.get("@context") {
        Some(Value::String(c)) if c == CONTEXT => {}
        None => return Err(refusal("a document without @context")),
        Some(_) => {
            return Err(refusal(
                "a @context other than \"https://pflow.xyz/schema\"",
            ))
        }
    }
    let mut body = doc.clone();
    body.remove("@id");
    body.remove("@context");
    let body = sealable(&body, false).map_err(|why| refusal(&why))?;
    let mut full = body;
    full.insert("@context".into(), json!(CONTEXT));
    let raw = serde_json::to_vec(&Value::Object(full)).map_err(|e| e.to_string())?;
    pflow_tokenmodel::compute_cid(&raw)
        .map(|(cid, _)| cid)
        .map_err(|e| refusal(&format!("the local sealer cannot read it: {e}")))
}

/// The refusal for a document the local sealer cannot seal byte-identically.
fn refusal(why: &str) -> String {
    format!(
        "cannot compute this CID locally byte-identically to pflow.xyz ({why}); \
         call pflow_cid on the hosted server, https://pflow.xyz/mcp, for its CID"
    )
}

/// `obj` with its null-valued entries dropped (JSON-LD drops a null value),
/// or why `compute_cid` would expand it differently from json-gold.
fn sealable(obj: &Map<String, Value>, in_node: bool) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for (k, v) in obj {
        if v.is_null() {
            continue;
        }
        match k.as_str() {
            "@type" => match v {
                Value::String(t) if !t.is_empty() && !t.contains(':') => {}
                _ => return Err(format!("@type {v} is not a plain type name")),
            },
            "@version" => {}
            "@id" if in_node => return Err("a nested @id".into()),
            _ if k.starts_with('@') => return Err(format!("the JSON-LD keyword {k}")),
            _ if k.is_empty() || k.contains(':') => {
                return Err(format!("the key {k:?}, which JSON-LD reads as an IRI"))
            }
            _ => {}
        }
        let v = if LIST_KEYS.contains(&k.as_str()) {
            match v {
                Value::Array(items) => {
                    let mut kept = Vec::with_capacity(items.len());
                    for item in items {
                        match item {
                            Value::Array(_) => return Err(format!("a list inside the {k} list")),
                            Value::Object(m) => kept.push(Value::Object(sealable(m, true)?)),
                            other => kept.push(other.clone()),
                        }
                    }
                    Value::Array(kept)
                }
                Value::Object(m) => Value::Object(sealable(m, true)?),
                other => other.clone(),
            }
        } else {
            match v {
                Value::Array(_) => {
                    return Err(format!(
                        "the array under {k:?}, which JSON-LD reads as a set"
                    ))
                }
                Value::Object(m) => Value::Object(sealable(m, true)?),
                other => other.clone(),
            }
        };
        out.insert(k.clone(), v);
    }
    Ok(out)
}

/// `model.Kind`.
fn kind(doc: &Map<String, Value>) -> &'static str {
    match doc.get("@type").and_then(Value::as_str) {
        Some(TYPE_COMPOSITION) => "composition",
        Some(TYPE_PLAN) => "plan",
        _ => "net",
    }
}

/// pflow-xyz `model.IsCID`: `^z[1-9A-HJ-NP-Za-km-z]{20,80}$`.
fn is_cid(s: &str) -> bool {
    let rest = match s.strip_prefix('z') {
        Some(r) => r,
        None => return false,
    };
    (20..=80).contains(&rest.len())
        && rest.bytes().all(|b| matches!(b, b'1'..=b'9' | b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'Z' | b'a'..=b'k' | b'm'..=b'z'))
}

/// `model.Check`: the structural check of a composition or plan, refusals in
/// its `[CODE] message` form.
fn check(doc: &Map<String, Value>) -> Result<(), String> {
    match kind(doc) {
        "composition" => check_composition(doc),
        "plan" => check_plan(doc),
        _ => Ok(()),
    }
}

fn refuse(code: &str, msg: String) -> String {
    format!("[{code}] {msg}")
}

fn check_composition(doc: &Map<String, Value>) -> Result<(), String> {
    let subs: &[Value] = doc
        .get("subnets")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if subs.is_empty() {
        return Err(refuse(
            "E_NO_SUBNETS",
            "a composition needs at least one subnet".into(),
        ));
    }
    let mut ids: Vec<&str> = Vec::new();
    for (i, s) in subs.iter().enumerate() {
        let m = s
            .as_object()
            .ok_or_else(|| refuse("E_SUBNET", format!("subnets[{i}] is not an object")))?;
        let id = m.get("id").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() {
            return Err(refuse("E_SUBNET", format!("subnets[{i}] has no id")));
        }
        if ids.contains(&id) {
            return Err(refuse(
                "E_SUBNET",
                format!("subnet id {} appears twice", go_quote(id)),
            ));
        }
        ids.push(id);
        let q = go_quote(id);
        match (m.get("cid"), m.get("model")) {
            (Some(_), Some(_)) => {
                return Err(refuse(
                    "E_PART",
                    format!("subnet {q} has both cid and model: a part is referenced or inline, not both"),
                ))
            }
            (None, None) => {
                return Err(refuse(
                    "E_PART",
                    format!("subnet {q} has neither cid nor model: a pattern recipe alone does not pin a net"),
                ))
            }
            (Some(c), None) => {
                if !c.as_str().map(is_cid).unwrap_or(false) {
                    return Err(refuse("E_PART", format!("subnet {q}: cid {} is not a CID", go_v(c))));
                }
            }
            (None, Some(model)) => {
                if !model.is_object() {
                    return Err(refuse("E_PART", format!("subnet {q}: model is not an object")));
                }
            }
        }
        if let Some(p) = m.get("pattern") {
            if p.as_str().map(str::is_empty).unwrap_or(true) {
                return Err(refuse(
                    "E_PATTERN",
                    format!("subnet {q}: pattern must be a non-empty string"),
                ));
            }
        }
        if let Some(p) = m.get("params") {
            let Some(s) = p.as_str() else {
                return Err(refuse(
                    "E_PATTERN",
                    format!(
                        "subnet {q}: params must be a canonical-JSON string (arrays inside a sealed document hash as sets, so ordered parameters are embedded as text)"
                    ),
                ));
            };
            if !matches!(serde_json::from_str::<Value>(s), Ok(Value::Object(_))) {
                return Err(refuse(
                    "E_PATTERN",
                    format!("subnet {q}: params is not a JSON object"),
                ));
            }
        }
    }
    if let Some(Value::Array(links)) = doc.get("links") {
        for (i, l) in links.iter().enumerate() {
            let m = l
                .as_object()
                .ok_or_else(|| refuse("E_LINK", format!("links[{i}] is not an object")))?;
            for k in ["from_subnet", "from_port", "to_subnet", "to_port"] {
                if m.contains_key(k) {
                    return Err(refuse(
                        "E_BUNDLE_SHAPE",
                        format!(
                            "links[{i}] uses {}: that is go-pflow's tokenmodel/subnet bundle, not metamodel.Bundle (links are {{kind, from:{{subnet,…}}, to:{{subnet,…}}}})",
                            go_quote(k)
                        ),
                    ));
                }
            }
            if m.get("kind")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty()
            {
                return Err(refuse("E_LINK", format!("links[{i}] has no kind")));
            }
            for side in ["from", "to"] {
                let e = m.get(side).and_then(Value::as_object).ok_or_else(|| {
                    refuse(
                        "E_LINK",
                        format!("links[{i}].{side} is not an endpoint object"),
                    )
                })?;
                let sn = e.get("subnet").and_then(Value::as_str).unwrap_or("");
                if !ids.contains(&sn) {
                    return Err(refuse(
                        "E_LINK",
                        format!(
                            "links[{i}].{side} names subnet {}, which is not a part",
                            go_quote(sn)
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn check_plan(doc: &Map<String, Value>) -> Result<(), String> {
    if !doc
        .get("model_cid")
        .and_then(Value::as_str)
        .map(is_cid)
        .unwrap_or(false)
    {
        return Err(refuse(
            "E_PLAN",
            "model_cid must be the CID of the model the plan is for".into(),
        ));
    }
    let Some(s) = doc.get("plan").and_then(Value::as_str) else {
        return Err(refuse(
            "E_PLAN",
            "plan must be the plan's canonical JSON as a string (its lists are ordered; a nested array would hash as a set)"
                .into(),
        ));
    };
    if !matches!(serde_json::from_str::<Value>(s), Ok(Value::Object(_))) {
        return Err(refuse("E_PLAN", "plan is not a JSON object".into()));
    }
    Ok(())
}

/// `model.BareWrapper`: the part's CID when the composition is nothing but
/// one net by reference.
fn bare_wrapper(doc: &Map<String, Value>) -> Option<String> {
    const TOP: &[&str] = &[
        "@context",
        "@id",
        "@type",
        "subnets",
        "namespace",
        "arc_merge",
        "links",
        "constraints",
    ];
    const SUB: &[&str] = &["@type", "id", "net_type", "cid"];
    for (k, v) in doc {
        if !TOP.contains(&k.as_str()) {
            return None;
        }
        if (k == "links" || k == "constraints") && !matches!(v, Value::Array(a) if a.is_empty()) {
            return None;
        }
    }
    let subs = doc.get("subnets")?.as_array()?;
    if subs.len() != 1 {
        return None;
    }
    let m = subs[0].as_object()?;
    if m.keys().any(|k| !SUB.contains(&k.as_str())) {
        return None;
    }
    let cid = m.get("cid")?.as_str()?;
    is_cid(cid).then(|| cid.to_string())
}

/// Go's `%q` of a string.
fn go_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Go's `%v` of a decoded JSON value, for the scalars a `cid` field holds.
fn go_v(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "<nil>".into(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i64),
            Some(f) => format!("{f}"),
            None => n.to_string(),
        },
        other => other.to_string(),
    }
}
