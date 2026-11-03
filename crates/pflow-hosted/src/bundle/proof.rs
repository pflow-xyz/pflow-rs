//! `bundle_prove` and `bundle_check_proof`: behavioural consistency of the
//! glue, and a proof record a second reader can re-check.
//!
//! `bundle_validate` judges a link by its ports (schema, direction, kind). This
//! judges what the glue *does* to the nets it joins:
//!
//! * **Part laws.** Every conservation law a part carries (its declared
//!   constraints, and the minimal P-invariants of the part alone) is carried
//!   onto the composite's places and tried against the composite's incidence
//!   matrix. It is `preserved` (y·C = 0 still), `extended` (it is the part's
//!   share of a larger composite law z, which is named) or `changed` (the
//!   composite moves it, and the residual names which transitions, so the
//!   change is explained, not just noticed). A declared law is also tried
//!   against every reachable marking, so a `changed` law that never breaks is
//!   told apart from one that does (with a replayable counterexample).
//! * **Composite claims.** Boundedness (a positive-cover invariant, a capacity,
//!   or an exhaustive search), deadlock-freedom and quasi-liveness, each
//!   `proved`, `refuted` (with a replayable witness) or `unknown` (the search
//!   stopped). Nothing is `proved` by a search that did not finish.
//! * **Per-link verdicts.** Each link is judged by taking it out and running
//!   the same analyses: a deadlock, unbounded growth or blocked transition that
//!   exists with the link and not without it is the link's.
//!
//! The record carries certificates (vectors, traces, matrices' rows) that
//! [`check`] verifies with arithmetic and replay, and the engine's own answers
//! that it recomputes. A reader that trusts neither the engine nor this module
//! needs only the flat net (`bundle_net`) and the certificate rules in
//! [`CHECK_RULES`].

use std::collections::{BTreeMap, BTreeSet, HashMap};

use pflow_compose::{Bundle, PortTarget};
use pflow_metamodel::{Marking, Model};
use pflow_reachability::{Analyzer, InvariantAnalyzer, Verdict};
use serde_json::{json, Map, Value};

use super::flatnet;
use super::read;
use super::rules::{self, Flat};

pub const RECORD_TYPE: &str = "CompositionProof";
pub const RECORD_VERSION: u64 = 1;
const DEFAULT_STATES: usize = 2000;
const MAX_STATES: usize = 20000;
const DEFAULT_TOKENS: i64 = 1000;
/// Links judged one by one (each is a reachability run).
const MAX_ABLATED: usize = 60;
/// Laws per part kept in the record.
const MAX_LAWS: usize = 24;

/// What a reader needs to re-check the record without this crate.
pub const CHECK_RULES: &[&str] = &[
    "net: the flat net (bundle_net); C[p][t] = (weight of t->p) - (weight of p->t) over normal arcs; read and inhibitor arcs move nothing",
    "law: a vector y over places with y.C = 0 is conserved; its value is y.M0",
    "preserved: the part's law lifted onto the composite's places has y.C = 0 and residual = {}",
    "extended: z.C = 0, z >= 0, and z equals the lifted law (up to the ratio given) on every composite place that holds a member of the part",
    "changed: residual[t] = y.C[t] != 0 exactly for the transitions listed",
    "bounded by invariant: z.C = 0, z >= 0, z[p] > 0, bound(p) = floor(z.M0 / z[p])",
    "bounded by capacity: the place's capacity is positive (a transition that would pass it is not enabled)",
    "witness: fire the trace from the initial marking; every step must be enabled; the end marking must equal the one given, and for a deadlock nothing may be enabled there",
    "complete search: no certificate beyond the engine's count; re-running the search with the same bounds must give the same verdict",
];

// ------------------------------------------------------------ options

pub struct Opts {
    pub max_states: usize,
    pub max_tokens: i64,
    pub ablate: bool,
}

impl Opts {
    pub fn from_args(a: &Map<String, Value>, tool: &str) -> Result<Opts, String> {
        let num = |k: &str, def: i64, lo: i64, hi: i64| -> Result<i64, String> {
            match a.get(k) {
                None | Some(Value::Null) => Ok(def),
                Some(v) => match v.as_i64() {
                    Some(n) if (lo..=hi).contains(&n) => Ok(n),
                    _ => Err(format!("{tool}: {k}: an integer in [{lo}, {hi}]")),
                },
            }
        };
        Ok(Opts {
            max_states: num("max_states", DEFAULT_STATES as i64, 1, MAX_STATES as i64)? as usize,
            max_tokens: num("max_tokens", DEFAULT_TOKENS, 1, 1_000_000)?,
            ablate: a.get("ablate").and_then(Value::as_bool).unwrap_or(true),
        })
    }

    fn view(&self) -> Value {
        json!({"max_states": self.max_states, "max_tokens": self.max_tokens, "ablate": self.ablate})
    }
}

// ------------------------------------------------------------ the analysis of one flat net

/// What one bounded search of a flat net found.
struct Search {
    states: usize,
    complete: bool,
    bounded: Verdict3,
    deadlock_free: Verdict,
    quasi_live: Verdict,
    /// A shortest trace to a dead marking, and the marking.
    dead: Option<(Vec<String>, Marking)>,
    dead_count: usize,
    fired: BTreeSet<String>,
    never_fires: Vec<String>,
    max_tokens: BTreeMap<String, i64>,
    /// Every explored marking (for declared-law checks); small by the state cap.
    markings: Vec<Marking>,
}

#[derive(Clone, Copy, PartialEq)]
enum Verdict3 {
    True,
    False,
    Unknown,
}

