//! `edit` (hosted `pflow_edit`): build or edit a net in the editor's JSON-LD
//! document, op by op, all or nothing.
//!
//! A port of pflow-xyz `cmd/webserver/mcp_registry.go` (`mcpEdit`, `applyOp`
//! and its helpers), which mirror the editor's `public/petri-commands.js`, so
//! an edit made here, on pflow.xyz/mcp and in the editor agree. The document is
//! edited as JSON; nothing is stored.
//!
//! Numbers: Go decodes every JSON number to `float64` and writes a whole one
//! without a fraction (`100`, not `100.0`); [`num`] does the same so the
//! returned document reads as the hosted one does.

use serde_json::{json, Map, Value};

/// At most this many ops per call, as hosted.
pub const MAX_EDIT_OPS: usize = 500;

const OPS: &str = "add_place, add_transition, add_arc, set_arc, delete_arc, set_place, add_tokens, set_rate, set_label, rename, move, delete, set_details";

/// A JSON number as Go writes a `float64`: whole values without a fraction.
pub fn num(f: f64) -> Value {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
        json!(f as i64)
    } else {
        json!(f)
    }
}

/// Go's `%g` for the numbers these messages print.
fn g(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e21 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

/// Go's `%v` for a decoded JSON value.
fn v(x: &Value) -> String {
    match x {
        Value::Null => "<nil>".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map(g).unwrap_or_else(|| n.to_string()),
        other => other.to_string(),
    }
}

fn empty_net() -> Map<String, Value> {
    let Value::Object(m) = json!({
        "@context": "https://pflow.xyz/schema", "@type": "PetriNet", "@version": "1.1",
        "token": ["https://pflow.xyz/tokens/black"], "places": {}, "transitions": {}, "arcs": [],
    }) else {
        unreachable!()
    };
    m
}

/// The edit tool. `args` is `{net?, ops}`.
pub fn edit(args: &Value) -> Result<Value, String> {
    let mut doc = match args.get("net") {
        None | Some(Value::Null) => empty_net(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => return Err("net: a JSON object (the editor's format)".into()),
    };
    for (k, def) in [
        ("places", json!({})),
        ("transitions", json!({})),
        ("arcs", json!([])),
    ] {
        if doc.get(k).is_none_or(Value::is_null) {
            doc.insert(k.into(), def);
        }
    }
    if !doc["places"].is_object() {
        return Err("net.places: an object keyed by place id".into());
    }
    if !doc["transitions"].is_object() {
        return Err("net.transitions: an object keyed by transition id".into());
    }
    if !doc["arcs"].is_array() {
        return Err("net.arcs: a list".into());
    }
    for (k, def) in [
        ("@context", json!("https://pflow.xyz/schema")),
        ("@type", json!("PetriNet")),
        ("@version", json!("1.1")),
        ("token", json!(["https://pflow.xyz/tokens/black"])),
    ] {
        if doc.get(k).is_none_or(Value::is_null) {
            doc.insert(k.into(), def);
        }
    }
    let Some(ops) = args.get("ops").and_then(Value::as_array) else {
        return Err("ops: a list of {op, …}".into());
    };
    if ops.len() > MAX_EDIT_OPS {
        return Err(format!("ops: at most {MAX_EDIT_OPS} per call"));
    }
    let mut results = Vec::with_capacity(ops.len());
    for (i, o) in ops.iter().enumerate() {
        let Some(op) = o.as_object() else {
            return Err(format!("ops[{i}]: an object {{op, …}}"));
        };
        let res = apply_op(&mut doc, op).map_err(|e| {
            format!(
                "ops[{i}] ({}): {e}",
                v(op.get("op").unwrap_or(&Value::Null))
            )
        })?;
        results.push(res);
    }
    let summary = json!({
        "places": doc["places"].as_object().map_or(0, Map::len),
        "transitions": doc["transitions"].as_object().map_or(0, Map::len),
        "arcs": doc["arcs"].as_array().map_or(0, Vec::len),
    });
    let net = Value::Object(doc);
    check_jsonld(&net).map_err(|e| format!("the edited net is not valid JSON-LD: {e}"))?;
    let mut out =
        json!({ "net": net, "applied": ops.len(), "results": results, "summary": summary });
    let raw = serde_json::to_vec(&out["net"]).map_err(|e| e.to_string())?;
    // `cid: false` skips the seal, which is about cubic in net size (bug B5).
    if args.get("cid") != Some(&Value::Bool(false)) {
        if let Ok((cid, _)) = pflow_tokenmodel::canonical_cid::compute_cid(&raw) {
            out["cid"] = json!(cid);
        }
    }
    if let Err(e) = pflow_parser::model_from_json(&raw) {
        out["warning"] = json!(format!("pflow-rs cannot read the result yet: {e}"));
    }
    Ok(out)
}

/// pflow-xyz `validateJSONLD`: an `@context` of the right type, nesting at
/// most 50 deep, no control characters in keys.
pub fn check_jsonld(doc: &Value) -> Result<(), String> {
    match doc.get("@context") {
        None => return Err("missing @context field".into()),
        Some(Value::String(_) | Value::Object(_) | Value::Array(_)) => {}
        Some(_) => return Err("@context must be a string, object, or array".into()),
    }
    fn depth(x: &Value, cur: usize) -> Result<(), String> {
        if cur > 50 {
            return Err("document exceeds maximum nesting depth of 50".into());
        }
        match x {
            Value::Object(m) => m.values().try_for_each(|e| depth(e, cur + 1)),
            Value::Array(a) => a.iter().try_for_each(|e| depth(e, cur + 1)),
            _ => Ok(()),
        }
    }
    fn keys(x: &Value) -> Result<(), String> {
        match x {
            Value::Object(m) => {
                for (k, e) in m {
                    if k.chars().any(|c| (c as u32) < 32 || c as u32 == 127) {
                        return Err("keys cannot contain control characters".into());
                    }
                    keys(e)?;
                }
                Ok(())
            }
            Value::Array(a) => a.iter().try_for_each(keys),
            _ => Ok(()),
        }
    }
    depth(doc, 0)?;
    keys(doc)
}

fn color_count(doc: &Map<String, Value>) -> usize {
    match doc.get("token").and_then(Value::as_array) {
        Some(t) if !t.is_empty() => t.len(),
        _ => 1,
    }
}

fn node_kind(doc: &Map<String, Value>, id: &str) -> Option<&'static str> {
    if doc["places"]
        .as_object()
        .is_some_and(|m| m.contains_key(id))
    {
        Some("place")
    } else if doc["transitions"]
        .as_object()
        .is_some_and(|m| m.contains_key(id))
    {
        Some("transition")
    } else {
        None
    }
}

/// The node `id` names, checked to be of kind `want` (any kind when `None`).
fn need_node<'a>(
    doc: &'a mut Map<String, Value>,
    id: Option<&Value>,
    want: Option<&str>,
) -> Result<&'a mut Map<String, Value>, String> {
    let s = id.and_then(Value::as_str).unwrap_or("");
    let Some(k) = node_kind(doc, s) else {
        return Err(format!("{s:?} is not a place or transition"));
    };
    if let Some(w) = want {
        if k != w {
            return Err(format!("{s:?} is a {k}, not a {w}"));
        }
    }
    let coll = if k == "place" {
        "places"
    } else {
        "transitions"
    };
    doc.get_mut(coll)
        .and_then(Value::as_object_mut)
        .and_then(|m| m.get_mut(s))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{s:?} is not an object"))
}

