//! Reading a tool's arguments and the bundle it carries, and the limits a
//! bundle is held to before any engine sees it (pflow-xyz
//! `cmd/webserver/mcp_model_read.go`, `checkArgs` in
//! `mcp_workbench_engine.go`).
//!
//! A bundle arrives as JSON: this refuses what the hosted server's JSON-RPC
//! layer could not carry exactly (a count of 2^53 or more), a name for a
//! kind, net type or merge policy that is not one, a name past
//! [`MAX_ID_BYTES`] and a bundle past the counts in [`super`], with the
//! hosted tools' words. The document is then read by pflow-compose's own
//! serde, as pflow-native reads it; what Go reads as a zero value (a missing
//! name or list, a `null`) is filled in first, so a document the hosted tool
//! reads is read here too.

use pflow_compose::{Bundle, Port, PortKind, Subnet};
use serde_json::{Map, Value};

use super::text::{integer, json, quote, quote_list};
use super::*;

/// A subnet, place, transition, port, link or constraint id, an arc end, a
/// schema: at most this many bytes.
pub const MAX_ID_BYTES: usize = 128;

/// JSON-RPC numbers are doubles: 2^53 and up cannot be told from their
/// neighbours.
pub const EXACT_INT: f64 = 9007199254740992.0;

/// What an argument must be (`argKind`).
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Any,
    Str,
    Bool,
    StrList,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Any => "anything",
            Kind::Str => "a string",
            Kind::Bool => "a boolean",
            Kind::StrList => "a list of strings",
        }
    }
}

/// The arguments as an object (a call's arguments always are one).
pub fn object<'a>(tool: &str, args: &'a Value) -> Result<&'a Map<String, Value>, String> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    match args {
        Value::Object(m) => Ok(m),
        Value::Null => Ok(EMPTY.get_or_init(Map::new)),
        other => Err(format!(
            "{tool}: the arguments should be an object, got {}",
            json(other)
        )),
    }
}

/// `checkArgs`: every argument is one the tool takes, of the kind it takes
/// (a `null` is as good as absent).
pub fn check_args(
    tool: &str,
    a: &Map<String, Value>,
    kinds: &[(&str, Kind)],
) -> Result<(), String> {
    let mut keys: Vec<&String> = a.keys().collect();
    keys.sort();
    for k in keys {
        let Some((_, kind)) = kinds.iter().find(|(n, _)| n == k) else {
            let mut names: Vec<&str> = kinds.iter().map(|(n, _)| *n).collect();
            names.sort();
            return Err(format!(
                "{tool}: unknown argument {}; takes {}",
                quote(k),
                quote_list(&names)
            ));
        };
        let v = &a[k];
        if v.is_null() || *kind == Kind::Any {
            continue;
        }
        let good = match kind {
            Kind::Str => v.is_string(),
            Kind::Bool => v.is_boolean(),
            Kind::StrList => v.as_array().is_some_and(|l| l.iter().all(Value::is_string)),
            Kind::Any => true,
        };
        if !good {
            return Err(format!(
                "{tool}: argument {} should be {}, got {}",
                quote(k),
                kind.name(),
                json(v)
            ));
        }
    }
    Ok(())
}

/// Checks the arguments and reads the bundle (`modelBundle`), refusing one
/// past the limits.
pub fn bundle_arg(
    tool: &str,
    a: &Map<String, Value>,
    kinds: &[(&str, Kind)],
) -> Result<Bundle, String> {
    check_args(tool, a, kinds)?;
    let Some(raw) = a.get("bundle").and_then(Value::as_object) else {
        return Err(format!("{tool}: bundle: a PetriNetBundle (a JSON object with subnets and links) is required; pflow_bundle_example returns one"));
    };
    if let Some(t) = raw.get("@type").and_then(Value::as_str) {
        if !t.is_empty() && t != BUNDLE_TYPE {
            return Err(format!(
                "{tool}: bundle: @type is {}, not {}",
                quote(t),
                quote(BUNDLE_TYPE)
            ));
        }
    }
    let subnets = match raw.get("subnets").and_then(Value::as_array) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Err(format!(
                "{tool}: bundle: needs a non-empty \"subnets\" array"
            ))
        }
    };
    if subnets.len() > MAX_SUBNETS {
        return Err(format!(
            "{tool}: bundle: {} subnets; at most {MAX_SUBNETS}",
            subnets.len()
        ));
    }
    if let Some(links) = raw.get("links").and_then(Value::as_array) {
        if links.len() > MAX_LINKS {
            return Err(format!(
                "{tool}: bundle: {} links; at most {MAX_LINKS}",
                links.len()
            ));
        }
    }
    check_numbers(tool, raw)?;
    check_enums(tool, raw)?;
    let mut doc = Value::Object(raw.clone());
    go_zero_values(tool, &mut doc)?;
    let b: Bundle = serde_json::from_value(doc).map_err(|e| format!("{tool}: bundle: {e}"))?;
    check_size(tool, &b)?;
    Ok(b)
}