fn verdict_str(v: Verdict) -> &'static str {
    v.as_str()
}

fn search(m: &Model, o: &Opts) -> Search {
    let r = Analyzer::new(m)
        .with_max_states(o.max_states)
        .with_max_tokens(o.max_tokens)
        .analyze();
    let lv = &r.liveness;
    let mut markings = vec![];
    for s in r.graph.states_list() {
        markings.push(s.marking.clone());
    }
    let dead = lv
        .witness
        .as_ref()
        .filter(|w| w.kind == pflow_reachability::WitnessKind::Deadlock)
        .map(|w| (w.trace.clone(), w.marking.clone()));
    Search {
        states: r.state_count,
        complete: r.is_complete,
        bounded: if r.is_complete {
            if r.bounded {
                Verdict3::True
            } else {
                Verdict3::False
            }
        } else {
            Verdict3::Unknown
        },
        deadlock_free: lv.deadlock_free,
        quasi_live: lv.quasi_live,
        dead,
        dead_count: r.dead_markings.len(),
        fired: r.fired_transitions.iter().cloned().collect(),
        never_fires: r.dead_trans.clone(),
        max_tokens: r.max_tokens.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        markings,
    }
}

fn mk_view(mk: &Marking) -> Value {
    let b: BTreeMap<&String, &i64> = mk.iter().filter(|(_, v)| **v != 0).collect();
    json!(b)
}

// ------------------------------------------------------------ laws

/// A conservation law over a part's own places.
struct Law {
    part: String,
    id: String,
    declared: bool,
    /// local place -> coefficient
    y: BTreeMap<String, i64>,
    /// the value the law holds (declared: n; derived: y·M0 of the part)
    n: i64,
}

fn render_law(y: &BTreeMap<String, i64>, n: i64) -> String {
    let mut s = String::new();
    for (p, c) in y {
        if *c == 0 {
            continue;
        }
        if s.is_empty() {
            if *c < 0 {
                s.push('-');
            }
        } else {
            s.push_str(if *c < 0 { " - " } else { " + " });
        }
        if c.abs() != 1 {
            s.push_str(&format!("{}*", c.abs()));
        }
        s.push_str(p);
    }
    format!("{s} == {n}")
}

/// `tokens("a") + tokens("b") == n` read as y.
fn parse_sum(expr: &str) -> Option<(BTreeMap<String, i64>, i64)> {
    let (lhs, rhs) = expr.rsplit_once(" == ")?;
    let n: i64 = rhs.trim().parse().ok()?;
    let mut y = BTreeMap::new();
    for term in lhs.split(" + ") {
        let p = term.trim().strip_prefix("tokens(\"")?.strip_suffix("\")")?;
        *y.entry(p.to_string()).or_insert(0) += 1;
    }
    Some((y, n))
}

fn part_laws(b: &Bundle) -> Vec<Law> {
    let mut out = vec![];
    for s in &b.subnets {
        let mut mine: Vec<Law> = vec![];
        for c in &s.model.constraints {
            if let Some((y, n)) = parse_sum(&c.expr) {
                if y.keys().all(|p| s.model.place_by_id(p).is_some()) {
                    mine.push(Law { part: s.id.clone(), id: c.id.clone(), declared: true, y, n });
                }
            }
        }
        let an = InvariantAnalyzer::new(&s.model);
        let init: Marking = s.model.initial_marking();
        for (k, inv) in an.find_p_invariants(&init).iter().enumerate() {
            let y: BTreeMap<String, i64> = inv.coefficients.iter().map(|(p, c)| (p.clone(), *c)).collect();
            if let Some(d) = mine.iter_mut().find(|l| l.y == y) {
                d.id = format!("{} (also a minimal invariant)", d.id);
                continue;
            }
            mine.push(Law { part: s.id.clone(), id: format!("invariant#{}", k + 1), declared: false, y, n: inv.value });
        }
        mine.truncate(MAX_LAWS);
        out.extend(mine);
    }
    out
}

/// y lifted onto the flat places.
fn lift(f: &Flat, l: &Law) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    for (p, c) in &l.y {
        if let Some(fp) = f.map.place.get(&l.part).and_then(|m| m.get(p)) {
            *out.entry(fp.clone()).or_insert(0) += c;
        }
    }
    out
}

/// y·C per flat transition, nonzero entries only.
fn residual(
    m: &Model,
    c: &[Vec<i64>],
    y: &BTreeMap<String, i64>,
    pidx: &HashMap<&str, usize>,
) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    for (t, tr) in m.transitions.iter().enumerate() {
        let mut r = 0i64;
        for (p, k) in y {
            if let Some(&i) = pidx.get(p.as_str()) {
                r += k * c[i][t];
            }
        }
        if r != 0 {
            out.insert(tr.id.clone(), r);
        }
    }
    out
}

/// The composite's minimal P-invariants as vectors over flat places.
fn composite_basis(m: &Model) -> (Vec<BTreeMap<String, i64>>, bool) {
    let an = InvariantAnalyzer::new(m);
    let r = an.p_invariant_basis();
    let v = r
        .basis
        .iter()
        .map(|row| {
            r.labels
                .iter()
                .zip(row)
                .filter(|(_, c)| **c != 0)
                .map(|(l, c)| (l.clone(), *c))
                .collect()
        })
        .collect();
    (v, r.truncated)
}

