//! `petri_canonical`: a content-addressed identifier for a model.
//!
//! Tries the real ecosystem CID first — [`pflow_tokenmodel::compute_cid`]
//! (URDNA2015-equivalent quad canonicalisation + dag-json CIDv1, matching
//! `pflow-xyz`'s `cid.mjs`) — which needs the input's own raw JSON-LD-shaped
//! bytes (a `@type`/`@id`-carrying document, i.e. Shape A). DSL text and
//! Shape B `Model` JSON have no `@id`/`@type` structure for that
//! canonicaliser to walk, so those fall back to
//! `pflow_tokenmodel::Schema::identity_hash` — a same-language-only
//! identity hash, not the ecosystem CID, labelled as such in the response
//! so a caller cannot mistake one for the other.

use super::convert::{model_to_schema, parse_any_model};
use serde::Serialize;

#[derive(Serialize)]
struct CanonicalResult {
    method: &'static str,
    cid: String,
    #[serde(rename = "note")]
    note: &'static str,
}

pub fn run(model: &str) -> Result<String, String> {
    let trimmed = model.trim();
    if !trimmed.starts_with('(') {
        if let Ok((cid, _nquads)) = pflow_tokenmodel::compute_cid(trimmed.as_bytes()) {
            let out = CanonicalResult {
                method: "urdna2015-cidv1",
                cid,
                note: "ecosystem-compatible CIDv1(dag-json, sha2-256) over the @id-stripped document, matching pflow-xyz's cid.mjs",
            };
            return serde_json::to_string_pretty(&out).map_err(|e| format!("Serialization error: {e}"));
        }
    }

    let parsed = parse_any_model(model)?;
    let schema = model_to_schema(&parsed);
    let out = CanonicalResult {
        method: "identity-hash",
        cid: schema.identity_hash(),
        note: "not the ecosystem CID: the input had no @id/@type JSON-LD structure to canonicalize (DSL text or Shape B JSON). This is pflow-rs's own identity hash over the model's JSON encoding",
    };

    serde_json::to_string_pretty(&out).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_model_same_hash() {
        let dsl = r#"(schema m (states (state p1 :kind token :initial 1)) (actions) (arcs))"#;
        let a = run(dsl).unwrap();
        let b = run(dsl).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_model_different_hash() {
        let a = run(r#"(schema m (states (state p1 :kind token :initial 1)) (actions) (arcs))"#).unwrap();
        let b = run(r#"(schema m (states (state p1 :kind token :initial 2)) (actions) (arcs))"#).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn jsonld_shaped_input_uses_the_real_cid() {
        let doc = r#"{"@type":"PetriNet","places":{"p1":{"@type":"Place"}}}"#;
        let result = run(doc).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["method"], "urdna2015-cidv1");
        assert!(v["cid"].as_str().unwrap().starts_with('z'));
    }
}