fn new_id(doc: &Map<String, Value>, x: Option<&Value>, prefix: &str) -> Result<String, String> {
    let x = match x {
        None | Some(Value::Null) => {
            return Ok((0..)
                .map(|i| format!("{prefix}{i}"))
                .find(|id| node_kind(doc, id).is_none())
                .expect("an unused id"))
        }
        Some(x) => x,
    };
    let id = match x.as_str() {
        Some(s) if !s.trim().is_empty() && !s.contains([' ', '\t', '\r', '\n']) => s,
        _ => {
            return Err(format!(
                "{} is not a usable id (non-empty, no spaces)",
                v(x)
            ))
        }
    };
    if id.chars().count() > 100 {
        return Err("an id is at most 100 characters".into());
    }
    if matches!(id, "__proto__" | "constructor" | "prototype") {
        return Err(format!("{id:?} cannot be an id"));
    }
    if node_kind(doc, id).is_some() {
        return Err(format!("{id:?} is already used"));
    }
    Ok(id.to_string())
}

fn check_label(x: Option<&Value>) -> Result<String, String> {
    let s = match x.and_then(Value::as_str) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Err("label cannot be empty".into()),
    };
    if s.chars().count() > 100 {
        return Err("label must be 100 characters or fewer".into());
    }
    if s.contains(['\r', '\n']) {
        return Err("label must be a single line".into());
    }
    Ok(s.to_string())
}