/// A composite invariant z that contains the lifted law: on every flat place
/// holding a member of the part, z is the same multiple of ŷ. Single basis
/// vectors first, then sums of two.
fn extension(
    basis: &[BTreeMap<String, i64>],
    yhat: &BTreeMap<String, i64>,
    part_places: &BTreeSet<String>,
) -> Option<(BTreeMap<String, i64>, (i64, i64))> {
    if yhat.is_empty() {
        return None;
    }
    let try_z = |z: &BTreeMap<String, i64>| -> Option<(i64, i64)> {
        // z = (num/den) * yhat on the part's places
        let (f0, &y0) = yhat.iter().next()?;
        let z0 = *z.get(f0).unwrap_or(&0);
        if z0 == 0 {
            return None;
        }
        let ok = part_places.iter().all(|f| {
            let zf = *z.get(f).unwrap_or(&0);
            let yf = *yhat.get(f).unwrap_or(&0);
            zf * y0 == yf * z0
        });
        ok.then_some((z0, y0))
    };
    for z in basis {
        if let Some(r) = try_z(z) {
            return Some((z.clone(), r));
        }
    }
    for (i, a) in basis.iter().enumerate() {
        for b in &basis[i + 1..] {
            let mut z = a.clone();
            for (p, c) in b {
                *z.entry(p.clone()).or_insert(0) += c;
            }
            if let Some(r) = try_z(&z) {
                return Some((z, r));
            }
        }
    }
    None
}

// ------------------------------------------------------------ resolving link ends

/// A link end's flat element: `("place"|"transition", flat id)`.
fn end_flat(b: &Bundle, f: &Flat, e: &pflow_compose::Endpoint) -> Option<(&'static str, String)> {
    let s = b.subnet_by_id(&e.subnet)?;
    let (is_t, local) = if !e.port.is_empty() {
        let ports = read::effective_ports(s);
        let p = ports.iter().find(|p| p.id == e.port)?;
        (matches!(p.target, Some(PortTarget::Transition)), p.element().to_string())
    } else if !e.place.is_empty() {
        (false, e.place.clone())
    } else {
        (true, e.transition.clone())
    };
    if is_t {
        f.map.transition.get(&e.subnet)?.get(&local).map(|x| ("transition", x.clone()))
    } else {
        f.map.place.get(&e.subnet)?.get(&local).map(|x| ("place", x.clone()))
    }
}

fn without_link(b: &Bundle, i: usize) -> Bundle {
    let mut c = b.clone();
    c.links.remove(i);
    c
}

// ------------------------------------------------------------ prove

