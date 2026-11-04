//! `bundle_parts`: a larger parts catalog than `bundle_example`'s six.
//!
//! `bundle_example` is held to pflow-native's goldens and lists only its six
//! instances; this lists every part this port can build, with its parameters,
//! its ports and the conservation law it carries, and builds a composition
//! from any of them. The three patterns of `bundle_example` (`queue`,
//! `resource-pool`, `saga`) are here too, built by the same constructors.
//!
//! The ten new parts are textbook nets (a lock, a token bucket, a circuit
//! breaker, a batcher, an approval desk, a stock with reordering, machines
//! that fail and are repaired, SIR, a WIP-limited stage, a traffic light),
//! written for this catalog from public descriptions, not copied from any
//! model: the catalog's judgment (sizes, which place is a port, which schema
//! a port speaks) is this module's, and `why` says what each is for. The
//! schemas are what lets a token link join two parts: `resource` joins a
//! pool's `available` to a lock's `free`; `queue:items` joins a queue to a
//! batcher or a stage.

use pflow_compose::{
    new_queue, new_resource_pool, Bundle, NetType, Port, PortKind, PortTarget, QueueSpec, Saga,
    Subnet,
};
use pflow_metamodel::{Arc as MArc, ArcType, Constraint, Model, Place, StateKind, Transition};
use serde_json::{json, Map, Value};

use super::rules::{link_view, parse_link, verdict_in};
use super::text::{quote, quote_list};

/// One tunable of a part.
struct Param {
    name: &'static str,
    default: i64,
    min: i64,
    max: i64,
    about: &'static str,
}

/// A part of the catalog.
pub struct Part {
    pub id: &'static str,
    pub title: &'static str,
    /// Where the idea comes from.
    pub origin: &'static str,
    pub why: &'static str,
    params: Vec<Param>,
    /// Suggested joins: (this port, other part, other port or "" for any, kind).
    pub joins: &'static [(&'static str, &'static str, &'static str, &'static str)],
}

#[allow(non_snake_case)]
fn P(name: &'static str, default: i64, min: i64, max: i64, about: &'static str) -> Param {
    Param { name, default, min, max, about }
}