/// A count or list of counts as one value per colour, padded with `fill`.
fn vec_of(x: &Value, colors: usize, fill: f64) -> Result<Vec<Value>, String> {
    let mut out = match x {
        Value::Number(_) => vec![x.clone()],
        Value::Null => vec![Value::Null],
        Value::Array(a) => a.clone(),
        _ => return Err(format!("{}: a count or a list of counts", v(x))),
    };
    for (i, e) in out.iter().enumerate() {
        match e {
            Value::Null => {}
            Value::Number(n) => {
                let f = n.as_f64().unwrap_or(f64::NAN);
                if f < 0.0 || f != f.trunc() || f.is_infinite() {
                    return Err(format!("[{i}]: a whole number ≥ 0"));
                }
            }
            _ => return Err(format!("[{i}]: a whole number ≥ 0")),
        }
    }
    while out.len() < colors {
        out.push(num(fill));
    }
    Ok(out)
}

/// A capacity or initial vector: null slots become 0 (for capacity, unbounded).
fn counts_of(x: &Value, colors: usize) -> Result<Vec<Value>, String> {
    let mut c = vec_of(x, colors, 0.0)?;
    for e in &mut c {
        if e.is_null() {
            *e = json!(0);
        }
    }
    Ok(c)
}

fn within_capacity(id: &str, p: &Map<String, Value>) -> Result<(), String> {
    let (Some(ini), Some(cap)) = (
        p.get("initial").and_then(Value::as_array),
        p.get("capacity").and_then(Value::as_array),
    ) else {
        return Ok(());
    };
    for (n, c) in ini.iter().zip(cap) {
        let (n, c) = (n.as_f64().unwrap_or(0.0), c.as_f64().unwrap_or(0.0));
        if c > 0.0 && n > c {
            return Err(format!(
                "places.{id}: {} tokens exceed capacity {}",
                g(n),
                g(c)
            ));
        }
    }
    Ok(())
}

/// An op's number field: `def` when absent or null.
fn num_field(op: &Map<String, Value>, k: &str, def: f64) -> Result<f64, String> {
    match op.get(k) {
        None | Some(Value::Null) => Ok(def),
        Some(Value::Number(n)) => n
            .as_f64()
            .filter(|f| f.is_finite())
            .ok_or(format!("{k}: a number")),
        Some(_) => Err(format!("{k}: a number")),
    }
}

fn string_list(x: Option<&Value>, name: &str) -> Result<Vec<String>, String> {
    let err = || format!("{name}: a list of strings");
    x.and_then(Value::as_array)
        .ok_or_else(err)?
        .iter()
        .map(|e| e.as_str().map(str::to_string).ok_or_else(err))
        .collect()
}

fn arcs_mut(doc: &mut Map<String, Value>) -> &mut Vec<Value> {
    doc.get_mut("arcs")
        .and_then(Value::as_array_mut)
        .expect("arcs checked to be a list")
}

fn arc_is(a: &Value, src: &str, dst: &str) -> bool {
    a.get("source").and_then(Value::as_str) == Some(src)
        && a.get("target").and_then(Value::as_str) == Some(dst)
}