// ------------------------------------------------------------ numbers

/// Refuses a number of 2^53 or more anywhere in the bundle.
fn check_numbers(tool: &str, raw: &Map<String, Value>) -> Result<(), String> {
    fn walk(tool: &str, v: &Value, path: &mut Vec<String>) -> Result<(), String> {
        match v {
            Value::Number(n) => {
                let x = n.as_f64().unwrap_or(0.0);
                if x.abs() >= EXACT_INT {
                    let p = path.concat();
                    return Err(format!(
                        "{tool}: bundle: {} is {}; a count or weight is at most 9007199254740991 (2^53 - 1), the largest integer JSON-RPC carries exactly (a bigger one would arrive rounded)",
                        p.strip_prefix('.').unwrap_or(&p),
                        integer(x)
                    ));
                }
            }
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                for k in keys {
                    path.push(format!(".{k}"));
                    walk(tool, &m[k], path)?;
                    path.pop();
                }
            }
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    path.push(format!("[{i}]"));
                    walk(tool, x, path)?;
                    path.pop();
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut keys: Vec<&String> = raw.keys().collect();
    keys.sort();
    let mut path = vec![];
    for k in keys {
        path.push(format!(".{k}"));
        walk(tool, &raw[k], &mut path)?;
        path.pop();
    }
    Ok(())
}

// ------------------------------------------------------------ names for kinds

/// A field that names a kind: a value outside `allowed` is refused.
fn check_enum(
    tool: &str,
    path: &str,
    m: &Map<String, Value>,
    key: &str,
    allowed: &[&str],
) -> Result<(), String> {
    let Some(v) = m.get(key) else { return Ok(()) };
    if v.is_null() || v.as_str().is_some_and(|s| allowed.contains(&s)) {
        return Ok(());
    }
    let quoted: Vec<String> = allowed.iter().map(|a| quote(a)).collect();
    Err(format!(
        "{tool}: bundle: {path}.{key} is {}; it is one of {}",
        json(v),
        quoted.join(", ")
    ))
}

fn objects(v: Option<&Value>) -> Vec<Option<&Map<String, Value>>> {
    v.and_then(Value::as_array)
        .map(|l| l.iter().map(Value::as_object).collect())
        .unwrap_or_default()
}

/// Refuses a misspelt arc_merge, net_type, place kind, port kind or target,
/// or link kind. (An arc type is Validate's to judge: E_UNKNOWN_ARC_TYPE.)
fn check_enums(tool: &str, raw: &Map<String, Value>) -> Result<(), String> {
    check_enum(tool, "bundle", raw, "arc_merge", &["sum", "max"])?;
    for (i, s) in objects(raw.get("subnets")).into_iter().enumerate() {
        let Some(s) = s else { continue };
        let path = format!("subnets[{i}]");
        check_enum(
            tool,
            &path,
            s,
            "net_type",
            &[
                "",
                "WorkflowNet",
                "ResourceNet",
                "GameNet",
                "ComputationNet",
                "ClassificationNet",
            ],
        )?;
        let model = s.get("model").and_then(Value::as_object);
        for (j, p) in objects(model.and_then(|m| m.get("places")))
            .into_iter()
            .enumerate()
        {
            if let Some(p) = p {
                check_enum(
                    tool,
                    &format!("{path}.model.places[{j}]"),
                    p,
                    "kind",
                    &["token", "data"],
                )?;
            }
        }
        for (j, p) in objects(s.get("ports")).into_iter().enumerate() {
            if let Some(p) = p {
                let pp = format!("{path}.ports[{j}]");
                check_enum(tool, &pp, p, "kind", &["in", "out", "inout", "observe"])?;
                check_enum(tool, &pp, p, "target", &["place", "transition"])?;
            }
        }
    }
    for (i, l) in objects(raw.get("links")).into_iter().enumerate() {
        if let Some(l) = l {
            check_enum(
                tool,
                &format!("links[{i}]"),
                l,
                "kind",
                &["token", "data", "event", "guard"],
            )?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------ Go's zero values

/// Fills in what Go's decoder reads as a zero value and pflow-compose's serde
/// requires: a `null` member is dropped (Go leaves the field as it was), a
/// missing name or list is set to "" or [], and an integer that arrived as
/// a double (`2.0`) is the integer. A subnet with no model and a link with no
/// kind are refused: pflow-compose has no representation for either.
fn go_zero_values(tool: &str, doc: &mut Value) -> Result<(), String> {
    drop_nulls(doc);
    let Value::Object(b) = doc else { return Ok(()) };
    for (i, s) in b
        .get_mut("subnets")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Value::Object(s) = s else { continue };
        fill(s, &[("id", Value::from(""))]);
        let Some(Value::Object(m)) = s.get_mut("model") else {
            return Err(format!("{tool}: bundle: subnets[{i}] has no model"));
        };
        fill(
            m,
            &[
                ("name", Value::from("")),
                ("places", Value::Array(vec![])),
                ("transitions", Value::Array(vec![])),
                ("arcs", Value::Array(vec![])),
            ],
        );
        for (list, keys) in [
            ("places", &["id"][..]),
            ("transitions", &["id"][..]),
            ("arcs", &["from", "to"][..]),
            ("constraints", &["id", "expr"][..]),
        ] {
            fill_each(m.get_mut(list), keys);
        }
        fill_each(s.get_mut("ports"), &["id"]);
    }
    fill_each(b.get_mut("constraints"), &["id", "expr"]);
    for (i, l) in b
        .get_mut("links")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Value::Object(l) = l else { continue };
        if !l.contains_key("kind") {
            return Err(format!("{tool}: bundle: links[{i}] has no kind"));
        }
        for end in ["from", "to"] {
            let e = l.entry(end).or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(e) = e {
                fill(e, &[("subnet", Value::from(""))]);
            }
        }
    }
    Ok(())
}

fn drop_nulls(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.retain(|_, x| !x.is_null());
            m.values_mut().for_each(drop_nulls);
        }
        Value::Array(a) => a.iter_mut().for_each(drop_nulls),
        Value::Number(n) if n.is_f64() => {
            let f = n.as_f64().unwrap_or(0.0);
            if f.fract() == 0.0 && f.abs() < EXACT_INT {
                *v = Value::from(f as i64);
            }
        }
        _ => {}
    }
}

