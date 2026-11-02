//! The composition catalog behind pflow_bundle_example (pflow-xyz
//! `cmd/webserver/mcp_model_catalog.go`, pflow-native's
//! `composition/mod.rs`): the six pattern instances (orders, workers,
//! payment and the extras payment2, orders2, workers2), the three idioms
//! that link the first three, and the four scenarios that fire the result.
//! The parameters, their reasons and the scenarios are the composition
//! page's judgment, copied; the subnets come from pflow-compose's own
//! constructors, as native builds them.

use pflow_compose::{
    new_queue, new_resource_pool, Bundle, Link, LinkKind, QueueSpec, Saga, Subnet,
};
use serde_json::{json, Value};

use super::rules::{link_view, new_link, port_end, verdict_in, Expect};
use super::text::quote_list;

/// native's PARAMS_NOTE.
pub const PARAMS_NOTE: &str = "Parameters are this page's choice (judgment): the description fixes the patterns, not their sizes.";

/// One instance of a pattern, with the parameters it is built from.
pub struct InstanceSpec {
    pub id: &'static str,
    pub pattern: &'static str,
    pub extra: bool,
    pub params: Value,
    pub why: &'static str,
}

fn saga_params(id: &str, customer: i64, shared: bool) -> Value {
    json!({
        "id": id,
        "accounts": {
            "customer": {"initial": customer, "shared": shared},
            "merchant": {"initial": 0, "shared": shared},
        },
        "steps": [
            {"id": "reserve", "name": "reserve stock", "compensation": "release stock", "moves": []},
            {"id": "charge", "name": "charge card", "compensation": "refund",
             "moves": [{"from": "customer", "to": "merchant", "amount": 1}]},
        ],
    })
}

/// The six instances, in native's order.
pub fn catalog() -> Vec<InstanceSpec> {
    vec![
        InstanceSpec {
            id: "orders",
            pattern: "queue",
            extra: false,
            params: json!({"ID": "orders", "Capacity": 3, "Initial": 2, "Payload": ""}),
            why: "Bounded (capacity 3) so the capacity invariant exists; two orders waiting so the one-saga-per-order caveat shows up in a run.",
        },
        InstanceSpec {
            id: "workers",
            pattern: "resource-pool",
            extra: false,
            params: json!({"id": "workers", "name": "workers", "capacity": 2}),
            why: "\"a fixed number of workers\": two.",
        },
        InstanceSpec {
            id: "payment",
            pattern: "saga",
            extra: false,
            params: saga_params("payment", 1, false),
            why: "Two steps so compensation runs in reverse; charge moves one token, so its compensation is the refund. \"A failed payment must refund\" is fail_commit after the charge.",
        },
        InstanceSpec {
            id: "payment2",
            pattern: "saga",
            extra: true,
            params: saga_params("payment2", 0, true),
            why: "A second order needs a second saga (one saga is one case). Its accounts are shared, so a token link to payment's accounts makes one balance.",
        },
        InstanceSpec {
            id: "orders2",
            pattern: "queue",
            extra: true,
            params: json!({"ID": "orders2", "Capacity": 3, "Initial": 0, "Payload": ""}),
            why: "A second queue: out of one into in of the other is a token link (a cospan composite).",
        },
        InstanceSpec {
            id: "workers2",
            pattern: "resource-pool",
            extra: true,
            params: json!({"id": "workers2", "name": "workers2", "capacity": 1}),
            why: "A second pool: its available place can be token-linked to the first pool's.",
        },
    ]
}

pub const DEFAULT_INSTANCES: [&str; 3] = ["orders", "workers", "payment"];

pub fn catalog_ids() -> Vec<&'static str> {
    catalog().iter().map(|s| s.id).collect()
}

fn text(p: &Value, k: &str) -> String {
    p[k].as_str().unwrap_or_default().to_string()
}

