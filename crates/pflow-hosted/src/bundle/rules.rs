//! The composition rules behind the tools (pflow-xyz
//! `cmd/webserver/mcp_model_rules.go`, itself pflow-native's
//! `composition/mod.rs`): a link's verdict, the candidates a drag from a
//! port could reach, the flat net and its view, a firing script on it and
//! its incidence matrix.
//!
//! The decisions are pflow-rs's: a link is judged by pflow-compose's
//! [`Bundle::validate_one_link`], the flat net is
//! [`Bundle::flatten_with_map`], and a script fires through
//! `pflow_metamodel::Model::enabled_why_not` / `fire`. What this adds is the
//! shape of the answers and the two page rules (`rule: "page"`): a link joins
//! two different subnets (PAGE_SAME_SUBNET) and the same link is not added
//! twice (PAGE_DUPLICATE, an event link either way round). Verdicts leave out
//! native's descriptive `passed` list, as the hosted tools do.

use std::collections::{BTreeMap, HashMap};

use pflow_compose::{Bundle, Endpoint, FlattenMap, Link, LinkKind};
use pflow_metamodel::{ArcType, Marking, Model};
use serde_json::{json, Map, Value};

use super::text::{json as json_text, quote, quote_list};
use super::MAX_ANSWER_BYTES;

// ------------------------------------------------------------ ports, links, endpoints

/// "subnet:port", the page's port reference.
pub fn port_key(e: &Endpoint) -> String {
    format!("{}:{}", e.subnet, e.port)
}

/// "kind:subnet:port->subnet:port".
pub fn link_key(l: &Link) -> String {
    format!(
        "{}:{}->{}",
        l.kind.name(),
        port_key(&l.from),
        port_key(&l.to)
    )
}

pub fn end_view(e: &Endpoint) -> Value {
    json!({"subnet": e.subnet, "port": e.port})
}

pub fn link_view(l: &Link) -> Value {
    json!({"kind": l.kind.name(), "from": end_view(&l.from), "to": end_view(&l.to)})
}

pub fn port_end(subnet: &str, port: &str) -> Endpoint {
    Endpoint {
        subnet: subnet.into(),
        port: port.into(),
        ..Default::default()
    }
}

pub fn new_link(kind: LinkKind, from: Endpoint, to: Endpoint) -> Link {
    Link {
        kind,
        from,
        to,
        ..Default::default()
    }
}

pub const KINDS: [LinkKind; 4] = [
    LinkKind::Token,
    LinkKind::Data,
    LinkKind::Event,
    LinkKind::Guard,
];

pub fn parse_kind(s: &str) -> Result<LinkKind, String> {
    KINDS
        .into_iter()
        .find(|k| k.name() == s)
        .ok_or_else(|| format!("unknown link kind {} (token, data, event, guard)", quote(s)))
}

/// `{"subnet", "port"}` or `"subnet:port"` (these tools link ports: an end on
/// a place or a transition is refused).
pub fn parse_endpoint(v: &Value) -> Result<Endpoint, String> {
    match v {
        Value::String(x) => match x.split_once(':') {
            Some((a, b)) if !a.is_empty() && !b.is_empty() => Ok(port_end(a, b)),
            _ => Err(format!("endpoint {} is not \"subnet:port\"", quote(x))),
        },
        Value::Object(o) => {
            let get = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or("");
            let (a, b, place, trans) =
                (get("subnet"), get("port"), get("place"), get("transition"));
            if !a.is_empty() && b.is_empty() && (!place.is_empty() || !trans.is_empty()) {
                return Err(format!(
                    "endpoint {} names a place or a transition: these tools link ports",
                    json_text(v)
                ));
            }
            if a.is_empty() || b.is_empty() {
                return Err(format!(
                    "endpoint {} needs \"subnet\" and one of \"port\", \"place\", \"transition\"",
                    json_text(v)
                ));
            }
            Ok(port_end(a, b))
        }
        other => Err(format!(
            "an endpoint is {{\"subnet\", \"port\"}} or \"subnet:port\", not {}",
            json_text(other)
        )),
    }
}

