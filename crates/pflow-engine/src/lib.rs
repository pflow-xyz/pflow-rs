//! The CID-checked pflow engine: one deterministic wasm32 module that loads a
//! model by its content address and runs the existing checks on it.
//!
//! A verdict is re-derivable from public infrastructure: the model's CID names
//! its bytes, the engine's sha256 names the code (`scripts/build-wasm-repro.sh`
//! rebuilds it bit for bit from a pflow-rs commit), and the same
//! `(cid, op, args)` gives the same `verdict_sha256` on any machine.
//!
//! This crate adds no engine behaviour. [`Model::load`] recomputes the CID of
//! the bytes it is given (`pflow_hosted`'s `cid`, the URDNA2015 seal pflow.xyz
//! uses) and refuses them when it is not the CID the caller asked for; then
//! [`Model::run`] dispatches to code that already exists:
//!
//! | op | engine |
//! |----|--------|
//! | `check`, `validate`, `verify`, `invariants`, `incidence`, `sde` | `pflow_hosted::call` (the pflow.xyz/mcp tools, run locally) |
//! | `simulate` (mass-action ODE, Tsit5), `ssa` (portable Gillespie), `reachability`, `fire` | `pflow_wasm::Core` (the @pflow-xyz/pflow-wasm engine) |
//!
//! Every answer is wrapped in an envelope whose `verdict_sha256` is the sha256
//! of the canonical JSON of `{cid, op, args, result}` ([`canonical_json`]:
//! keys sorted, no whitespace, serde_json's shortest round-trip floats). NaN
//! cannot reach a hash: a JSON number is finite, and serde_json writes a
//! non-finite `f64` as `null`, so any NaN, whatever its bit pattern, hashes as
//! `null` (`nan_hashes_as_null`).
//!
//! The logic is plain Rust (tested natively by `cargo test -p pflow-engine`);
//! the `wasm_bindgen` items at the bottom only convert values.

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use wasm_bindgen::prelude::*;

/// The engine's version: the pflow-rs workspace version. The engine's
/// identity is its sha256 (the build manifest), not this string.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The ops [`Model::run`] answers, in the order `ops()` lists them.
pub const OPS: &[&str] = &[
    "check",
    "validate",
    "verify",
    "invariants",
    "incidence",
    "sde",
    "simulate",
    "ssa",
    "reachability",
    "fire",
];

const HOSTED_OPS: &[&str] = &["check", "validate", "verify", "invariants", "incidence", "sde"];

/// The CID pflow.xyz gives these bytes: the URDNA2015 seal of the JSON-LD
/// document with its top-level `@id` dropped. Refuses what the local sealer
/// cannot reproduce byte for byte (another `@context`, a composition's set).
pub fn cid_of(bytes: &[u8]) -> Result<String, String> {
    let doc = parse_doc(bytes)?;
    seal(&doc)
}

fn parse_doc(bytes: &[u8]) -> Result<Value, String> {
    let doc: Value =
        serde_json::from_slice(bytes).map_err(|e| format!("model: not JSON: {e}"))?;
    if !doc.is_object() {
        return Err("model: a JSON-LD object is required".into());
    }
    Ok(doc)
}

fn seal(doc: &Value) -> Result<String, String> {
    let out = pflow_hosted::call("cid", &json!({ "net": doc }))?;
    out.get("cid")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "internal: cid returned no cid".to_string())
}

/// A model whose bytes are known to seal to [`Model::cid`].
pub struct Model {
    cid: String,
    text: String,
    doc: Value,
}