fn parts() -> Vec<Part> {
    vec![
        Part {
            id: "queue",
            title: "Bounded queue",
            origin: "pflow-compose pattern (go-pflow `metamodel.Queue`)",
            why: "Work waiting to be taken; capacity makes the slots a conserved resource.",
            params: vec![
                P("capacity", 3, 1, 50, "slots"),
                P("initial", 0, 0, 50, "items waiting at the start"),
            ],
            joins: &[("out", "kanban", "ready", "token"), ("out", "batcher", "in", "token"), ("out", "approval", "requests", "token"), ("dequeue", "mutex", "lock", "event"), ("dequeue", "token_bucket", "admit", "event"), ("dequeue", "circuit_breaker", "call", "event"), ("dequeue", "resource-pool", "acquire", "event")],
        },
        Part {
            id: "resource-pool",
            title: "Resource pool",
            origin: "pflow-compose pattern (go-pflow `metamodel.ResourcePool`)",
            why: "A fixed number of interchangeable workers, machines or seats.",
            params: vec![P("capacity", 2, 1, 50, "units")],
            joins: &[("available", "failover", "up", "token"), ("acquire", "queue", "dequeue", "event")],
        },
        Part {
            id: "saga",
            title: "Two-step saga",
            origin: "pflow-compose pattern (go-pflow `saga`)",
            why: "A reserve-then-charge transaction with compensation.",
            params: vec![P("customer", 1, 0, 50, "tokens in the customer account")],
            joins: &[("do:reserve", "queue", "dequeue", "event"), ("commit", "resource-pool", "release", "event")],
        },
        Part {
            id: "mutex",
            title: "Lock",
            origin: "textbook mutual exclusion",
            why: "One holder at a time. The `held` port is what a guard link reads to keep another part out.",
            params: vec![],
            joins: &[("free", "resource-pool", "available", "token")],
        },
        Part {
            id: "token_bucket",
            title: "Token bucket",
            origin: "textbook rate limiter",
            why: "At most `capacity` calls in a burst, refilled one at a time. Bounded by its capacity, not by a conservation law: a source and a sink.",
            params: vec![P("capacity", 3, 1, 50, "burst size"), P("initial", 3, 0, 50, "tokens at the start")],
            joins: &[("admit", "queue", "dequeue", "event"), ("tokens", "resource-pool", "available", "token")],
        },
        Part {
            id: "circuit_breaker",
            title: "Circuit breaker",
            origin: "textbook closed / open / half-open breaker",
            why: "Calls pass only while closed; trips on failure, probes, then recovers or reopens. Exactly one of the three states is marked.",
            params: vec![],
            joins: &[("call", "queue", "dequeue", "event")],
        },
        Part {
            id: "batcher",
            title: "Batcher",
            origin: "textbook aggregator",
            why: "Collects `size` items and emits one batch. Joined to a queue it consumes the queue's items, which changes the queue's own law (a worked 'explained' case).",
            params: vec![P("size", 2, 1, 20, "items per batch"), P("capacity", 4, 1, 50, "buffer slots")],
            joins: &[("in", "queue", "out", "token"), ("batches", "kanban", "ready", "token")],
        },
        Part {
            id: "approval",
            title: "Approval desk",
            origin: "textbook decision step",
            why: "Each request is approved or rejected, never both: requests + approved + rejected is conserved.",
            params: vec![P("requests", 2, 0, 50, "requests waiting at the start")],
            joins: &[("requests", "queue", "out", "token"), ("approved", "queue", "in", "token")],
        },
        Part {
            id: "inventory",
            title: "Stock with reordering",
            origin: "textbook inventory control",
            why: "On hand and on order are both capped by the stock level; consumption is a sink, ordering a source. Bounded by capacity alone.",
            params: vec![P("stock", 3, 1, 50, "stock level (the cap on hand and on order)")],
            joins: &[("consume", "queue", "dequeue", "event")],
        },
        Part {
            id: "failover",
            title: "Machines that fail",
            origin: "textbook availability model (two-state repairable machines)",
            why: "Of `machines`, some are up and some down: up + down is conserved. Joined to a pool's `available` it makes the pool's size a moving target, which breaks the pool's own law.",
            params: vec![P("machines", 2, 1, 50, "machines in service or in repair")],
            joins: &[("up", "resource-pool", "available", "token")],
        },
        Part {
            id: "sir",
            title: "SIR epidemic",
            origin: "Kermack-McKendrick compartment model (public domain)",
            why: "Susceptible, infected, recovered: S + I + R = N. Infection needs one of each of S and I.",
            params: vec![P("population", 6, 2, 100, "N"), P("infected", 1, 1, 50, "infected at the start")],
            joins: &[],
        },
        Part {
            id: "kanban",
            title: "WIP-limited stage",
            origin: "kanban / work-in-progress limit",
            why: "Work moves ready, doing, done; `doing` is capped so the stage never holds more than `wip`.",
            params: vec![P("wip", 2, 1, 20, "most items in progress"), P("ready", 2, 0, 50, "items ready at the start")],
            joins: &[("ready", "queue", "out", "token"), ("done", "queue", "in", "token")],
        },
        Part {
            id: "traffic_light",
            title: "Traffic light",
            origin: "textbook cyclic controller",
            why: "Red, green, yellow in a ring; one is marked. A guard link from `green` lets a queue's dequeue through only on green.",
            params: vec![],
            joins: &[("green", "queue", "dequeue", "guard")],
        },
    ]
}

pub fn ids() -> Vec<&'static str> {
    parts().iter().map(|p| p.id).collect()
}

// ------------------------------------------------------------ building

struct Net {
    id: String,
    net_type: NetType,
    places: Vec<Place>,
    transitions: Vec<Transition>,
    arcs: Vec<MArc>,
    constraints: Vec<Constraint>,
    ports: Vec<Port>,
}

fn n(id: &str, t: NetType) -> Net {
    Net { id: id.into(), net_type: t, places: vec![], transitions: vec![], arcs: vec![], constraints: vec![], ports: vec![] }
}