/// `{kind, from, to}`.
pub fn parse_link(v: &Value) -> Result<Link, String> {
    let Some(kind) = v.get("kind").and_then(Value::as_str) else {
        return Err(format!("link {} has no \"kind\"", json_text(v)));
    };
    let k = parse_kind(kind)?;
    let from = parse_endpoint(v.get("from").unwrap_or(&Value::Null))?;
    let to = parse_endpoint(v.get("to").unwrap_or(&Value::Null))?;
    Ok(new_link(k, from, to))
}

// ------------------------------------------------------------ verdicts

/// A refusal or warning, with whose rule it is.
#[derive(Debug, Clone)]
pub struct Issue {
    pub code: String,
    pub message: String,
    pub rule: &'static str,
}

/// The judgment on one link.
#[derive(Debug, Clone)]
pub struct Verdict {
    pub ok: bool,
    pub kind: &'static str,
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

impl Verdict {
    pub fn view(&self) -> Value {
        let view = |is: &[Issue]| -> Vec<Value> {
            is.iter()
                .map(|i| json!({"code": i.code, "message": i.message, "rule": i.rule}))
                .collect()
        };
        json!({"ok": self.ok, "kind": self.kind, "errors": view(&self.errors), "warnings": view(&self.warnings)})
    }

    /// "[CODE] message; …", the error a refused edit returns.
    pub fn refusal(&self) -> String {
        self.errors
            .iter()
            .map(|e| format!("[{}] {}", e.code, e.message))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// The label pflow-compose gives the one link of the bundle a verdict is
/// computed on.
pub const LINK_LABEL: &str = "links[0]";

/// The same-subnet page rule, then [`Bundle::validate_one_link`]: errors are
/// all of it, warnings the link's own.
pub fn validate_link(b: &Bundle, l: &Link) -> Verdict {
    let mut v = Verdict {
        ok: true,
        kind: l.kind.name(),
        errors: vec![],
        warnings: vec![],
    };
    if l.from.subnet == l.to.subnet {
        v.ok = false;
        v.errors.push(Issue {
            code: "PAGE_SAME_SUBNET".into(),
            message: format!(
                "{LINK_LABEL}: both ends are on subnet {}; a link connects two subnets",
                quote(&l.from.subnet)
            ),
            rule: "page",
        });
        return v;
    }
    let res = b.validate_one_link(l);
    let rule = "pflow-compose";
    v.errors = res
        .errors
        .into_iter()
        .map(|e| Issue {
            code: e.code,
            message: e.message,
            rule,
        })
        .collect();
    v.warnings = res
        .warnings
        .into_iter()
        .filter(|w| w.element == LINK_LABEL)
        .map(|e| Issue {
            code: e.code,
            message: e.message,
            rule,
        })
        .collect();
    v.ok = v.errors.is_empty();
    v
}

/// Whether the bundle already has `l` (an event link either way round).
fn has_link(b: &Bundle, l: &Link) -> bool {
    let key = link_key(l);
    let rev = link_key(&new_link(l.kind, l.to.clone(), l.from.clone()));
    b.links
        .iter()
        .any(|x| link_key(x) == key || (l.kind == LinkKind::Event && link_key(x) == rev))
}

/// The verdict on `l` against this bundle, page rules included.
pub fn verdict_in(b: &Bundle, l: &Link) -> Verdict {
    let mut v = validate_link(b, l);
    if v.ok && has_link(b, l) {
        v.ok = false;
        v.errors.push(Issue {
            code: "PAGE_DUPLICATE".into(),
            message: "already linked".into(),
            rule: "page",
        });
    }
    v
}

// ------------------------------------------------------------ candidates

/// The kinds worth trying from a port to a port, in the order "auto"
/// prefers; `true`: a guard oriented transition → place, the drag having
/// started at the place.
fn kinds_for(from_transition: bool, to_transition: bool) -> Vec<(LinkKind, bool)> {
    match (from_transition, to_transition) {
        (true, true) => vec![(LinkKind::Event, false)],
        (true, false) => vec![(LinkKind::Guard, false)],
        (false, true) => vec![(LinkKind::Guard, true)],
        (false, false) => vec![(LinkKind::Token, false), (LinkKind::Data, false)],
    }
}

/// A refusal in native's words, with the hosted tool's hint behind it.
pub struct Hinted {
    pub native: String,
    pub hint: String,
}

/// Every other declared port of the bundle, judged as the far end of a link
/// dragged from `from`; for each the first legal kind wins, else the first
/// kind tried is kept.
pub fn candidates(b: &Bundle, from: &Endpoint, prefer: &str) -> Result<Vec<Value>, Hinted> {
    let plain = |native: String| Hinted {
        native,
        hint: String::new(),
    };
    let prefer_kind = match prefer {
        "auto" => None,
        k => Some(parse_kind(k).map_err(plain)?),
    };
    let fs = b.subnet_by_id(&from.subnet).ok_or_else(|| {
        plain(format!(
            "no subnet {} in the composition",
            quote(&from.subnet)
        ))
    })?;
    let fp = fs.port_by_id(&from.port).ok_or_else(|| Hinted {
        native: format!(
            "subnet {} has no port {}",
            quote(&from.subnet),
            quote(&from.port)
        ),
        hint: super::read::ports_hint(fs, &fs.ports, false),
    })?;
    let mut out = vec![];
    for s in &b.subnets {
        for tp in &s.ports {
            if s.id == from.subnet && tp.id == from.port {
                continue;
            }
            let to = port_end(&s.id, &tp.id);
            let mut kinds = kinds_for(fp.is_transition(), tp.is_transition());
            if let Some(k) = prefer_kind {
                kinds.retain(|(x, _)| *x == k);
                if kinds.is_empty() {
                    kinds = vec![(k, false)];
                }
            }
            let mut tried: Vec<(Link, Verdict)> = vec![];
            let mut best = 0;
            for (kind, swap) in kinds {
                let l = if swap {
                    new_link(kind, to.clone(), from.clone())
                } else {
                    new_link(kind, from.clone(), to.clone())
                };
                let v = verdict_in(b, &l);
                let ok = v.ok;
                tried.push((l, v));
                if ok {
                    best = tried.len() - 1;
                    break;
                }
            }
            let (bl, bv) = &tried[best];
            out.push(json!({
                "ref": end_view(&to),
                "key": port_key(&to),
                "ok": bv.ok,
                "link": link_view(bl),
                "verdict": bv.view(),
                "tried": tried.iter().map(|(l, v)| json!({"link": link_view(l), "verdict": v.view()})).collect::<Vec<_>>(),
            }));
        }
    }
    Ok(out)
}

// ------------------------------------------------------------ the flat net

/// A flattened bundle: the net and how it was made.
pub struct Flat {
    pub model: Model,
    pub map: FlattenMap,
}

pub fn flatten(b: &Bundle) -> Result<Flat, String> {
    b.flatten_with_map().map(|(model, map)| Flat { model, map })
}

fn member(m: &HashMap<String, HashMap<String, String>>, name: &str) -> Option<String> {
    let (s, local) = name.split_once('/')?;
    m.get(s)?.get(local).cloned()
}

fn invert(m: &HashMap<String, HashMap<String, String>>) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (s, locals) in m {
        for (local, flat) in locals {
            out.entry(flat.clone())
                .or_default()
                .push(format!("{s}/{local}"));
        }
    }
    for v in out.values_mut() {
        v.sort();
    }
    out
}

fn sorted(m: &HashMap<String, Vec<String>>) -> Value {
    let b: BTreeMap<&String, &Vec<String>> = m.iter().collect();
    json!(b)
}

fn nested(m: &HashMap<String, HashMap<String, String>>) -> Value {
    let b: BTreeMap<&String, BTreeMap<&String, &String>> =
        m.iter().map(|(k, v)| (k, v.iter().collect())).collect();
    json!(b)
}

pub fn arc_type(t: ArcType) -> &'static str {
    match t {
        ArcType::Normal => "",
        ArcType::Inhibitor => "inhibitor",
        ArcType::Read => "read",
    }
}

pub fn place_view(p: &pflow_metamodel::Place) -> Value {
    json!({
        "id": p.id,
        "kind": if p.is_token() { "token" } else { "data" },
        "initial": p.initial,
        "capacity": p.capacity,
        "exported": p.exported,
        "resource": p.resource,
        "description": p.description,
    })
}

pub fn arcs_view(m: &Model) -> Vec<Value> {
    m.arcs
        .iter()
        .map(|a| json!({"from": a.from, "to": a.to, "weight": a.weight, "type": arc_type(a.typ)}))
        .collect()
}

pub fn constraints_view(m: &Model) -> Vec<Value> {
    m.constraints
        .iter()
        .map(|c| json!({"id": c.id, "expr": c.expr}))
        .collect()
}

/// The arcs of a model between a token place and a transition with no
/// weight (unless `moving_only`, the read and inhibitor arcs among them):
/// every firing rule reads that as 1, a sum over the arcs as 0.
pub fn weightless(m: &Model, moving_only: bool) -> usize {
    let token: std::collections::HashSet<&str> = m
        .places
        .iter()
        .filter(|p| p.is_token())
        .map(|p| p.id.as_str())
        .collect();
    let transition: std::collections::HashSet<&str> =
        m.transitions.iter().map(|t| t.id.as_str()).collect();
    m.arcs
        .iter()
        .filter(|a| {
            let touches = (token.contains(a.from.as_str()) && transition.contains(a.to.as_str()))
                || (transition.contains(a.from.as_str()) && token.contains(a.to.as_str()));
            a.weight == 0 && touches && !(moving_only && a.is_read_only())
        })
        .count()
}

impl Flat {
    /// The flat transition a member "<subnet>/<transition>" ended up in.
    pub fn transition_of(&self, name: &str) -> Option<String> {
        member(&self.map.transition, name)
    }