impl Model {
    /// Loads `bytes`, the model's canonical JSON-LD, and refuses them unless
    /// they seal to `expected_cid`.
    pub fn load(bytes: &[u8], expected_cid: &str) -> Result<Model, String> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| format!("model: not UTF-8: {e}"))?
            .to_string();
        let doc = parse_doc(bytes)?;
        let cid = seal(&doc)?;
        if cid != expected_cid {
            return Err(format!(
                "cid mismatch: asked for {expected_cid}, the bytes seal to {cid}"
            ));
        }
        Ok(Model { cid, text, doc })
    }

    pub fn cid(&self) -> &str {
        &self.cid
    }

    /// Runs `op` with `args` (a JSON object, or empty for none) and returns
    /// the envelope as canonical JSON text.
    pub fn run(&self, op: &str, args_json: &str) -> Result<String, String> {
        let args: Value = if args_json.trim().is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str(args_json).map_err(|e| format!("args: not JSON: {e}"))?
        };
        let Value::Object(args_map) = &args else {
            return Err("args: a JSON object is required".into());
        };
        let result = self.result(op, args_map)?;
        Ok(canonical_json(&envelope(&self.cid, op, &args, &result)))
    }

    fn result(&self, op: &str, args: &Map<String, Value>) -> Result<Value, String> {
        if HOSTED_OPS.contains(&op) {
            if args.contains_key("net") {
                return Err(format!("{op}: args must not carry a net; the engine supplies the loaded one"));
            }
            let mut call = args.clone();
            call.insert("net".into(), self.doc.clone());
            return pflow_hosted::call(op, &Value::Object(call));
        }
        let opts = Value::Object(args.clone()).to_string();
        let text = match op {
            "simulate" => self.core()?.simulate_json(&opts)?,
            "ssa" => self.core()?.stochastic_json(&opts)?,
            "reachability" => self.core()?.reachability_json(&opts)?,
            "fire" => return self.fire(args),
            _ => return Err(format!("unknown op {op:?}; the ops are {}", OPS.join(", "))),
        };
        serde_json::from_str(&text).map_err(|e| format!("internal: {op} result is not JSON: {e}"))
    }

    fn core(&self) -> Result<pflow_wasm::Core, String> {
        pflow_wasm::Core::new(&self.text)
    }

    /// `{"sequence": ["t1", "t2", ...]}`: fires each in turn from the initial
    /// marking. The answer is the marking after each step; a refusal names the
    /// step and the firing rule's reason.
    fn fire(&self, args: &Map<String, Value>) -> Result<Value, String> {
        for k in args.keys() {
            if k != "sequence" {
                return Err(format!("fire: unknown argument {k:?} (only sequence)"));
            }
        }
        let seq = match args.get("sequence") {
            None => Vec::new(),
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| v.as_str().map(str::to_string).ok_or("fire: sequence is a list of transition ids"))
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => return Err("fire: sequence is a list of transition ids".into()),
        };
        let mut core = self.core()?;
        let parse = |t: String| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string());
        let initial = parse(core.marking_json())?;
        let mut steps = Vec::new();
        for (i, t) in seq.iter().enumerate() {
            let m = core
                .fire_json(t)
                .map_err(|e| format!("fire: step {i} ({t:?}): {e}"))?;
            steps.push(json!({ "transition": t, "marking": parse(m)? }));
        }
        Ok(json!({
            "initial": initial,
            "steps": steps,
            "enabled": parse(core.enabled_json())?,
        }))
    }
}

/// The answer to one `(cid, op, args)`.
pub fn envelope(cid: &str, op: &str, args: &Value, result: &Value) -> Value {
    let verdict = verdict_sha256(cid, op, args, result);
    json!({
        "engine": "pflow-engine",
        "engine_version": ENGINE_VERSION,
        "cid": cid,
        "op": op,
        "args": args,
        "result": result,
        "verdict_sha256": verdict,
    })
}