fn fill(m: &mut Map<String, Value>, defaults: &[(&str, Value)]) {
    for (k, d) in defaults {
        m.entry(*k).or_insert_with(|| d.clone());
    }
}

fn fill_each(list: Option<&mut Value>, keys: &[&str]) {
    for x in list.and_then(Value::as_array_mut).into_iter().flatten() {
        if let Value::Object(x) = x {
            for k in keys {
                x.entry(*k).or_insert_with(|| Value::from(""));
            }
        }
    }
}

// ------------------------------------------------------------ size

/// Refuses a bundle past the counts and a name past [`MAX_ID_BYTES`].
fn check_size(tool: &str, b: &Bundle) -> Result<(), String> {
    let long = |path: String, id: &str| -> Result<(), String> {
        if id.len() > MAX_ID_BYTES {
            return Err(format!(
                "{tool}: bundle: {path} is {} bytes; names are at most {MAX_ID_BYTES} bytes",
                id.len()
            ));
        }
        Ok(())
    };
    let (mut places, mut transitions, mut arcs, mut ports, mut constraints) = (0, 0, 0, 0, 0);
    for (i, s) in b.subnets.iter().enumerate() {
        let sp = format!("subnets[{i}]");
        long(format!("{sp}.id"), &s.id)?;
        ports += s.ports.len();
        for (j, p) in s.ports.iter().enumerate() {
            let pp = format!("{sp}.ports[{j}]");
            long(format!("{pp}.id"), &p.id)?;
            long(format!("{pp}.place"), &p.place)?;
            long(format!("{pp}.transition"), &p.transition)?;
            long(format!("{pp}.schema"), &p.schema)?;
        }
        let m = &s.model;
        places += m.places.len();
        transitions += m.transitions.len();
        arcs += m.arcs.len();
        constraints += m.constraints.len();
        for (j, p) in m.places.iter().enumerate() {
            let pp = format!("{sp}.model.places[{j}]");
            long(format!("{pp}.id"), &p.id)?;
            long(format!("{pp}.type"), &p.typ)?;
        }
        for (j, t) in m.transitions.iter().enumerate() {
            long(format!("{sp}.model.transitions[{j}].id"), &t.id)?;
        }
        for (j, a) in m.arcs.iter().enumerate() {
            let ap = format!("{sp}.model.arcs[{j}]");
            long(format!("{ap}.from"), &a.from)?;
            long(format!("{ap}.to"), &a.to)?;
        }
        for (j, c) in m.constraints.iter().enumerate() {
            long(format!("{sp}.model.constraints[{j}].id"), &c.id)?;
        }
    }
    constraints += b.constraints.len();
    for (j, c) in b.constraints.iter().enumerate() {
        long(format!("constraints[{j}].id"), &c.id)?;
    }
    for (i, l) in b.links.iter().enumerate() {
        let lp = format!("links[{i}]");
        long(format!("{lp}.id"), &l.id)?;
        for (end, e) in [("from", &l.from), ("to", &l.to)] {
            long(format!("{lp}.{end}.subnet"), &e.subnet)?;
            long(format!("{lp}.{end}.port"), &e.port)?;
            long(format!("{lp}.{end}.place"), &e.place)?;
            long(format!("{lp}.{end}.transition"), &e.transition)?;
        }
    }
    for (what, n, limit) in [
        ("places", places, MAX_PLACES),
        ("transitions", transitions, MAX_TRANSITIONS),
        ("arcs", arcs, MAX_ARCS),
        ("ports", ports, MAX_PORTS),
        ("constraints", constraints, MAX_CONSTRAINTS),
    ] {
        if n > limit {
            return Err(format!("{tool}: bundle: {n} {what}; at most {limit}"));
        }
    }
    Ok(())
}