pub fn prove(b: &Bundle, o: &Opts, tool: &str) -> Result<Value, String> {
    let f = rules::flatten(b).map_err(|e| format!("{tool}: {e}"))?;
    let m = &f.model;
    let order: Vec<String> = b.subnets.iter().map(|s| s.id.clone()).collect();
    let net = flatnet::editor_net(&b.name, &f, &order).ok();
    let composite_cid = net
        .as_ref()
        .and_then(|(n, _)| crate::cid::run(&json!({"net": n})).ok())
        .and_then(|v| v["cid"].as_str().map(String::from));
    let c = rules::incidence(m);
    let pidx: HashMap<&str, usize> = m.places.iter().enumerate().map(|(i, p)| (p.id.as_str(), i)).collect();

    let whole = search(m, o);
    let (basis, basis_truncated) = composite_basis(m);

    // ---- part laws
    let laws = part_laws(b);
    let mut law_views = vec![];
    let mut touched_by_place: HashMap<String, Vec<usize>> = HashMap::new();
    let mut touched_by_transition: HashMap<String, Vec<usize>> = HashMap::new();
    for l in &laws {
        let yhat = lift(&f, l);
        let part_places: BTreeSet<String> = f
            .map
            .place
            .get(&l.part)
            .map(|mm| mm.values().cloned().collect())
            .unwrap_or_default();
        let res = residual(m, &c, &yhat, &pidx);
        let ext = if res.is_empty() { None } else { extension(&basis, &yhat, &part_places) };
        let status = if res.is_empty() {
            "preserved"
        } else if ext.is_some() {
            "extended"
        } else {
            "changed"
        };
        // a declared law is the author's claim about the part's places: try it
        // on every reachable marking of the composite
        let (holds, counterexample) = if l.declared {
            let mut bad: Option<&Marking> = None;
            for mk in &whole.markings {
                let v: i64 = yhat.iter().map(|(p, k)| k * mk.get(p).copied().unwrap_or(0)).sum();
                if v != l.n {
                    bad = Some(mk);
                    break;
                }
            }
            match bad {
                Some(mk) => {
                    let trace = trace_to(m, o, mk);
                    ("refuted", Some(json!({"trace": trace, "marking": mk_view(mk)})))
                }
                None if whole.complete => ("proved", None),
                None => ("unknown", None),
            }
        } else {
            ("not_declared", None)
        };
        let idx = law_views.len();
        for p in yhat.keys() {
            touched_by_place.entry(p.clone()).or_default().push(idx);
        }
        for t in res.keys() {
            touched_by_transition.entry(t.clone()).or_default().push(idx);
        }
        let y0: i64 = yhat
            .iter()
            .map(|(p, k)| k * m.place_by_id(p).map_or(0, |q| q.initial))
            .sum();
        law_views.push(json!({
            "part": l.part,
            "id": l.id,
            "declared": l.declared,
            "law": render_law(&l.y, l.n),
            "lifted": yhat,
            "value": l.n,
            "initial_value": y0,
            "status": status,
            "residual": res,
            "extended_by": ext.as_ref().map(|(z, (zn, yn))| json!({"z": z, "ratio": [zn, yn]})),
            "holds_on_reachable": holds,
            "counterexample": counterexample,
        }));
    }

    // ---- composite claims
    let mut claims: Vec<Value> = vec![];
    claims.push(bounded_claim(m, &c, &pidx, &basis, basis_truncated, &whole));
    claims.push(deadlock_claim(&whole));
    claims.push(live_claim(&whole));

    // ---- links
    // which parts end on their own (a saga completes): their dead markings are intended
    let terminating: HashMap<&str, bool> = b
        .subnets
        .iter()
        .map(|sn| (sn.id.as_str(), search(&sn.model, o).deadlock_free == Verdict::False))
        .collect();
    let mut link_views = vec![];
    let ablate = o.ablate && b.links.len() <= MAX_ABLATED;
    for (i, l) in b.links.iter().enumerate() {
        let others = without_link(b, i);
        let sv = rules::validate_link(&others, l);
        let glue: Vec<Value> = [&l.from, &l.to]
            .iter()
            .filter_map(|e| end_flat(b, &f, e))
            .map(|(k, id)| json!({"kind": k, "flat": id}))
            .collect();
        let glue_places: BTreeSet<String> = glue
            .iter()
            .filter(|g| g["kind"] == "place")
            .filter_map(|g| g["flat"].as_str().map(String::from))
            .collect();
        let glue_ts: BTreeSet<String> = glue
            .iter()
            .filter(|g| g["kind"] == "transition")
            .filter_map(|g| g["flat"].as_str().map(String::from))
            .collect();
        let mut touching: BTreeSet<usize> = BTreeSet::new();
        for p in &glue_places {
            touching.extend(touched_by_place.get(p).into_iter().flatten().copied());
        }
        for t in &glue_ts {
            touching.extend(touched_by_transition.get(t).into_iter().flatten().copied());
        }
        // for a place link the residual transitions are the other part's moves
        // on the glued place; they are the ones "touching" through the place
        let laws_touched: Vec<Value> = touching
            .iter()
            .map(|&k| {
                let v = &law_views[k];
                json!({"part": v["part"], "id": v["id"], "law": v["law"], "status": v["status"], "holds_on_reachable": v["holds_on_reachable"]})
            })
            .collect();

        let mut findings: Vec<Value> = vec![];
        let mut level = 0u8; // 0 ok, 1 explained, 2 warn, 3 fail
        let bump = |level: &mut u8, to: u8| *level = (*level).max(to);
        if !sv.ok {
            bump(&mut level, 3);
            findings.push(json!({"code": "structural", "says": format!("the link is not legal: {}", sv.refusal())}));
        }
        // the author's declared laws one by one; derived laws summed up per part
        let mut derived: BTreeMap<(String, &str), Vec<String>> = BTreeMap::new();
        for w in sv.warnings.iter() {
            bump(&mut level, 1);
            findings.push(json!({"code": "structural_warning", "says": format!("{} ({})", w.message, w.code)}));
        }
        for &k in &touching {
            let v = &law_views[k];
            let part = v["part"].as_str().unwrap_or("");
            let law = v["law"].as_str().unwrap_or("");
            let status = v["status"].as_str().unwrap_or("");
            let holds = v["holds_on_reachable"].as_str().unwrap_or("");
            if status == "preserved" && holds != "refuted" {
                continue;
            }
            if v["declared"] != true {
                let st = if status == "extended" { "extended" } else { "changed" };
                derived.entry((part.to_string(), st)).or_default().push(law.to_string());
                continue;
            }
            if holds == "refuted" {
                // a falsified declared law is a failure unless the composite
                // conserves a larger law of which it is the part's share
                bump(&mut level, if status == "extended" { 2 } else { 3 });
                let ts: Vec<&str> = v["residual"].as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
                let tail = if status == "extended" { ", though the composite does conserve the larger law the record names (the token link shares the place, so each part's sum is only part of the whole)" } else { "" };
                findings.push(json!({"code": "law_broken", "law": v["id"], "says": format!("{part}'s declared law {law} is false in a reachable marking of the composite (moved by {}){tail}; the record has a replayable counterexample", ts.join(", "))}));
            } else if status == "extended" {
                bump(&mut level, 1);
                findings.push(json!({"code": "law_extended", "law": v["id"], "says": format!("{part}'s declared law {law} is no longer closed on its own: it is {part}'s share of a larger composite law, which the record names")}));
            } else {
                bump(&mut level, if holds == "proved" { 1 } else { 2 });
                let ts: Vec<&str> = v["residual"].as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
                findings.push(json!({"code": "law_changed", "law": v["id"], "says": format!("{part}'s declared law {law} is moved by {}{}", ts.join(", "), if holds == "proved" { ", yet it holds in every reachable marking" } else { "" })}));
            }
        }
        for ((part, st), ls) in &derived {
            bump(&mut level, 1);
            let e = ls.first().map(String::as_str).unwrap_or("");
            findings.push(json!({"code": if *st == "extended" { "derived_extended" } else { "derived_changed" }, "count": ls.len(),
                "says": format!("{} of {part}'s derived conservation laws (e.g. {e}) {} through this glue", ls.len(), if *st == "extended" { "continue as parts of larger composite laws" } else { "are moved" })}));
        }
        // ablation: what exists with the link and not without it
        let mut effect = Value::Null;
        if ablate {
            if let Ok(g) = rules::flatten(&others) {
                let s0 = search(&g.model, o);
                let mut e = Map::new();
                e.insert("states_without".into(), json!(s0.states));
                e.insert("deadlock_free_without".into(), json!(verdict_str(s0.deadlock_free)));
                e.insert("deadlock_free_with".into(), json!(verdict_str(whole.deadlock_free)));
                let intro_dead = whole.deadlock_free == Verdict::False && s0.deadlock_free == Verdict::True;
                if intro_dead {
                    // a part that finishes on its own (a saga) ends the composite on purpose
                    let finishing: Vec<&str> = [&l.from.subnet, &l.to.subnet]
                        .into_iter()
                        .filter(|id| terminating.get(id.as_str()).copied().unwrap_or(false))
                        .map(String::as_str)
                        .collect();
                    if finishing.is_empty() {
                        bump(&mut level, 3);
                        findings.push(json!({"code": "introduces_deadlock", "says": "the composite can reach a dead marking and, without this link, it cannot; neither joined part ends on its own, so this is a stuck state; the witness is in the deadlock claim"}));
                    } else {
                        bump(&mut level, 1);
                        findings.push(json!({"code": "deadlock_explained", "says": format!("the composite can reach a dead marking and, without this link, it cannot; {} reaches a dead marking on its own (it finishes), so the composite finishes with it", finishing.join(" and "))}));
                    }
                }
                let heals = whole.deadlock_free == Verdict::True && s0.deadlock_free == Verdict::False;
                if heals {
                    findings.push(json!({"code": "removes_deadlock", "says": "without this link the composite can deadlock; with it, it cannot"}));
                }
                e.insert("introduces_deadlock".into(), json!(intro_dead));
                let intro_unb = whole.bounded == Verdict3::False && s0.bounded == Verdict3::True;
                e.insert("introduces_unboundedness".into(), json!(intro_unb));
                if intro_unb {
                    bump(&mut level, 3);
                    findings.push(json!({"code": "introduces_unboundedness", "says": "the composite is unbounded and, without this link, it is bounded"}));
                }
                // members that fire without the link and never fire with it
                let fired_members = |gg: &Flat, fired: &BTreeSet<String>| -> BTreeSet<String> {
                    let mut out = BTreeSet::new();
                    for (s, locals) in &gg.map.transition {
                        for (local, flat) in locals {
                            if fired.contains(flat) {
                                out.insert(format!("{s}/{local}"));
                            }
                        }
                    }
                    out
                };
                let before = fired_members(&g, &s0.fired);
                let after = fired_members(&f, &whole.fired);
                let blocked: Vec<&String> = before.difference(&after).collect();
                e.insert("blocked_members".into(), json!(blocked));
                if !blocked.is_empty() {
                    if whole.complete {
                        bump(&mut level, 2);
                        findings.push(json!({"code": "blocks_transitions", "says": format!("with this link these can no longer fire in any reachable marking: {}", blocked.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", "))}));
                    } else {
                        findings.push(json!({"code": "blocks_transitions_unknown", "says": "some transitions fired without this link and not within the explored states with it; the search stopped before it could tell"}));
                    }
                }
                effect = Value::Object(e);
            }
        }
        let verdict = ["ok", "explained", "warn", "fail"][level as usize];
        let says = if findings.is_empty() {
            format!(
                "{} link {} → {}: no behavioural finding{}",
                l.kind.name(),
                l.from,
                l.to,
                if ablate { "" } else { " (the link was not tried against the composite: ablation off or too many links)" }
            )
        } else {
            format!("{} link {} → {}: {}", l.kind.name(), l.from, l.to, verdict)
        };
        link_views.push(json!({
            "index": i,
            "key": rules::link_key(l),
            "kind": l.kind.name(),
            "from": rules::end_view(&l.from),
            "to": rules::end_view(&l.to),
            "structural": sv.view(),
            "glue": glue,
            "laws_touched": laws_touched,
            "effect": effect,
            "findings": findings,
            "verdict": verdict,
            "says": says,
        }));
    }

    // ---- overall
    let count = |v: &str| link_views.iter().filter(|x| x["verdict"] == v).count();
    let refuted_claim = claims.iter().any(|c| c["status"] == "refuted" && c["id"] != "deadlock_free" && c["id"] != "live");
    let any_fail = count("fail") > 0 || refuted_claim;
    let unknowns = claims.iter().filter(|c| c["status"] == "unknown").count()
        + law_views.iter().filter(|l| l["holds_on_reachable"] == "unknown").count();
    let status = if any_fail {
        "inconsistent"
    } else if unknowns > 0 && !whole.complete {
        "inconclusive"
    } else if count("warn") > 0 {
        "needs_attention"
    } else if count("explained") > 0 {
        "consistent_with_notes"
    } else {
        "consistent"
    };
    let mut says: Vec<String> = vec![];
    says.push(format!(
        "{} subnets, {} links, flattened to {} places and {} transitions; {} states explored{}.",
        b.subnets.len(),
        b.links.len(),
        m.places.len(),
        m.transitions.len(),
        whole.states,
        if whole.complete { " (the search finished)" } else { " (the search stopped at the bound, so nothing below is claimed for unexplored states)" }
    ));
    for cl in &claims {
        says.push(cl["says"].as_str().unwrap_or("").to_string());
    }
    for lv in &link_views {
        if lv["verdict"] != "ok" {
            says.push(lv["says"].as_str().unwrap_or("").to_string());
            for fd in lv["findings"].as_array().into_iter().flatten() {
                says.push(format!("  - {}", fd["says"].as_str().unwrap_or("")));
            }
        }
    }

    Ok(json!({
        "type": RECORD_TYPE,
        "version": RECORD_VERSION,
        "options": o.view(),
        "bundle": {"name": b.name, "subnets": order, "links": b.links.len()},
        "composite": {
            "cid": composite_cid,
            "places": m.places.len(),
            "transitions": m.transitions.len(),
            "arcs": m.arcs.len(),
            "net_available": net.is_some(),
        },
        "search": {
            "states": whole.states,
            "complete": whole.complete,
            "dead_markings": whole.dead_count,
            "max_tokens": whole.max_tokens,
        },
        "laws": law_views,
        "claims": claims,
        "links": link_views,
        "verdict": {
            "status": status,
            "links": {"ok": count("ok"), "explained": count("explained"), "warn": count("warn"), "fail": count("fail")},
            "claims": {
                "proved": claims.iter().filter(|c| c["status"] == "proved").count(),
                "refuted": claims.iter().filter(|c| c["status"] == "refuted").count(),
                "unknown": claims.iter().filter(|c| c["status"] == "unknown").count(),
            },
        },
        "says": says,
        "check": {"rules": CHECK_RULES, "tool": "pflow_bundle_check_proof"},
    }))
}