fn int(p: &Value, k: &str) -> i64 {
    p[k].as_i64().unwrap_or_default()
}

impl InstanceSpec {
    /// The instance's subnet, from pflow-compose's constructor.
    pub fn build(&self) -> Result<Subnet, String> {
        let p = &self.params;
        match self.pattern {
            "queue" => new_queue(QueueSpec {
                id: text(p, "ID"),
                capacity: int(p, "Capacity"),
                initial: int(p, "Initial"),
                description: String::new(),
            }),
            "resource-pool" => Ok(new_resource_pool(text(p, "id"), int(p, "capacity"))),
            "saga" => {
                let mut b = Saga::builder(text(p, "id"));
                // accounts sorted by id, as native sorts them
                for id in ["customer", "merchant"] {
                    let a = &p["accounts"][id];
                    b = if a["shared"] == json!(true) {
                        b.shared_account(id)
                    } else {
                        b.account(id, int(a, "initial"))
                    };
                }
                for st in p["steps"].as_array().into_iter().flatten() {
                    let mut s = b.step(text(st, "id"));
                    let name = text(st, "name");
                    if !name.is_empty() {
                        s = s.name(name);
                    }
                    let comp = text(st, "compensation");
                    if !comp.is_empty() {
                        s = s.compensate(comp);
                    }
                    for m in st["moves"].as_array().into_iter().flatten() {
                        s = s.move_tokens(text(m, "from"), text(m, "to"), int(m, "amount"));
                    }
                    b = s.done();
                }
                Ok(b.build_validated()?.to_meta_subnet())
            }
            other => Err(format!("unknown pattern {other:?}")),
        }
    }

    /// The catalog entry pflow_bundle_example lists.
    pub fn view(&self) -> Value {
        let built = self.build();
        json!({
            "id": self.id, "pattern": self.pattern, "label": self.id, "extra": self.extra,
            "params": self.params, "why": self.why,
            "available": built.is_ok(), "reason": built.err(),
        })
    }
}

/// The three idioms, on the default instances.
pub fn default_links() -> Vec<Link> {
    let ev = |a: &str, ap: &str, b: &str, bp: &str| {
        new_link(LinkKind::Event, port_end(a, ap), port_end(b, bp))
    };
    vec![
        ev("orders", "dequeue", "workers", "acquire"),
        ev("orders", "dequeue", "payment", "do:reserve"),
        ev("workers", "release", "payment", "commit"),
    ]
}

/// A catalog composition: what native's bundle.open leaves open.
pub struct Opened {
    pub bundle: Bundle,
    pub instances: Vec<&'static str>,
    pub unavailable: Vec<Value>,
    pub dropped: Vec<Value>,
}

/// Builds `instances` from the catalog and adds `links` whose ends exist,
/// each judged as pflow_bundle_add_link judges it. An instance that cannot
/// be built is recorded, not fatal; a link with an end that is not there is
/// dropped and listed. The error is in native's words.
pub fn open(instances: &[String], links: &[Link]) -> Result<Opened, String> {
    let cat = catalog();
    let mut o = Opened {
        bundle: Bundle::new("composition"),
        instances: vec![],
        unavailable: vec![],
        dropped: vec![],
    };
    for id in instances {
        let spec = cat.iter().find(|s| s.id == id);
        let have = o.instances.contains(&id.as_str());
        let res = match spec {
            None => Err(format!(
                "no instance {}; the catalog has {}",
                super::text::quote(id),
                quote_list(&catalog_ids())
            )),
            Some(_) if have => Err(format!(
                "instance {} is already in the composition",
                super::text::quote(id)
            )),
            Some(spec) => spec.build().map(|sub| {
                o.bundle.add_subnet(sub);
                o.instances.push(spec.id);
            }),
        };
        if let Err(e) = res {
            if have || spec.is_none() {
                return Err(e);
            }
            o.unavailable.push(json!({"instance": id, "reason": e}));
        }
    }
    for l in links {
        let missing: Vec<&str> = [&l.from.subnet, &l.to.subnet]
            .into_iter()
            .filter(|s| o.bundle.subnet_by_id(s).is_none())
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            o.dropped.push(json!({
                "link": link_view(l),
                "reason": format!("not in the composition: {}", missing.join(", ")),
            }));
            continue;
        }
        let v = verdict_in(&o.bundle, l);
        if !v.ok {
            return Err(v.refusal());
        }
        o.bundle.add_link(l.clone());
    }
    Ok(o)
}