impl Net {
    /// A token place; `schema` non-empty makes it a port of kind `kind`.
    fn place(mut self, id: &str, initial: i64, capacity: i64, port: Option<(PortKind, &str)>) -> Self {
        self.places.push(Place {
            id: id.into(),
            kind: Some(StateKind::Token),
            initial,
            capacity,
            exported: port.is_some(),
            typ: port.map(|(_, s)| s.to_string()).unwrap_or_default(),
            ..Default::default()
        });
        if let Some((k, s)) = port {
            self.ports.push(Port { id: id.into(), kind: Some(k), place: id.into(), schema: s.into(), ..Default::default() });
        }
        self
    }

    fn transition(mut self, id: &str, port: Option<PortKind>) -> Self {
        self.transitions.push(Transition { id: id.into(), ..Default::default() });
        if let Some(k) = port {
            self.ports.push(Port { id: id.into(), kind: Some(k), target: Some(PortTarget::Transition), transition: id.into(), ..Default::default() });
        }
        self
    }

    fn arc(mut self, from: &str, to: &str, weight: i64) -> Self {
        self.arcs.push(MArc { from: from.into(), to: to.into(), weight, ..Default::default() });
        self
    }

    /// A read arc: the transition needs `place` marked and does not move it.
    fn read(mut self, place: &str, to: &str) -> Self {
        self.arcs.push(MArc { from: place.into(), to: to.into(), weight: 1, typ: ArcType::Read, ..Default::default() });
        self
    }

    fn sum(mut self, id: &str, places: &[&str], total: i64) -> Self {
        let lhs: Vec<String> = places.iter().map(|p| format!("tokens(\"{p}\")")).collect();
        self.constraints.push(Constraint { id: id.into(), expr: format!("{} == {total}", lhs.join(" + ")) });
        self
    }

    fn done(self) -> Subnet {
        Subnet {
            typ: pflow_compose::bundle::SUBNET_TYPE.to_string(),
            id: self.id.clone(),
            net_type: self.net_type,
            model: Model {
                name: self.id,
                places: self.places,
                transitions: self.transitions,
                arcs: self.arcs,
                constraints: self.constraints,
                ..Default::default()
            },
            ports: self.ports,
        }
    }
}

use PortKind::{In, Inout, Observe, Out};

fn get(spec: &Part, params: &Map<String, Value>, name: &str) -> i64 {
    let p = spec.params.iter().find(|p| p.name == name).expect("declared param");
    params.get(name).and_then(Value::as_i64).unwrap_or(p.default)
}

/// Checks `params` against the part's declared ones.
fn check_params(spec: &Part, params: &Map<String, Value>) -> Result<(), String> {
    for k in params.keys() {
        if !spec.params.iter().any(|p| p.name == k) {
            let names: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
            return Err(format!(
                "part {} has no parameter {}; it takes {}",
                spec.id,
                quote(k),
                if names.is_empty() { "none".to_string() } else { quote_list(&names) }
            ));
        }
    }
    for p in &spec.params {
        if let Some(v) = params.get(p.name) {
            match v.as_i64() {
                Some(x) if (p.min..=p.max).contains(&x) => {}
                _ => return Err(format!("part {}: {} should be an integer in [{}, {}], got {v}", spec.id, p.name, p.min, p.max)),
            }
        }
    }
    Ok(())
}