    pub fn place_of(&self, name: &str) -> Option<String> {
        member(&self.map.place, name)
    }

    /// pflow_bundle_flatten's answer.
    pub fn view(&self) -> Value {
        let pm = invert(&self.map.place);
        let tm = invert(&self.map.transition);
        let places: Vec<Value> = self
            .model
            .places
            .iter()
            .map(|p| {
                let mut v = place_view(p);
                v["members"] = json!(pm.get(&p.id).cloned().unwrap_or_default());
                v
            })
            .collect();
        let transitions: Vec<Value> = self
            .model
            .transitions
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "description": t.description,
                    "members": tm.get(&t.id).cloned().unwrap_or_default(),
                    "fused": self.map.fused_groups.contains_key(&t.id),
                })
            })
            .collect();
        let mk = self.model.initial_marking();
        let enablement: Vec<Value> = self
            .model
            .transitions
            .iter()
            .map(|t| match self.model.enabled_why_not(&t.id, &mk) {
                Ok(()) => json!({"transition": t.id, "enabled": true, "why": ""}),
                Err(e) => json!({"transition": t.id, "enabled": false, "why": e.to_string()}),
            })
            .collect();
        let mut view = json!({
            "ok": true,
            "error": "",
            "places": places,
            "transitions": transitions,
            "arcs": arcs_view(&self.model),
            "constraints": constraints_view(&self.model),
            "wires": sorted(&self.map.wires),
            "fused_groups": sorted(&self.map.fused_groups),
            "place_map": nested(&self.map.place),
            "transition_map": nested(&self.map.transition),
            "warnings": self.map.warnings.iter().map(|w| json!({"code": w.code, "element": w.element, "message": w.message})).collect::<Vec<_>>(),
            "initial_enablement": enablement,
        });
        // (flattening several subnets gives an arc without a weight the
        // weight 1; a lone subnet is returned as it is)
        let n = weightless(&self.model, false);
        if n > 0 {
            view["notes"] = json!([format!(
                "{n} of the {} arcs have no weight: they are listed with weight 0 here but fire as weight 1 (pflow_bundle_run, the initial enablement above). Give every arc an explicit weight.",
                self.model.arcs.len()
            )]);
        }
        view
    }

    /// An estimate of pflow_bundle_flatten's answer in bytes, made before it
    /// is built (`answerBytes`).
    pub fn answer_bytes(&self) -> usize {
        let m = &self.model;
        let mut n = 0;
        for p in &m.places {
            n += 160 + p.id.len() + p.description.len();
        }
        for t in &m.transitions {
            n += 320 + 3 * t.id.len() + t.description.len();
        }
        for a in &m.arcs {
            n += 64 + a.from.len() + a.to.len();
        }
        for c in &m.constraints {
            n += 32 + c.id.len() + c.expr.len();
        }
        for groups in [&self.map.wires, &self.map.fused_groups] {
            for (name, members) in groups {
                n += 16 + name.len();
                n += members.iter().map(|x| 6 + x.len()).sum::<usize>();
            }
        }
        for map in [&self.map.place, &self.map.transition] {
            for (subnet, locals) in map {
                for (local, flat) in locals {
                    n += 2 * (subnet.len() + local.len()) + flat.len() + 24;
                }
            }
        }
        for w in &self.map.warnings {
            n += 64 + w.code.len() + w.element.len() + w.message.len();
        }
        n
    }

    /// Refuses a flat net whose answer would pass [`MAX_ANSWER_BYTES`].
    pub fn check_answer(&self, tool: &str) -> Result<(), String> {
        let n = self.answer_bytes();
        if n > MAX_ANSWER_BYTES {
            return Err(format!("{tool}: the flat net's answer would be about {n} bytes (at most {MAX_ANSWER_BYTES}): long fused transition names are repeated in every list and map that mentions them; use shorter ids or fewer event links"));
        }
        Ok(())
    }
}