// ------------------------------------------------------------ scenarios

/// A firing script in members "<subnet>/<transition>" with its expected
/// predicates: judgment, frozen in native from the prototype.
pub struct Scenario {
    pub id: &'static str,
    pub title: &'static str,
    pub script: &'static [&'static str],
    pub expect: &'static [(&'static str, i64, &'static str)],
    pub note: &'static str,
}

pub const SCENARIOS: [Scenario; 4] = [
    Scenario {
        id: "commit",
        title: "An order is paid",
        script: &["orders/dequeue", "payment/do_charge", "payment/commit"],
        expect: &[
            ("payment/completed", 1, "the saga completes"),
            ("workers/available", 2, "the worker returns to the pool"),
        ],
        note: "",
    },
    Scenario {
        id: "refund",
        title: "The payment fails after the charge (refund)",
        script: &[
            "orders/dequeue",
            "payment/do_charge",
            "payment/fail_commit",
            "payment/undo_charge",
            "payment/undo_reserve",
        ],
        expect: &[
            ("payment/rolled_back", 1, "the saga rolls back"),
            ("payment/customer", 1, "the customer is refunded"),
            ("workers/available", 2, "the worker returns to the pool"),
        ],
        note: "Expected to fail on the last predicate: release is fused only with commit (the pool-leak caveat, patterns.ttl:496).",
    },
    Scenario {
        id: "charge-fails",
        title: "The charge itself fails (nothing to refund)",
        script: &["orders/dequeue", "payment/fail_charge", "payment/undo_reserve"],
        expect: &[
            ("payment/rolled_back", 1, "the saga rolls back"),
            ("payment/customer", 1, "the customer was never charged"),
            ("workers/available", 2, "the worker returns to the pool"),
        ],
        note: "A step that fails itself moved nothing, so there is nothing to refund (saga caveat). The worker leaks here too.",
    },
    Scenario {
        id: "second-order",
        title: "A second order, one saga",
        script: &[
            "orders/dequeue",
            "payment/do_charge",
            "payment/commit",
            "orders/dequeue",
        ],
        expect: &[("orders/items", 0, "both orders are taken off the queue")],
        note: "Expected to stop at the last step: the fused dequeue needs next_reserve, which the one-shot saga has spent (saga caveat: one saga is one case).",
    },
];

pub fn scenario(id: &str) -> Option<&'static Scenario> {
    SCENARIOS.iter().find(|s| s.id == id)
}

pub fn scenario_ids() -> Vec<&'static str> {
    SCENARIOS.iter().map(|s| s.id).collect()
}

impl Scenario {
    pub fn expects(&self) -> Vec<Expect> {
        self.expect
            .iter()
            .map(|(place, n, says)| Expect {
                place: place.to_string(),
                n: *n,
                says: says.to_string(),
            })
            .collect()
    }

    pub fn script(&self) -> Vec<String> {
        self.script.iter().map(|s| s.to_string()).collect()
    }

    pub fn view(&self) -> Value {
        let expect: Vec<Value> = self
            .expect
            .iter()
            .map(|(place, n, says)| json!({"place": place, "op": "==", "n": n, "says": says}))
            .collect();
        let mut v =
            json!({"id": self.id, "title": self.title, "script": self.script, "expect": expect});
        if !self.note.is_empty() {
            v["note"] = json!(self.note);
        }
        v
    }
}