/// sha256 of the canonical JSON of `{cid, op, args, result}`. The engine's
/// version is not in it: a rebuilt engine that answers the same is the same
/// verdict.
pub fn verdict_sha256(cid: &str, op: &str, args: &Value, result: &Value) -> String {
    let body = json!({ "cid": cid, "op": op, "args": args, "result": result });
    hex(&Sha256::digest(canonical_json(&body).as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// JSON with every object's keys sorted (by UTF-8 bytes), no whitespace, and
/// serde_json's number text (integers as written, floats in shortest
/// round-trip form). The same value gives the same text on every platform.
pub fn canonical_json(v: &Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                write_canonical(&m[k], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(x, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

// ---- the JavaScript boundary -------------------------------------------------

/// The engine's version (the pflow-rs workspace version).
#[wasm_bindgen(js_name = engineVersion)]
pub fn engine_version() -> String {
    ENGINE_VERSION.to_string()
}

/// The ops `Model.run` answers, as a JSON array.
#[wasm_bindgen]
pub fn ops() -> String {
    Value::from(OPS.to_vec()).to_string()
}

/// The CID these bytes seal to.
#[wasm_bindgen(js_name = cidOf)]
pub fn cid_of_js(bytes: &[u8]) -> Result<String, JsError> {
    cid_of(bytes).map_err(|e| JsError::new(&e))
}

/// Load, check the CID and run one op: the envelope as canonical JSON text.
#[wasm_bindgen]
pub fn verify(bytes: &[u8], cid: &str, op: &str, args: &str) -> Result<String, JsError> {
    Model::load(bytes, cid)
        .and_then(|m| m.run(op, args))
        .map_err(|e| JsError::new(&e))
}

/// A model loaded by CID.
#[wasm_bindgen(js_name = Model)]
pub struct JsModel {
    inner: Model,
}

#[wasm_bindgen(js_class = Model)]
impl JsModel {
    /// Throws unless `bytes` seal to `cid`.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8], cid: &str) -> Result<JsModel, JsError> {
        Model::load(bytes, cid)
            .map(|inner| JsModel { inner })
            .map_err(|e| JsError::new(&e))
    }

    #[wasm_bindgen(getter)]
    pub fn cid(&self) -> String {
        self.inner.cid().to_string()
    }

    /// Runs `op` with `args` (JSON text, `""` for none): the envelope as
    /// canonical JSON text.
    pub fn run(&self, op: &str, args: &str) -> Result<String, JsError> {
        self.inner.run(op, args).map_err(|e| JsError::new(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_sorts_keys_at_every_level() {
        let v: Value = serde_json::from_str(r#"{"b":[{"z":1,"a":0.5}],"a":"x"}"#).unwrap();
        assert_eq!(canonical_json(&v), r#"{"a":"x","b":[{"a":0.5,"z":1}]}"#);
    }

    #[test]
    fn nan_hashes_as_null() {
        // Two NaNs with different bit patterns, and null, are one verdict.
        let quiet = f64::from_bits(0x7ff8_0000_0000_0001);
        let other = f64::from_bits(0xfff8_dead_beef_0000);
        assert!(quiet.is_nan() && other.is_nan());
        let a = json!({ "x": quiet });
        let b = json!({ "x": other });
        let c = json!({ "x": null });
        let h = |v: &Value| verdict_sha256("z", "op", &json!({}), v);
        assert_eq!(h(&a), h(&c));
        assert_eq!(h(&b), h(&c));
    }
}

/// The recorded example set (`verdicts/cases.json`), run by one engine.
/// Native and WASM both go through this, so the comparison is of engines,
/// not of two harnesses.
pub mod cases {
    use super::*;

    /// Runs every case in `cases_json` (`{"cases": [{id, model, cid, op,
    /// args}]}`, `model` a path `read` resolves) and returns
    /// `{"results": {id: {"verdict_sha256"} | {"refusal"}}}` as canonical
    /// JSON. A case's refusal is a result too: it is recorded and must recur.
    pub fn run_all(cases_json: &[u8], read: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> String {
        let doc: Value = match serde_json::from_slice(cases_json) {
            Ok(v) => v,
            Err(e) => return canonical_json(&json!({ "error": format!("cases: {e}") })),
        };
        let mut results = Map::new();
        for c in doc["cases"].as_array().into_iter().flatten() {
            let id = c["id"].as_str().unwrap_or("?").to_string();
            let args = c.get("args").cloned().unwrap_or_else(|| json!({}));
            let out = read(c["model"].as_str().unwrap_or("")).and_then(|bytes| {
                let m = Model::load(&bytes, c["cid"].as_str().unwrap_or(""))?;
                m.run(c["op"].as_str().unwrap_or(""), &args.to_string())
            });
            let entry = match out {
                Ok(text) => {
                    let env: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    json!({ "verdict_sha256": env["verdict_sha256"] })
                }
                Err(e) => json!({ "refusal": e }),
            };
            results.insert(id, entry);
        }
        canonical_json(&json!({ "results": results }))
    }
}

/// [`cases::run_all`] in the module: `files` maps each case's model path to
/// its bytes, as a JSON object of path → string (the file's UTF-8 text).
#[wasm_bindgen(js_name = runCases)]
pub fn run_cases_js(cases_json: &str, files_json: &str) -> Result<String, JsError> {
    let files: Map<String, Value> = serde_json::from_str(files_json)
        .map_err(|e| JsError::new(&format!("files: {e}")))?;
    let read = |p: &str| {
        files
            .get(p)
            .and_then(Value::as_str)
            .map(|s| s.as_bytes().to_vec())
            .ok_or_else(|| format!("{p}: not supplied"))
    };
    Ok(cases::run_all(cases_json.as_bytes(), &read))
}