/// A shortest trace from the initial marking to `target`, by the analyzer.
fn trace_to(m: &Model, o: &Opts, target: &Marking) -> Value {
    let a = Analyzer::new(m).with_max_states(o.max_states).with_max_tokens(o.max_tokens);
    match a.path_to(target) {
        Some(t) => json!(t),
        None => Value::Null,
    }
}

fn claim(id: &str, status: &str, method: &str, says: String, certificate: Value) -> Value {
    json!({"id": id, "status": status, "method": method, "says": says, "certificate": certificate})
}

fn bounded_claim(
    m: &Model,
    c: &[Vec<i64>],
    pidx: &HashMap<&str, usize>,
    basis: &[BTreeMap<String, i64>],
    truncated: bool,
    s: &Search,
) -> Value {
    // z = the sum of the minimal invariants: positive exactly where some invariant covers the place
    let mut z: BTreeMap<String, i64> = BTreeMap::new();
    for v in basis {
        for (p, k) in v {
            *z.entry(p.clone()).or_insert(0) += k;
        }
    }
    debug_assert!(residual(m, c, &z, pidx).is_empty());
    let z0: i64 = z.iter().map(|(p, k)| k * m.place_by_id(p).map_or(0, |q| q.initial)).sum();
    let mut by: BTreeMap<String, Value> = BTreeMap::new();
    let mut open: Vec<String> = vec![];
    for p in &m.places {
        if let Some(&k) = z.get(&p.id) {
            if k > 0 {
                by.insert(p.id.clone(), json!({"by": "invariant", "bound": z0 / k}));
                continue;
            }
        }
        if p.capacity > 0 {
            by.insert(p.id.clone(), json!({"by": "capacity", "bound": p.capacity}));
        } else {
            open.push(p.id.clone());
        }
    }
    let cert = json!({"z": z, "z_value": z0, "places": by, "uncovered": open, "basis_truncated": truncated});
    if open.is_empty() {
        let n = m.places.len();
        return claim(
            "bounded",
            "proved",
            "invariant cover and capacities",
            format!("Bounded: every one of the {n} places is held by a conservation law or a capacity, so no place can grow without limit."),
            cert,
        );
    }
    match s.bounded {
        Verdict3::True => claim(
            "bounded",
            "proved",
            "exhaustive search",
            format!("Bounded: no conservation law covers {} (e.g. {}), but the whole state space was explored and it is finite.", plural(open.len(), "place"), open[0]),
            json!({"search": {"states": s.states, "complete": true}, "uncovered": open, "z": cert["z"], "z_value": z0, "places": cert["places"], "basis_truncated": truncated}),
        ),
        Verdict3::False => claim(
            "bounded",
            "refuted",
            "exhaustive search",
            format!("Not bounded: the search found tokens growing past the cap in {}.", open.join(", ")),
            json!({"search": {"states": s.states, "complete": s.complete}, "uncovered": open}),
        ),
        Verdict3::Unknown => claim(
            "bounded",
            "unknown",
            "search stopped",
            format!("Boundedness unknown: no conservation law or capacity holds {} (e.g. {}), and the search stopped before it finished.", plural(open.len(), "place"), open[0]),
            json!({"search": {"states": s.states, "complete": false}, "uncovered": open, "basis_truncated": truncated}),
        ),
    }
}