// ------------------------------------------------------------ ports

/// The ports a subnet offers the tools that derive them (every tool but
/// pflow_bundle_candidates): those it declares, else one inout port per
/// exported place, named for it (a place id that repeats gives one port, and
/// only when its first place is exported), as the hosted `mbDerivePorts`.
pub fn effective_ports(s: &Subnet) -> Vec<Port> {
    if !s.ports.is_empty() {
        return s.ports.clone();
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    for p in &s.model.places {
        let first = s.model.places.iter().find(|q| q.id == p.id);
        if p.exported && first.is_some_and(|f| f.exported) && seen.insert(p.id.clone()) {
            out.push(Port {
                id: p.id.clone(),
                kind: Some(PortKind::Inout),
                place: p.id.clone(),
                schema: first.map(|f| f.typ.clone()).unwrap_or_default(),
                ..Default::default()
            });
        }
    }
    out
}

/// Which ports a subnet has and how to get one (`mbPortsHint`). `derived`:
/// whether a subnet that declares none has one per exported place.
pub fn ports_hint(s: &Subnet, ports: &[Port], derived: bool) -> String {
    let mut ids: Vec<String> = ports.iter().map(|p| quote(&p.id)).collect();
    ids.sort();
    let sid = quote(&s.id);
    if !ids.is_empty() && ids.len() <= 40 {
        format!("subnet {sid} has the ports {}", ids.join(", "))
    } else if ids.len() > 40 {
        format!(
            "subnet {sid} has {} ports ({}, ...)",
            ids.len(),
            ids[..40].join(", ")
        )
    } else if derived {
        format!("subnet {sid} has no ports: declare them in its \"ports\" (a port names an exported place, or a transition with \"target\": \"transition\"), or export places, whose ids become its ports")
    } else {
        format!("subnet {sid} declares no ports, so it offers no candidates: declare them in its \"ports\" (a port names an exported place, or a transition with \"target\": \"transition\")")
    }
}
