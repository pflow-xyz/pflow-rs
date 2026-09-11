//! Content-addressed identity for schemas.
//!
//! **This is not the ecosystem CID.** `Schema::cid()` is a plain
//! `sha256(serde_json::to_string(normalized))` identity hash over
//! `pflow-tokenmodel`'s own DSL-derived `Schema` type — useful on its own for
//! detecting duplicate/renamed schemas within this crate, but it is not
//! `CIDv1(dag-json, sha2-256, base58btc)` over URDNA2015 N-Quads the way
//! pflow-xyz's `internal/seal.SealJSONLD` (Go) and `public/seal-cid.mjs` (JS)
//! compute it. **The ecosystem CID is [`crate::canonical_cid::compute_cid`]
//! — see that module's doc comment for the algorithm, why it's a hand-rolled
//! port rather than a general JSON-LD-processor-plus-off-the-shelf-Rust-
//! canonicalizer pipeline (that combination was evaluated and produces the
//! wrong bytes on graphs with blank-node symmetry, which every real pflow
//! net has), and its provenance (a field-for-field port of `pflow-jl`'s
//! already-byte-exact-verified `src/urdna2015.jl` + `src/cid.jl`).** It
//! takes a raw pflow net JSON-LD document (not a `Schema`), because the
//! ecosystem CID is defined over that document shape, not over this crate's
//! DSL-derived type — the two are different inputs, so `Schema::cid()`
//! staying a local identity hash is not a placeholder for the ecosystem CID,
//! it answers a genuinely different question ("do these two `Schema`s parse
//! to the same structure").

use sha2::{Digest, Sha256};

use crate::schema::Schema;

impl Schema {
    /// Computes a **local** content-addressed identifier for this schema —
    /// `sha256` over the normalized DSL `Schema`, *not* the ecosystem's
    /// JSON-LD/URDNA2015 CID. See this module's doc comment for why.
    pub fn cid(&self) -> String {
        let normalized = self.normalize();
        match serde_json::to_string(&normalized) {
            Ok(data) => {
                let hash = Sha256::digest(data.as_bytes());
                format!("cid:{}", hex::encode(hash))
            }
            Err(_) => String::new(),
        }
    }

    /// Computes a structural fingerprint (ignoring name/version).
    pub fn identity_hash(&self) -> String {
        #[derive(serde::Serialize)]
        struct Structural {
            states: Vec<super::schema::State>,
            actions: Vec<super::schema::Action>,
            arcs: Vec<super::schema::Arc>,
        }

        let structural = Structural {
            states: self.normalize_states(),
            actions: self.normalize_actions(),
            arcs: self.normalize_arcs(),
        };

        match serde_json::to_string(&structural) {
            Ok(data) => {
                let hash = Sha256::digest(data.as_bytes());
                format!("idh:{}", hex::encode(&hash[..16]))
            }
            Err(_) => String::new(),
        }
    }

    fn normalize(&self) -> Schema {
        Schema {
            name: self.name.clone(),
            version: self.version.clone(),
            states: self.normalize_states(),
            actions: self.normalize_actions(),
            arcs: self.normalize_arcs(),
            constraints: Vec::new(),
            events: Vec::new(),
        }
    }

    fn normalize_states(&self) -> Vec<super::schema::State> {
        let mut states = self.states.clone();
        states.sort_by(|a, b| a.id.cmp(&b.id));
        states
    }

    fn normalize_actions(&self) -> Vec<super::schema::Action> {
        let mut actions = self.actions.clone();
        actions.sort_by(|a, b| a.id.cmp(&b.id));
        actions
    }

    fn normalize_arcs(&self) -> Vec<super::schema::Arc> {
        let mut arcs = self.arcs.clone();
        arcs.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.target.cmp(&b.target))
        });
        arcs
    }

    /// Returns true if two schemas have the same CID.
    pub fn equal(&self, other: &Schema) -> bool {
        self.cid() == other.cid()
    }

    /// Returns true if two schemas have the same structure.
    pub fn structurally_equal(&self, other: &Schema) -> bool {
        self.identity_hash() == other.identity_hash()
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::*;

    #[test]
    fn test_cid_deterministic() {
        let mut s1 = Schema::new("test");
        s1.add_token_state("p1", 1);
        s1.add_token_state("p2", 0);
        s1.add_action(Action {
            id: "t1".into(),
            guard: String::new(),
            event_id: String::new(),
            event_bindings: None,
        });

        let mut s2 = Schema::new("test");
        // Add in different order
        s2.add_token_state("p2", 0);
        s2.add_token_state("p1", 1);
        s2.add_action(Action {
            id: "t1".into(),
            guard: String::new(),
            event_id: String::new(),
            event_bindings: None,
        });

        assert_eq!(s1.cid(), s2.cid());
    }

    #[test]
    fn test_identity_hash() {
        let mut s1 = Schema::new("name1");
        s1.version = "v1".into();
        s1.add_token_state("p1", 1);

        let mut s2 = Schema::new("name2");
        s2.version = "v2".into();
        s2.add_token_state("p1", 1);

        // Same structure, different metadata
        assert_eq!(s1.identity_hash(), s2.identity_hash());
        // Different CIDs due to different name/version
        assert_ne!(s1.cid(), s2.cid());
    }

    #[test]
    fn test_cid_not_empty() {
        let mut s = Schema::new("test");
        s.add_token_state("p1", 1);
        let cid = s.cid();
        assert!(cid.starts_with("cid:"));
        assert!(cid.len() > 10);
    }
}