fn plural(n: usize, w: &str) -> String {
    if n == 1 {
        format!("1 {w}")
    } else {
        format!("{n} {w}s")
    }
}

fn deadlock_claim(s: &Search) -> Value {
    match s.deadlock_free {
        Verdict::True => claim(
            "deadlock_free",
            "proved",
            "exhaustive search",
            format!("Deadlock-free: all {} reachable markings were explored and each has an enabled transition.", s.states),
            json!({"search": {"states": s.states, "complete": true}}),
        ),
        Verdict::False => {
            let (trace, mk) = s.dead.clone().unwrap_or_default();
            claim(
                "deadlock_free",
                "refuted",
                "witness",
                format!(
                    "Can reach a dead marking (nothing enabled) after {} firings: {}. Workflows that finish end like this on purpose; the per-link verdicts say which dead markings a link introduces.",
                    trace.len(),
                    if trace.is_empty() { "the initial marking".to_string() } else { trace.join(" → ") }
                ),
                json!({"trace": trace, "marking": mk_view(&mk), "dead_markings": s.dead_count, "search": {"states": s.states, "complete": s.complete}}),
            )
        }
        Verdict::Unknown => claim(
            "deadlock_free",
            "unknown",
            "search stopped",
            format!("Deadlock-freedom unknown: {} markings explored, no dead one among them, and the search stopped before it finished.", s.states),
            json!({"search": {"states": s.states, "complete": false}}),
        ),
    }
}

