//! `bundle_net`: a flattened bundle as an ordinary pflow.xyz net.
//!
//! `bundle_flatten` answers with the flat net in the composition page's own
//! shape (lists of places, transitions and `from`/`to` arcs) which the net
//! tools (`fire`, `simulate`, `reachability`, `invariants`, `verify`, `cid`,
//! `export`, ...) refuse to read. This is the same net written in the editor's
//! JSON-LD (places and transitions keyed by id, `source`/`target` arcs), so
//! every net tool reads a composite exactly as it reads any other net.
//!
//! The conversion is the inverse of `net::model_of` (the reader the net tools
//! use): a read arc is written as a transition -> place arc with
//! `inhibitTransition`, an inhibitor as a place -> transition one, a place's
//! capacity as its one-colour capacity, and an arc with no weight as weight 1
//! (every firing rule reads 0 as 1; the net reader would drop a zero-weight
//! arc). What the editor's format cannot say is refused, naming it, rather
//! than dropped: a net that ran differently from the composite would be worse
//! than no net.
//!
//! One id changes: a `:` is written `~` (the fused transitions' `fused:a+b`
//! would read as a JSON-LD compact IRI, and the CID would be refused). The
//! provenance (which `<subnet>/<element>` each flat element came from, the
//! declared constraints) is returned beside the net, not in it, so the net is
//! plain and its CID depends on nothing but the net.

use std::collections::BTreeMap;

use pflow_metamodel::{ArcType, Model};
use serde_json::{json, Map, Value};

use super::rules::Flat;

const CONTEXT: &str = "https://pflow.xyz/schema";
const TOKEN: &str = "https://pflow.xyz/tokens/black";

/// What a flat model has that the editor's format cannot say.
pub fn unrepresentable(m: &Model) -> Vec<String> {
    let mut why = vec![];
    for p in &m.places {
        if !p.is_token() {
            why.push(format!(
                "place {} is a data place (kind {:?}); the editor's format has token places only",
                p.id, p.kind
            ));
        }
    }
    for t in &m.transitions {
        if !t.guard.is_empty() {
            why.push(format!(
                "transition {} has the guard {:?}; the editor's format has no guards",
                t.id, t.guard
            ));
        }
    }
    why
}

/// `<subnet>/<element>` members of each flat element, in sorted order.
fn members(map: &std::collections::HashMap<String, std::collections::HashMap<String, String>>) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (s, locals) in map {
        for (local, flat) in locals {
            out.entry(flat.clone()).or_default().push(format!("{s}/{local}"));
        }
    }
    for v in out.values_mut() {
        v.sort();
    }
    out
}

/// A flat id as the editor net spells it.
pub fn net_id(id: &str) -> String {
    id.replace(':', "~")
}