// ------------------------------------------------------------ running a script

/// A predicate on the marking at the end of a run.
#[derive(Debug, Clone)]
pub struct Expect {
    pub place: String,
    pub n: i64,
    pub says: String,
}

fn marking_view(mk: &Marking) -> Value {
    let b: BTreeMap<&String, &i64> = mk.iter().collect();
    json!(b)
}

/// Fires `script` (members "<subnet>/<transition>") on the flat net from its
/// initial marking, stopping at the first member that is not in the net or
/// cannot fire; then evaluates `expect` on the marking reached.
pub fn run(
    flat: &Flat,
    id: &str,
    title: &str,
    script: &[String],
    expect: &[Expect],
) -> Result<Value, String> {
    let m = &flat.model;
    let mut mk = m.initial_marking();
    let mut steps = vec![];
    let mut stopped_at: i64 = -1;
    let marking_size: usize = m.places.iter().map(|p| 24 + p.id.len()).sum();
    let mut size = 2 * marking_size;
    let mut markings = vec![marking_view(&mk)];
    for (i, name) in script.iter().enumerate() {
        let t = flat.transition_of(name);
        let why = match &t {
            None => format!("no member {} in the composition", quote(name)),
            Some(t) => m
                .enabled_why_not(t, &mk)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default(),
        };
        steps.push(json!({"member": name, "flat": t, "enabled": why.is_empty(), "why": why}));
        size += 80 + name.len() + t.as_deref().map_or(0, str::len) + why.len() + marking_size;
        if size > MAX_ANSWER_BYTES {
            return Err(format!("pflow_bundle_run: the answer would be over {MAX_ANSWER_BYTES} bytes (the steps name long fused transitions, and each step lists the marking it reached); use a shorter script or shorter ids"));
        }
        if !why.is_empty() {
            stopped_at = i as i64;
            break;
        }
        mk = m.fire(t.as_deref().unwrap_or_default(), &mk);
        markings.push(marking_view(&mk));
    }
    let results: Vec<Value> = expect
        .iter()
        .map(|e| {
            let f = flat.place_of(&e.place);
            let actual = f.as_ref().and_then(|p| mk.get(p)).copied().unwrap_or(0);
            json!({
                "place": e.place, "flat": f, "op": "==", "n": e.n, "says": e.says,
                "actual": actual, "pass": actual == e.n,
            })
        })
        .collect();
    Ok(json!({
        "id": id,
        "title": title,
        "steps": steps,
        "completed": stopped_at < 0,
        "stopped_at": stopped_at,
        "stopped_step": (stopped_at >= 0).then(|| script[stopped_at as usize].clone()),
        "final_marking": marking_view(&mk),
        "markings": markings,
        "expect": results,
    }))
}