pub fn build(spec: &Part, id: &str, params: &Map<String, Value>) -> Result<Subnet, String> {
    check_params(spec, params)?;
    let g = |k: &str| get(spec, params, k);
    let rs = "resource";
    Ok(match spec.id {
        "queue" => {
            let (capacity, initial) = (g("capacity"), g("initial"));
            if initial > capacity {
                return Err(format!("part queue: initial {initial} is more than capacity {capacity}"));
            }
            new_queue(QueueSpec { id: id.into(), capacity, initial, description: String::new() })?
        }
        "resource-pool" => new_resource_pool(id, g("capacity")),
        "saga" => {
            let mut b = Saga::builder(id);
            b = b.account("customer", g("customer")).account("merchant", 0);
            b = b.step("reserve").name("reserve stock").compensate("release stock").done();
            b = b.step("charge").name("charge card").compensate("refund").move_tokens("customer", "merchant", 1).done();
            b.build_validated()?.to_meta_subnet()
        }
        "mutex" => n(id, NetType::Resource)
            .place("free", 1, 1, Some((Inout, rs)))
            .place("held", 0, 1, Some((Observe, rs)))
            .transition("lock", Some(In))
            .transition("unlock", Some(Out))
            .arc("free", "lock", 1).arc("lock", "held", 1).arc("held", "unlock", 1).arc("unlock", "free", 1)
            .sum("mutual_exclusion", &["free", "held"], 1)
            .done(),
        "token_bucket" => {
            let (cap, init) = (g("capacity"), g("initial"));
            if init > cap {
                return Err(format!("part token_bucket: initial {init} is more than capacity {cap}"));
            }
            n(id, NetType::Resource)
                .place("tokens", init, cap, Some((Inout, rs)))
                .transition("refill", Some(In))
                .transition("admit", Some(In))
                .arc("refill", "tokens", 1).arc("tokens", "admit", 1)
                .done()
        }
        "circuit_breaker" => n(id, NetType::Untyped)
            .place("closed", 1, 1, Some((Observe, "breaker")))
            .place("open", 0, 1, Some((Observe, "breaker")))
            .place("half_open", 0, 1, None)
            .transition("call", Some(In))
            .transition("trip", Some(In))
            .transition("probe", None)
            .transition("recover", None)
            .transition("reopen", None)
            .read("closed", "call")
            .arc("closed", "trip", 1).arc("trip", "open", 1)
            .arc("open", "probe", 1).arc("probe", "half_open", 1)
            .arc("half_open", "recover", 1).arc("recover", "closed", 1)
            .arc("half_open", "reopen", 1).arc("reopen", "open", 1)
            .sum("one_state", &["closed", "open", "half_open"], 1)
            .done(),
        "batcher" => {
            let (size, cap) = (g("size"), g("capacity"));
            if cap < size {
                return Err(format!("part batcher: capacity {cap} is less than size {size}, so a batch could never form"));
            }
            n(id, NetType::Untyped)
                .place("in", 0, cap, Some((In, "queue:items")))
                .place("batches", 0, 0, Some((Out, "queue:items")))
                .transition("flush", Some(Out))
                .arc("in", "flush", size).arc("flush", "batches", 1)
                .done()
        }
        "approval" => {
            let r = g("requests");
            n(id, NetType::Resource)
                .place("requests", r, 0, Some((In, "queue:items")))
                .place("approved", 0, 0, Some((Out, "queue:items")))
                .place("rejected", 0, 0, Some((Out, "queue:items")))
                .transition("approve", Some(Out))
                .transition("reject", Some(Out))
                .arc("requests", "approve", 1).arc("approve", "approved", 1)
                .arc("requests", "reject", 1).arc("reject", "rejected", 1)
                .sum("decided_once", &["requests", "approved", "rejected"], r)
                .done()
        }
        "inventory" => {
            let s = g("stock");
            n(id, NetType::Resource)
                .place("on_hand", s, s, Some((Inout, "stock")))
                .place("on_order", 0, s, None)
                .transition("consume", Some(In))
                .transition("order", None)
                .transition("arrive", Some(Out))
                .arc("on_hand", "consume", 1)
                .arc("order", "on_order", 1)
                .arc("on_order", "arrive", 1).arc("arrive", "on_hand", 1)
                .done()
        }
        "failover" => {
            let m = g("machines");
            n(id, NetType::Resource)
                .place("up", m, m, Some((Inout, rs)))
                .place("down", 0, m, Some((Observe, rs)))
                .transition("fail", Some(Out))
                .transition("repair", Some(In))
                .arc("up", "fail", 1).arc("fail", "down", 1)
                .arc("down", "repair", 1).arc("repair", "up", 1)
                .sum("machines_conserved", &["up", "down"], m)
                .done()
        }
        "sir" => {
            let (pop, inf) = (g("population"), g("infected"));
            if inf >= pop {
                return Err(format!("part sir: infected {inf} must be less than population {pop}"));
            }
            n(id, NetType::Untyped)
                .place("susceptible", pop - inf, 0, Some((Observe, "population")))
                .place("infected", inf, 0, Some((Observe, "population")))
                .place("recovered", 0, 0, Some((Out, "population")))
                .transition("infect", None)
                .transition("recover", Some(Out))
                .arc("susceptible", "infect", 1).arc("infected", "infect", 1).arc("infect", "infected", 2)
                .arc("infected", "recover", 1).arc("recover", "recovered", 1)
                .sum("population_conserved", &["susceptible", "infected", "recovered"], pop)
                .done()
        }
        "kanban" => {
            let (wip, ready) = (g("wip"), g("ready"));
            n(id, NetType::Resource)
                .place("ready", ready, 0, Some((In, "queue:items")))
                .place("doing", 0, wip, Some((Observe, "queue:items")))
                .place("done", 0, 0, Some((Out, "queue:items")))
                .transition("start", Some(In))
                .transition("finish", Some(Out))
                .arc("ready", "start", 1).arc("start", "doing", 1)
                .arc("doing", "finish", 1).arc("finish", "done", 1)
                .sum("work_conserved", &["ready", "doing", "done"], ready)
                .done()
        }
        "traffic_light" => n(id, NetType::Untyped)
            .place("red", 1, 1, Some((Observe, "light")))
            .place("green", 0, 1, Some((Observe, "light")))
            .place("yellow", 0, 1, Some((Observe, "light")))
            .transition("go", None)
            .transition("slow", None)
            .transition("stop", None)
            .arc("red", "go", 1).arc("go", "green", 1)
            .arc("green", "slow", 1).arc("slow", "yellow", 1)
            .arc("yellow", "stop", 1).arc("stop", "red", 1)
            .sum("one_light", &["red", "green", "yellow"], 1)
            .done(),
        other => return Err(format!("no part {other:?}")),
    })
}