fn live_claim(s: &Search) -> Value {
    match s.quasi_live {
        Verdict::True => claim(
            "quasi_live",
            "proved",
            "exhaustive search",
            "Every transition can fire in some reachable marking.".to_string(),
            json!({"search": {"states": s.states, "complete": s.complete}}),
        ),
        Verdict::False => claim(
            "quasi_live",
            "refuted",
            "exhaustive search",
            format!("These transitions can never fire in the composite: {}.", s.never_fires.join(", ")),
            json!({"never_fires": s.never_fires, "search": {"states": s.states, "complete": s.complete}}),
        ),
        Verdict::Unknown => claim(
            "quasi_live",
            "unknown",
            "search stopped",
            "Whether every transition can fire is unknown: the search stopped before it finished.".to_string(),
            json!({"search": {"states": s.states, "complete": false}}),
        ),
    }
}

// ------------------------------------------------------------ check

/// Verifies a record against the bundle it claims to be about. Two layers:
/// every certificate is checked by arithmetic or replay on the flat net, and
/// the engine's own answers are recomputed and compared.
pub fn check(b: &Bundle, rec: &Value, tool: &str) -> Result<Value, String> {
    if rec["type"] != RECORD_TYPE {
        return Err(format!("{tool}: proof: not a {RECORD_TYPE} record"));
    }
    let opts = Opts {
        max_states: rec["options"]["max_states"].as_u64().unwrap_or(DEFAULT_STATES as u64).clamp(1, MAX_STATES as u64) as usize,
        max_tokens: rec["options"]["max_tokens"].as_i64().unwrap_or(DEFAULT_TOKENS).max(1),
        ablate: rec["options"]["ablate"].as_bool().unwrap_or(true),
    };
    let f = rules::flatten(b).map_err(|e| format!("{tool}: {e}"))?;
    let m = &f.model;
    let c = rules::incidence(m);
    let pidx: HashMap<&str, usize> = m.places.iter().enumerate().map(|(i, p)| (p.id.as_str(), i)).collect();
    let mut results: Vec<Value> = vec![];
    let mut ok = true;
    let mut rec_check = |what: String, method: &str, pass: bool, detail: String| {
        if !pass {
            ok = false;
        }
        results.push(json!({"what": what, "method": method, "ok": pass, "detail": detail}));
    };

    // the composite is the one the record is about
    let order: Vec<String> = b.subnets.iter().map(|s| s.id.clone()).collect();
    let cid = flatnet::editor_net(&b.name, &f, &order)
        .ok()
        .and_then(|(n, _)| crate::cid::run(&json!({"net": n})).ok())
        .and_then(|v| v["cid"].as_str().map(String::from));
    let claimed = rec["composite"]["cid"].as_str().map(String::from);
    rec_check(
        "composite net".into(),
        "recompute",
        cid == claimed && rec["composite"]["places"] == json!(m.places.len()) && rec["composite"]["transitions"] == json!(m.transitions.len()),
        format!("cid {} vs record {}", cid.as_deref().unwrap_or("-"), claimed.as_deref().unwrap_or("-")),
    );

    let m0 = m.initial_marking();
    let vec_of = |v: &Value| -> BTreeMap<String, i64> {
        v.as_object().map(|o| o.iter().filter_map(|(k, x)| x.as_i64().map(|n| (k.clone(), n))).collect()).unwrap_or_default()
    };
    // laws
    for l in rec["laws"].as_array().into_iter().flatten() {
        let name = format!("law {} / {}", l["part"].as_str().unwrap_or("?"), l["id"].as_str().unwrap_or("?"));
        let yhat = vec_of(&l["lifted"]);
        let res = residual(m, &c, &yhat, &pidx);
        let claimed_res = vec_of(&l["residual"]);
        match l["status"].as_str().unwrap_or("") {
            "preserved" => rec_check(name.clone(), "arithmetic", res.is_empty() && claimed_res.is_empty(), "y·C = 0 for every transition".into()),
            "changed" | "extended" => {
                let mut pass = res == claimed_res && !res.is_empty();
                let mut detail = format!("residual on {} transitions", res.len());
                if l["status"] == "extended" {
                    let z = vec_of(&l["extended_by"]["z"]);
                    let zr = residual(m, &c, &z, &pidx);
                    let nonneg = z.values().all(|k| *k >= 0);
                    let ratio = (l["extended_by"]["ratio"][0].as_i64().unwrap_or(0), l["extended_by"]["ratio"][1].as_i64().unwrap_or(1));
                    let part = l["part"].as_str().unwrap_or("");
                    let places: BTreeSet<&String> = f.map.place.get(part).map(|mm| mm.values().collect()).unwrap_or_default();
                    let prop = places.iter().all(|p| {
                        z.get(*p).copied().unwrap_or(0) * ratio.1 == yhat.get(*p).copied().unwrap_or(0) * ratio.0
                    });
                    pass = pass && zr.is_empty() && nonneg && prop && ratio.0 != 0;
                    detail = "z·C = 0, z ≥ 0 and z is a fixed multiple of the law on the part's places".into();
                }
                rec_check(name.clone(), "arithmetic", pass, detail)
            }
            other => rec_check(name.clone(), "arithmetic", false, format!("unknown status {other:?}")),
        }
        // the value the law holds at the start
        if l["declared"] == true {
            let y0: i64 = yhat.iter().map(|(p, k)| k * m0.get(p).copied().unwrap_or(0)).sum();
            let stated = l["value"].as_i64().unwrap_or(i64::MIN);
            if l["holds_on_reachable"] != "refuted" {
                rec_check(format!("{name} (initial value)"), "arithmetic", y0 == stated, format!("y·M0 = {y0}, declared {stated}"));
            }
        }
        if l["holds_on_reachable"] == "refuted" {
            let (tr, mk) = (&l["counterexample"]["trace"], &l["counterexample"]["marking"]);
            let end = replay(m, tr);
            let yv: i64 = end.as_ref().map(|e| yhat.iter().map(|(p, k)| k * e.get(p).copied().unwrap_or(0)).sum()).unwrap_or(0);
            let want = vec_of(mk);
            let same = end.as_ref().is_some_and(|e| nonzero(e) == want);
            rec_check(format!("{name} counterexample"), "replay", same && yv != l["value"].as_i64().unwrap_or(yv), "the trace replays to a marking where the law's sum differs".into());
        }
    }
    // claims
    for cl in rec["claims"].as_array().into_iter().flatten() {
        let id = cl["id"].as_str().unwrap_or("?");
        let cert = &cl["certificate"];
        let name = format!("claim {id}");
        match (id, cl["status"].as_str().unwrap_or("")) {
            ("bounded", "proved") => {
                let z = vec_of(&cert["z"]);
                let zr = residual(m, &c, &z, &pidx);
                let z0: i64 = z.iter().map(|(p, k)| k * m0.get(p).copied().unwrap_or(0)).sum();
                let mut pass = zr.is_empty() && z.values().all(|k| *k >= 0) && cert["z_value"] == json!(z0);
                let mut exhaustive = false;
                for p in &m.places {
                    let e = &cert["places"][&p.id];
                    let good = match e["by"].as_str() {
                        Some("invariant") => z.get(&p.id).copied().unwrap_or(0) > 0 && e["bound"] == json!(z0 / z[&p.id]),
                        Some("capacity") => p.capacity > 0 && e["bound"] == json!(p.capacity),
                        _ => false,
                    };
                    if !good {
                        exhaustive = true;
                    }
                }
                if exhaustive {
                    // some place has no certificate: the claim rests on a finished search
                    let s = search(m, &opts);
                    pass = pass && s.bounded == Verdict3::True;
                    rec_check(name.clone(), "arithmetic + recompute", pass, "z·C = 0, z ≥ 0; the uncovered places rest on a finished search, re-run".into());
                } else {
                    rec_check(name.clone(), "arithmetic", pass, "z·C = 0, z ≥ 0, every place has a bound from z or a capacity".into());
                }
            }
            ("deadlock_free", "refuted") => {
                let end = replay(m, &cert["trace"]);
                let want = vec_of(&cert["marking"]);
                let dead = end.as_ref().is_some_and(|e| m.transitions.iter().all(|t| !m.enabled(&t.id, e)));
                rec_check(name.clone(), "replay", dead && end.as_ref().is_some_and(|e| nonzero(e) == want), "the trace replays to the stated marking, where nothing is enabled".into());
            }
            _ => {
                // proved/unknown by search, bounded refuted/unknown, live: recompute and compare
                let s = search(m, &opts);
                let now = match id {
                    "bounded" => match s.bounded {
                        Verdict3::True => "proved",
                        Verdict3::False => "refuted",
                        Verdict3::Unknown => "unknown",
                    },
                    "deadlock_free" => verdict_to_status(s.deadlock_free),
                    "quasi_live" => verdict_to_status(s.quasi_live),
                    _ => "?",
                };
                // a proved boundedness by invariants alone would have taken the first arm
                rec_check(name.clone(), "recompute", now == cl["status"], format!("search again with the same bounds: {now}"));
            }
        }
    }
    // links: the verdicts are recomputed; structural verdicts and findings must match
    let again = prove(b, &opts, tool)?;
    let same_links = again["links"] == rec["links"];
    rec_check("link verdicts".into(), "recompute", same_links, format!("{} links re-judged", b.links.len()));
    rec_check("overall verdict".into(), "recompute", again["verdict"] == rec["verdict"], format!("{}", again["verdict"]["status"]));

    let failed: Vec<&Value> = results.iter().filter(|r| r["ok"] == false).collect();
    Ok(json!({
        "valid": ok,
        "checked": results.len(),
        "failed": failed,
        "checks": results,
        "trusts": "flattening (pflow-compose, held to go-pflow's goldens) and the firing rule; every certificate above is plain arithmetic or replay on the flat net",
    }))
}

fn verdict_to_status(v: Verdict) -> &'static str {
    match v {
        Verdict::True => "proved",
        Verdict::False => "refuted",
        Verdict::Unknown => "unknown",
    }
}

fn nonzero(mk: &Marking) -> BTreeMap<String, i64> {
    mk.iter().filter(|(_, v)| **v != 0).map(|(k, v)| (k.clone(), *v)).collect()
}

/// Fires a trace from the initial marking; `None` at the first step that is not enabled.
fn replay(m: &Model, trace: &Value) -> Option<Marking> {
    let mut mk = m.initial_marking();
    for t in trace.as_array()? {
        let t = t.as_str()?;
        m.enabled_why_not(t, &mk).ok()?;
        mk = m.fire(t, &mk);
    }
    Some(mk)
}