// ------------------------------------------------------------ incidence

/// C = post − pre of a model (rows: places, columns: transitions, in model
/// order; the first of a repeated id). Read and inhibitor arcs move nothing.
pub fn incidence(m: &Model) -> Vec<Vec<i64>> {
    let mut c = vec![vec![0i64; m.transitions.len()]; m.places.len()];
    let pi = |id: &str| m.places.iter().position(|p| p.id == id);
    let ti = |id: &str| m.transitions.iter().position(|t| t.id == id);
    for a in &m.arcs {
        if a.is_read_only() {
            continue;
        }
        if let (Some(p), Some(t)) = (pi(&a.from), ti(&a.to)) {
            c[p][t] = c[p][t].wrapping_sub(a.weight);
        } else if let (Some(t), Some(p)) = (ti(&a.from), pi(&a.to)) {
            c[p][t] = c[p][t].wrapping_add(a.weight);
        }
    }
    c
}

/// A declared constraint `tokens("a") + … == n` read as a P-invariant y (1
/// on each place it names): y, n, y·C per transition, y·M₀ and whether it
/// holds (y·C = 0 and y·M₀ = n). `None` when the expression is not such a
/// sum or names a place the net lacks.
pub fn invariant_check(m: &Model, c: &[Vec<i64>], expr: &str) -> Option<Map<String, Value>> {
    let (lhs, rhs) = expr.rsplit_once(" == ")?;
    let n: i64 = rhs.trim().parse().ok()?;
    let mut y = vec![0i64; m.places.len()];
    let mut named: Vec<(String, i64)> = vec![];
    for term in lhs.split(" + ") {
        let p = term.trim().strip_prefix("tokens(\"")?.strip_suffix("\")")?;
        let pos = m.places.iter().position(|q| q.id == p)?;
        y[pos] += 1;
        match named.iter_mut().find(|(q, _)| q == p) {
            Some(e) => e.1 += 1,
            None => named.push((p.to_string(), 1)),
        }
    }
    let y_m0 = m
        .places
        .iter()
        .zip(&y)
        .fold(0i64, |s, (p, k)| s.wrapping_add(k.wrapping_mul(p.initial)));
    let y_c: Vec<i64> = (0..m.transitions.len())
        .map(|t| (0..y.len()).fold(0i64, |s, p| s.wrapping_add(y[p].wrapping_mul(c[p][t]))))
        .collect();
    let holds = y_c.iter().all(|v| *v == 0) && y_m0 == n;
    let mut out = Map::new();
    out.insert("holds".into(), json!(holds));
    out.insert("y".into(), json!(named));
    out.insert("n".into(), json!(n));
    out.insert("y_c".into(), json!(y_c));
    out.insert("y_m0".into(), json!(y_m0));
    Some(out)
}

/// `[\"a\", \"b\"]` for a refusal that lists names.
pub fn names(xs: &[&str]) -> String {
    quote_list(xs)
}