// ------------------------------------------------------------ the tool

fn port_view(p: &Port) -> Value {
    let mut v = json!({"id": p.id, "kind": p.kind, "on": if p.is_transition() { "transition" } else { "place" }});
    if !p.schema.is_empty() {
        v["schema"] = json!(p.schema);
    }
    v
}

/// The suggested joins that hold: each tried both ways round against a
/// default instance of the other part, as the composition page would judge it.
fn legal_joins(spec: &Part) -> (Vec<Value>, Vec<String>) {
    let all = parts();
    let mut good = vec![];
    let mut bad = vec![];
    for (port, other, oport, kind) in spec.joins {
        let Some(o) = all.iter().find(|p| p.id == *other) else {
            bad.push(format!("{} ~ {other}.{oport}: no such part", port));
            continue;
        };
        let (Ok(a), Ok(b)) = (build(spec, "a", &Map::new()), build(o, "b", &Map::new())) else {
            continue;
        };
        let mut bundle = Bundle::new("joins");
        bundle.add_subnet(a);
        bundle.add_subnet(b);
        let try_it = |from: (&str, &str), to: (&str, &str)| {
            parse_link(&json!({"kind": kind, "from": {"subnet": from.0, "port": from.1}, "to": {"subnet": to.0, "port": to.1}}))
                .ok()
                .filter(|l| verdict_in(&bundle, l).ok)
        };
        match try_it(("a", port), ("b", oport)).map(|_| "out").or_else(|| try_it(("b", oport), ("a", port)).map(|_| "in")) {
            Some(dir) => good.push(json!({
                "this": port, "with": format!("{other}.{oport}"), "kind": kind,
                "direction": if dir == "out" { "this → other" } else { "other → this" },
            })),
            None => bad.push(format!("{} {port} ~ {other}.{oport} ({kind})", spec.id)),
        }
    }
    (good, bad)
}

pub fn catalog_view() -> Value {
    let list: Vec<Value> = parts()
        .iter()
        .map(|spec| {
            let built = build(spec, spec.id, &Map::new());
            let (ports, laws) = match &built {
                Ok(s) => (
                    s.ports.iter().map(port_view).collect::<Vec<_>>(),
                    s.model.constraints.iter().map(|c| c.expr.clone()).collect::<Vec<_>>(),
                ),
                Err(_) => (vec![], vec![]),
            };
            json!({
                "part": spec.id,
                "title": spec.title,
                "origin": spec.origin,
                "why": spec.why,
                "params": spec.params.iter().map(|p| (p.name.to_string(), json!({"default": p.default, "min": p.min, "max": p.max, "about": p.about}))).collect::<Map<String, Value>>(),
                "ports": ports,
                "laws": laws,
                "joins": legal_joins(spec).0,
                "available": built.is_ok(),
            })
        })
        .collect();
    json!({"parts": list, "note": "The catalog's sizes and ports are this port's judgment; `why` says what each part is for. Build a composition with `instances`."})
}

