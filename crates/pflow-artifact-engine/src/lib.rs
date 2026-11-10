//! A zero-import wasm32 module exposing pflow-rs's reachability analysis,
//! P/T-invariants and firing rule as JSON in, JSON out.
//!
//! ABI (all u32 pointers into this module's linear memory):
//!   alloc(len) -> ptr            reserve len bytes for the caller to write JSON into
//!   dealloc(ptr, len)            free a buffer returned by alloc or by a call below
//!   analyze(ptr, len) -> u64     request {net, max_states?}; returns (ptr << 32) | len
//!   fire(ptr, len) -> u64        request {net, marking?, transition}
//! The response is always a JSON object; on failure it is {"error": "..."}.
//! No wasm-bindgen, no WASI: the page needs only WebAssembly.instantiate(bytes, {}).
//!
//! Built into @pflow-xyz/engine (npm/, `make engine-npm`): one script that embeds
//! this module and defines the global `PflowEngine`. Moved from pflow-xyz's
//! experiments/artifact-wasm/engine, whose build pflow_artifact embeds inline
//! (engine "wasm-inline"); npm/goldens/ hold this build to that one's replies.

use std::collections::BTreeMap;

use pflow_metamodel::{Marking, Model};
use pflow_reachability::{Analyzer, InvariantAnalyzer, Verdict};
use serde_json::{json, Value};

#[no_mangle]
pub extern "C" fn alloc(len: u32) -> u32 {
    let mut v = Vec::<u8>::with_capacity(len as usize);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p as u32
}

#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: u32, len: u32) {
    drop(Vec::from_raw_parts(ptr as *mut u8, 0, len as usize));
}

fn respond(v: Value) -> u64 {
    let mut s = v.to_string().into_bytes();
    s.shrink_to_fit();
    let (p, n) = (s.as_mut_ptr() as u64, s.len() as u64);
    std::mem::forget(s);
    (p << 32) | n
}

unsafe fn request(ptr: u32, len: u32) -> Result<Value, String> {
    let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
    serde_json::from_slice(bytes).map_err(|e| format!("bad request JSON: {e}"))
}

fn parse_net(v: &Value) -> Result<Model, String> {
    let text = serde_json::to_vec(v).map_err(|e| e.to_string())?;
    if pflow_parser::is_pflow_json(&text) {
        Ok(pflow_parser::model_from_json(&text).map_err(|e| format!("net: {e}"))?.0)
    } else {
        serde_json::from_slice::<Model>(&text).map_err(|e| format!("net: {e}"))
    }
}

fn sorted(m: &Marking) -> BTreeMap<&str, i64> {
    m.iter().map(|(k, v)| (k.as_str(), *v)).collect()
}

fn verdict(v: Verdict) -> Value {
    match v {
        Verdict::True => json!(true),
        Verdict::False => json!(false),
        _ => json!("unknown"),
    }
}

fn analyze_inner(req: &Value) -> Result<Value, String> {
    let model = parse_net(req.get("net").ok_or("missing net")?)?;
    let max_states = req.get("max_states").and_then(Value::as_u64).unwrap_or(2000) as usize;
    let r = Analyzer::new(&model).with_max_states(max_states).analyze();
    let lv = &r.liveness;
    let witness = lv.witness.as_ref().map(|w| {
        json!({
            "kind": format!("{:?}", w.kind).to_lowercase(),
            "trace": w.trace,
            "marking": sorted(&w.marking),
            "region_size": w.region_size,
            "never_fires": w.never_fires,
        })
    });
    let inv = InvariantAnalyzer::new(&model);
    let initial = model.initial_marking();
    let p_basis = inv.p_invariant_basis();
    let p: Vec<Value> = inv
        .find_p_invariants(&initial)
        .iter()
        .map(|i| json!({"places": i.places, "coefficients": i.coefficients, "value": i.value, "render": i.render()}))
        .collect();
    let t_basis = inv.t_invariant_basis();
    let t: Vec<Value> = inv
        .find_t_invariants()
        .iter()
        .map(|i| json!({"transitions": i.transitions, "counts": i.counts, "render": i.render()}))
        .collect();
    let dead: Vec<Value> = r
        .dead_markings
        .iter()
        .filter_map(|k| r.graph.get_state_by_key(k))
        .map(|s| json!(sorted(&s.marking)))
        .collect();
    Ok(json!({
        "engine": "pflow-rs pflow-reachability (wasm32, @pflow-xyz/engine)",
        "state_count": r.state_count,
        "edge_count": r.edge_count,
        "complete": r.is_complete,
        "bounded": r.bounded,
        "max_tokens": r.max_tokens.into_iter().collect::<BTreeMap<_, _>>(),
        "deadlock_free": verdict(lv.deadlock_free),
        "live": verdict(lv.live),
        "quasi_live": verdict(lv.quasi_live),
        "not_live": lv.not_live,
        "witness": witness,
        "dead_markings": dead,
        "p_invariants": p,
        "p_truncated": p_basis.truncated,
        "t_invariants": t,
        "t_truncated": t_basis.truncated,
    }))
}

fn fire_inner(req: &Value) -> Result<Value, String> {
    let model = parse_net(req.get("net").ok_or("missing net")?)?;
    let mut mk = model.initial_marking();
    if let Some(obj) = req.get("marking").and_then(Value::as_object) {
        for (k, v) in obj {
            mk.insert(k.clone(), v.as_i64().ok_or("marking values must be integers")?);
        }
    }
    if let Some(t) = req.get("transition").and_then(Value::as_str) {
        model.enabled_why_not(t, &mk).map_err(|e| e.to_string())?;
        mk = model.fire(t, &mk);
    }
    Ok(json!({"marking": sorted(&mk), "enabled": model.enabled_transitions(&mk)}))
}

fn wrap(r: Result<Value, String>) -> u64 {
    respond(r.unwrap_or_else(|e| json!({"error": e})))
}

#[no_mangle]
pub unsafe extern "C" fn analyze(ptr: u32, len: u32) -> u64 {
    wrap(request(ptr, len).and_then(|v| analyze_inner(&v)))
}

#[no_mangle]
pub unsafe extern "C" fn fire(ptr: u32, len: u32) -> u64 {
    wrap(request(ptr, len).and_then(|v| fire_inner(&v)))
}
