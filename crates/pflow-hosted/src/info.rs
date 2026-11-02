//! `info`: hosted `pflow_info` (`mcpInfo`). The net as the editor wrote it
//! (places with `initial`/`capacity` echoed as given, transitions with their
//! `rate`, default 1, arcs with `inhibit`), its color count, and the initial
//! marking and enabled transitions in the analyzers' unfolded names.

use pflow_reachability::Analyzer;
use serde_json::{json, Map, Value};

use crate::net::{
    color_count, expand, go_f64, int_map, model_of, net_arg, petri_net, sorted, start_marking,
};

pub(crate) fn run(args: &Value) -> Result<Value, String> {
    let doc = net_arg(args)?;
    let net = petri_net(&doc);
    let empty = Map::new();

    let pm = doc
        .get("places")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let places: Vec<Value> = sorted(pm.keys())
        .into_iter()
        .map(|id| {
            let p = pm[&id].as_object().unwrap_or(&empty);
            let mut e = Map::new();
            e.insert("id".into(), json!(id));
            e.insert(
                "initial".into(),
                p.get("initial").cloned().unwrap_or(Value::Null),
            );
            e.insert(
                "capacity".into(),
                p.get("capacity").cloned().unwrap_or(Value::Null),
            );
            if let Some(l) = p.get("label") {
                e.insert("label".into(), l.clone());
            }
            Value::Object(e)
        })
        .collect();

    let tm = doc
        .get("transitions")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let transitions: Vec<Value> = sorted(tm.keys())
        .into_iter()
        .map(|id| {
            let t = tm[&id].as_object().unwrap_or(&empty);
            let mut e = Map::new();
            e.insert("id".into(), json!(id));
            e.insert("rate".into(), go_f64(rate_of(t).unwrap_or(1.0)));
            if let Some(l) = t.get("label") {
                e.insert("label".into(), l.clone());
            }
            Value::Object(e)
        })
        .collect();

    let arcs: Vec<Value> = doc
        .get("arcs")
        .and_then(Value::as_array)
        .map(|al| {
            al.iter()
                .filter_map(Value::as_object)
                .map(|m| {
                    let get = |k: &str| m.get(k).cloned().unwrap_or(Value::Null);
                    json!({
                        "source": get("source"),
                        "target": get("target"),
                        "weight": get("weight"),
                        "inhibit": m.get("inhibitTransition") == Some(&Value::Bool(true)),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let (expanded, _) = expand(&net);
    let model = model_of(&expanded);
    let analyzer = Analyzer::new(&model);
    let enabled: Vec<String> = sorted(expanded.transitions.keys())
        .into_iter()
        .filter(|t| analyzer.can_fire(std::slice::from_ref(t)).0)
        .collect();

    let mut out = Map::new();
    out.insert("places".into(), Value::Array(places));
    out.insert("transitions".into(), Value::Array(transitions));
    out.insert("arcs".into(), Value::Array(arcs));
    out.insert("colors".into(), json!(color_count(&doc)));
    out.insert("marking".into(), int_map(&start_marking(&expanded)));
    out.insert("enabled".into(), json!(enabled));
    for k in ["name", "description"] {
        if let Some(v) = doc.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    Ok(Value::Object(out))
}

/// `ratesOf` for one transition: its `rate`, when a finite non-negative number.
pub(crate) fn rate_of(t: &Map<String, Value>) -> Option<f64> {
    t.get("rate")
        .and_then(Value::as_f64)
        .filter(|r| *r >= 0.0 && r.is_finite())
}