/// `instances`: `[{id, part, params?}]`, `links`: as `bundle_example`'s.
pub fn open(instances: &[Value], links: &[Value]) -> Result<Value, String> {
    let all = parts();
    let mut bundle = Bundle::new("composition");
    for (i, raw) in instances.iter().enumerate() {
        let Some(m) = raw.as_object() else {
            return Err(format!("instances[{i}] should be {{id, part, params?}}, got {raw}"));
        };
        if let Some(k) = m.keys().find(|k| !matches!(k.as_str(), "id" | "part" | "params")) {
            return Err(format!("instances[{i}]: unknown field {}; takes id, part, params", quote(k)));
        }
        let part = m.get("part").and_then(Value::as_str).unwrap_or("");
        let Some(spec) = all.iter().find(|p| p.id == part) else {
            return Err(format!("instances[{i}]: no part {}; the catalog has {}", quote(part), quote_list(&ids())));
        };
        let id = m.get("id").and_then(Value::as_str).unwrap_or(spec.id);
        if id.is_empty() || id.len() > 128 || id.contains('/') || id.contains(':') {
            return Err(format!("instances[{i}]: id {} should be 1 to 128 bytes with no '/' or ':'", quote(id)));
        }
        if bundle.subnet_by_id(id).is_some() {
            return Err(format!("instances[{i}]: id {} is already in the composition", quote(id)));
        }
        let empty = Map::new();
        let params = m.get("params").and_then(Value::as_object).unwrap_or(&empty);
        let s = build(spec, id, params).map_err(|e| format!("instances[{i}]: {e}"))?;
        bundle.add_subnet(s);
    }
    let mut dropped = vec![];
    for (i, raw) in links.iter().enumerate() {
        let l = parse_link(raw).map_err(|e| format!("links[{i}]: {e}"))?;
        let missing: Vec<&str> = [&l.from.subnet, &l.to.subnet]
            .into_iter()
            .filter(|s| bundle.subnet_by_id(s).is_none())
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            dropped.push(json!({"link": link_view(&l), "reason": format!("not in the composition: {}", missing.join(", "))}));
            continue;
        }
        let v = verdict_in(&bundle, &l);
        if !v.ok {
            return Err(format!("links[{i}]: {}", v.refusal()));
        }
        bundle.add_link(l);
    }
    let mut b = serde_json::to_value(&bundle).unwrap_or_else(|_| json!({}));
    if b.get("subnets").is_none() {
        b["subnets"] = json!([]);
    }
    Ok(json!({"bundle": b, "dropped_links": dropped}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_suggested_join_is_legal() {
        let bad: Vec<String> = parts().iter().flat_map(|p| legal_joins(p).1).collect();
        assert!(bad.is_empty(), "illegal joins: {bad:?}");
    }

    #[test]
    fn every_part_builds_validates_and_keeps_its_declared_laws() {
        for spec in parts() {
            let s = build(&spec, spec.id, &Map::new()).unwrap_or_else(|e| panic!("{}: {e}", spec.id));
            let mut b = Bundle::new("one");
            b.add_subnet(s.clone());
            let v = b.validate();
            assert!(v.valid, "{}: {:?}", spec.id, v.errors);
            // a declared law holds at the start
            for c in &s.model.constraints {
                let (lhs, rhs) = c.expr.rsplit_once(" == ").unwrap();
                let total: i64 = lhs
                    .split(" + ")
                    .map(|t| {
                        let p = t.strip_prefix("tokens(\"").unwrap().strip_suffix("\")").unwrap();
                        s.model.place_by_id(p).unwrap().initial
                    })
                    .sum();
                assert_eq!(total.to_string(), rhs, "{} {}", spec.id, c.id);
            }
        }
    }
}