/// The flat net in the editor's JSON-LD, laid out one column per subnet,
/// with its provenance. `Err` lists what could not be written.
pub fn editor_net(name: &str, f: &Flat, subnet_order: &[String]) -> Result<(Value, Value), Vec<String>> {
    let m = &f.model;
    let why = unrepresentable(m);
    if !why.is_empty() {
        return Err(why);
    }
    let pm = members(&f.map.place);
    let tm = members(&f.map.transition);
    // the column of an element: the first subnet (in bundle order) that holds a member
    let column = |ms: Option<&Vec<String>>| -> usize {
        ms.and_then(|ms| {
            ms.iter()
                .filter_map(|x| x.split_once('/'))
                .filter_map(|(s, _)| subnet_order.iter().position(|o| o == s))
                .min()
        })
        .unwrap_or(0)
    };
    let mut fill: BTreeMap<(usize, bool), usize> = BTreeMap::new();
    let mut place = |col: usize, is_place: bool| -> (i64, i64) {
        let n = fill.entry((col, is_place)).or_insert(0);
        let (row, lane) = (*n / 3, *n % 3);
        *n += 1;
        let base = col as i64 * 360;
        if is_place {
            (base + lane as i64 * 110, 80 + row as i64 * 150)
        } else {
            (base + lane as i64 * 110 + 55, 155 + row as i64 * 150)
        }
    };
    let mut places = Map::new();
    for p in &m.places {
        let (x, y) = place(column(pm.get(&p.id)), true);
        places.insert(
            net_id(&p.id),
            json!({"@type": "Place", "initial": [p.initial], "capacity": [p.capacity], "x": x, "y": y}),
        );
    }
    let mut transitions = Map::new();
    for t in &m.transitions {
        let (x, y) = place(column(tm.get(&t.id)), false);
        transitions.insert(net_id(&t.id), json!({"@type": "Transition", "x": x, "y": y}));
    }
    let arcs: Vec<Value> = m
        .arcs
        .iter()
        .map(|a| {
            // every firing rule reads a missing weight as 1
            let w = a.weight.max(1);
            let (source, target, inhibit) = match a.typ {
                ArcType::Normal => (&a.from, &a.to, false),
                ArcType::Inhibitor => (&a.from, &a.to, true),
                // net::model_of reads transition -> place + inhibitTransition as a read arc
                ArcType::Read => (&a.to, &a.from, true),
            };
            json!({"@type": "Arrow", "source": net_id(source), "target": net_id(target), "weight": [w], "inhibitTransition": inhibit})
        })
        .collect();
    let constraints: Vec<Value> = m
        .constraints
        .iter()
        .map(|c| json!({"id": c.id, "expr": net_id(&c.expr)}))
        .collect();
    let rekey = |m: BTreeMap<String, Vec<String>>| -> BTreeMap<String, Vec<String>> {
        m.into_iter().map(|(k, v)| (net_id(&k), v)).collect()
    };
    let net = json!({
        "@context": CONTEXT,
        "@type": "PetriNet",
        "@version": "1.1",
        "name": name,
        "token": [TOKEN],
        "places": places,
        "transitions": transitions,
        "arcs": arcs,
    });
    let prov = json!({
        "places": rekey(pm),
        "transitions": rekey(tm),
        "constraints": constraints,
        "subnets": subnet_order,
    });
    Ok((net, prov))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_compose::Bundle;
    use crate::net::{expand, model_of, petri_net};
    use serde_json::json;

    fn bundle(instances: Value, links: Value) -> Bundle {
        let r = crate::call("bundle_example", &json!({"instances": instances, "links": links})).unwrap();
        serde_json::from_value(r["bundle"].clone()).unwrap()
    }

    /// The net the net tools read is the flat model: same places, markings,
    /// capacities and arcs, and the same enabled set after the same firings.
    fn same_as_flat(b: &Bundle) {
        let f = super::super::rules::flatten(b).unwrap();
        let order: Vec<String> = b.subnets.iter().map(|s| s.id.clone()).collect();
        let (net, _) = editor_net(&b.name, &f, &order).unwrap();
        let doc = net.as_object().unwrap().clone();
        let (expanded, _) = expand(&petri_net(&doc));
        let read = model_of(&expanded);
        let mut want: Vec<(String, i64, i64)> = f.model.places.iter().map(|p| (net_id(&p.id), p.initial, p.capacity)).collect();
        let mut got: Vec<(String, i64, i64)> = read.places.iter().map(|p| (p.id.clone(), p.initial, p.capacity)).collect();
        want.sort();
        got.sort();
        assert_eq!(want, got);
        let key = |a: &pflow_metamodel::Arc| (net_id(&a.from), net_id(&a.to), a.weight.max(1), a.typ as u8);
        let mut wa: Vec<_> = f.model.arcs.iter().map(key).collect();
        let mut ga: Vec<_> = read.arcs.iter().map(|a| (a.from.clone(), a.to.clone(), a.weight, a.typ as u8)).collect();
        wa.sort();
        ga.sort();
        assert_eq!(wa, ga);
        // walk: always fire the first enabled transition (sorted), 40 steps, on both
        let (mut m1, mut m2) = (f.model.initial_marking(), read.initial_marking());
        for _ in 0..40 {
            let mut e1: Vec<String> = f.model.transitions.iter().filter(|t| f.model.enabled(&t.id, &m1)).map(|t| net_id(&t.id)).collect();
            let mut e2: Vec<String> = read.transitions.iter().filter(|t| read.enabled(&t.id, &m2)).map(|t| t.id.clone()).collect();
            e1.sort();
            e2.sort();
            assert_eq!(e1, e2);
            let Some(t) = e1.first() else { break };
            let orig = f.model.transitions.iter().find(|x| &net_id(&x.id) == t).unwrap().id.clone();
            m1 = f.model.fire(&orig, &m1);
            m2 = read.fire(t, &m2);
        }
    }

    #[test]
    fn default_composite() {
        same_as_flat(&bundle(json!(["orders", "workers", "payment"]), Value::Null));
    }

    #[test]
    fn linked_places_and_guards() {
        same_as_flat(&bundle(
            json!(["orders", "orders2", "workers", "workers2"]),
            json!([
                {"kind": "token", "from": {"subnet": "orders", "port": "out"}, "to": {"subnet": "orders2", "port": "in"}},
                {"kind": "guard", "from": {"subnet": "workers2", "port": "acquire"}, "to": {"subnet": "orders", "port": "out"}}
            ]),
        ));
    }
}