fn apply_op(doc: &mut Map<String, Value>, op: &Map<String, Value>) -> Result<Value, String> {
    let name = op.get("op").and_then(Value::as_str).unwrap_or("");
    let colors = color_count(doc);
    let count = |doc: &Map<String, Value>, k: &str| doc[k].as_object().map_or(0, Map::len) as f64;
    match name {
        "add_place" => {
            let id = new_id(doc, op.get("id"), "p")?;
            let n = count(doc, "places");
            let x = num_field(op, "x", 100.0 + 150.0 * (n % 6.0))?;
            let y = num_field(op, "y", 100.0 + 150.0 * (n / 6.0).floor() * 2.0)?;
            let ini = counts_of(op.get("initial").unwrap_or(&Value::Null), colors)
                .map_err(|e| format!("initial{e}"))?;
            let cap = counts_of(op.get("capacity").unwrap_or(&Value::Null), colors)
                .map_err(|e| format!("capacity{e}"))?;
            let mut p = Map::new();
            p.insert("@type".into(), json!("Place"));
            p.insert("x".into(), num(x.round()));
            p.insert("y".into(), num(y.round()));
            p.insert("offset".into(), json!(0));
            p.insert("initial".into(), Value::Array(ini));
            p.insert("capacity".into(), Value::Array(cap));
            if op.contains_key("label") {
                p.insert("label".into(), json!(check_label(op.get("label"))?));
            }
            within_capacity(&id, &p)?;
            doc["places"]
                .as_object_mut()
                .unwrap()
                .insert(id.clone(), Value::Object(p));
            Ok(json!({ "id": id }))
        }
        "add_transition" => {
            let id = new_id(doc, op.get("id"), "t")?;
            let n = count(doc, "transitions");
            let x = num_field(op, "x", 100.0 + 150.0 * (n % 6.0))?;
            let y = num_field(op, "y", 100.0 + 150.0 * (n / 6.0).floor() * 2.0 + 150.0)?;
            let mut t = Map::new();
            t.insert("@type".into(), json!("Transition"));
            t.insert("x".into(), num(x.round()));
            t.insert("y".into(), num(y.round()));
            if op.contains_key("label") {
                t.insert("label".into(), json!(check_label(op.get("label"))?));
            }
            match op.get("rate") {
                None | Some(Value::Null) => {}
                Some(r) => {
                    let f = r
                        .as_f64()
                        .filter(|f| *f >= 0.0 && f.is_finite())
                        .ok_or("rate: a number ≥ 0")?;
                    if f != 1.0 {
                        t.insert("rate".into(), r.clone());
                    }
                }
            }
            doc["transitions"]
                .as_object_mut()
                .unwrap()
                .insert(id.clone(), Value::Object(t));
            Ok(json!({ "id": id }))
        }
        "add_arc" | "set_arc" => {
            let src = op
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let dst = op
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if name == "add_arc" {
                let (sk, tk) = (node_kind(doc, &src), node_kind(doc, &dst));
                need_node(doc, op.get("source"), None)?;
                need_node(doc, op.get("target"), None)?;
                if sk == tk {
                    return Err(format!(
                        "an arc joins a place and a transition; {src} and {dst} are both {}s",
                        sk.unwrap_or("")
                    ));
                }
                let inhibit = op.get("inhibit").and_then(Value::as_bool).unwrap_or(false);
                let dup = arcs_mut(doc).iter().any(|a| {
                    arc_is(a, &src, &dst)
                        && (a.get("inhibitTransition") == Some(&json!(true))) == inhibit
                });
                if dup {
                    return Err(format!(
                        "there is already an arc {src} → {dst}; use set_arc"
                    ));
                }
                let w = match op.get("weight") {
                    None | Some(Value::Null) => json!(1),
                    Some(w) => w.clone(),
                };
                let w = vec_of(&w, colors, 0.0).map_err(|e| format!("weight{e}"))?;
                arcs_mut(doc).push(json!({
                    "@type": "Arrow", "source": src, "target": dst, "weight": w, "inhibitTransition": inhibit,
                }));
                return Ok(json!({}));
            }
            let Some(a) = arcs_mut(doc).iter_mut().find(|a| arc_is(a, &src, &dst)) else {
                return Err(format!("no arc {src} → {dst}"));
            };
            if let Some(w) = op.get("weight") {
                a["weight"] =
                    Value::Array(vec_of(w, colors, 0.0).map_err(|e| format!("weight{e}"))?);
            }
            if let Some(inh) = op.get("inhibit").and_then(Value::as_bool) {
                a["inhibitTransition"] = json!(inh);
            }
            Ok(json!({}))
        }
        "delete_arc" => {
            let src = op
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let dst = op
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let arcs = arcs_mut(doc);
            let before = arcs.len();
            arcs.retain(|a| !arc_is(a, &src, &dst));
            if arcs.len() == before {
                return Err(format!("no arc {src} → {dst}"));
            }
            Ok(json!({}))
        }
        "set_place" => {
            let id = op
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let p = need_node(doc, op.get("id"), Some("place"))?;
            if let Some(x) = op.get("initial") {
                p.insert(
                    "initial".into(),
                    Value::Array(counts_of(x, colors).map_err(|e| format!("initial{e}"))?),
                );
            }
            if let Some(x) = op.get("capacity") {
                p.insert(
                    "capacity".into(),
                    Value::Array(counts_of(x, colors).map_err(|e| format!("capacity{e}"))?),
                );
            }
            within_capacity(&id, p)?;
            Ok(json!({}))
        }
        "set_label" => {
            let label = op.get("label");
            let n = need_node(doc, op.get("id"), None)?;
            n.insert("label".into(), json!(check_label(label)?));
            Ok(json!({}))
        }
        "add_tokens" => {
            let id = op
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            need_node(doc, op.get("id"), Some("place"))?;
            let delta = num_field(op, "delta", 1.0)?;
            let ci = num_field(op, "color", 0.0)?;
            let p = need_node(doc, op.get("id"), Some("place"))?;
            let mut ini = p
                .get("initial")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let i = ci as i64;
            if ci < 0.0 || ci != ci.trunc() || (i as usize >= colors && i as usize >= ini.len()) {
                return Err(format!("color {i}: the net has {colors} colour(s)"));
            }
            let i = i as usize;
            while ini.len() <= i {
                ini.push(json!(0));
            }
            let cur = ini[i].as_f64().unwrap_or(0.0);
            if cur + delta < 0.0 {
                return Err(format!(
                    "{id} has {} token(s); cannot remove {}",
                    g(cur),
                    g(-delta)
                ));
            }
            ini[i] = num(cur + delta);
            p.insert("initial".into(), Value::Array(ini.clone()));
            within_capacity(&id, p)?;
            Ok(json!({ "initial": ini }))
        }
        "set_rate" => {
            let rate = op.get("rate").cloned();
            let t = need_node(doc, op.get("id"), Some("transition"))?;
            let ok = rate
                .as_ref()
                .and_then(Value::as_f64)
                .is_some_and(|r| r >= 0.0 && r.is_finite());
            if !ok {
                return Err("rate: a number ≥ 0".into());
            }
            t.insert("rate".into(), rate.unwrap());
            Ok(json!({}))
        }
        "move" => {
            let x = num_field(op, "x", f64::NAN);
            let y = num_field(op, "y", f64::NAN);
            let n = need_node(doc, op.get("id"), None)?;
            match (x, y) {
                (Ok(x), Ok(y)) if !x.is_nan() && !y.is_nan() => {
                    n.insert("x".into(), num(x.round()));
                    n.insert("y".into(), num(y.round()));
                    Ok(json!({}))
                }
                _ => Err("x and y: numbers".into()),
            }
        }
        "rename" => {
            let old = op
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let Some(k) = node_kind(doc, &old) else {
                return Err(format!("{old:?} is not a place or transition"));
            };
            let nid = match op.get("new_id") {
                None | Some(Value::Null) => {
                    // As hosted: newID(""), whose refusal wins over "required".
                    return Err(new_id(doc, Some(&json!("")), "")
                        .err()
                        .unwrap_or("new_id: required".into()));
                }
                Some(x) => new_id(doc, Some(x), "")?,
            };
            let coll = if k == "place" {
                "places"
            } else {
                "transitions"
            };
            let m = doc[coll].as_object_mut().unwrap();
            let node = m.remove(&old).unwrap();
            m.insert(nid.clone(), node);
            for a in arcs_mut(doc) {
                for end in ["source", "target"] {
                    if a.get(end).and_then(Value::as_str) == Some(old.as_str()) {
                        a[end] = json!(nid);
                    }
                }
            }
            Ok(json!({ "id": nid }))
        }
        "delete" => {
            let ids = string_list(op.get("ids"), "ids")?;
            for id in &ids {
                if node_kind(doc, id).is_none() {
                    return Err(format!("{id:?} is not a place or transition"));
                }
            }
            for id in &ids {
                doc["places"].as_object_mut().unwrap().remove(id);
                doc["transitions"].as_object_mut().unwrap().remove(id);
            }
            let gone = |x: Option<&Value>| x.map(v).is_some_and(|s| ids.contains(&s));
            arcs_mut(doc).retain(|a| !gone(a.get("source")) && !gone(a.get("target")));
            Ok(json!({ "deleted": ids }))
        }
        "set_details" => {
            for k in ["name", "description"] {
                let Some(x) = op.get(k) else { continue };
                let s = x.as_str().ok_or(format!("{k}: a string"))?.trim();
                if s.is_empty() {
                    doc.remove(k);
                } else {
                    doc.insert(k.into(), json!(s));
                }
            }
            Ok(json!({}))
        }
        _ => Err(format!("unknown op {name:?} ({OPS})")),
    }
}
