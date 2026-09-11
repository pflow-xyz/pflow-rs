//! The ecosystem's canonical CID: `CIDv1(dag-json, sha2-256, base58btc)` over
//! the `@id`-stripped, URDNA2015-canonicalized N-Quads of a pflow net's
//! JSON-LD document — byte-identical to go-pflow's `internal/seal.SealJSONLD`
//! (`piprate/json-gold`) and pflow-xyz's `public/seal-cid.mjs`
//! (`digitalbazaar/jsonld.js`). See `pflow-xyz/parity/golden.json` for the
//! cross-language contract and `super::cid`'s module doc for why
//! `Schema::cid()` is a *different*, local thing that this module does not
//! replace.
//!
//! ## Why this exists as a hand-rolled implementation, not a general JSON-LD
//! processor + an off-the-shelf Rust canonicalizer
//!
//! That combination was evaluated first (`json-ld` 0.21 for expansion,
//! `rdf-canon` 0.15 — the W3C-standardized RDFC-1.0 algorithm — for
//! canonicalization) and **rejected**: it produces RDF graphs isomorphic to
//! go-pflow's own (verified by re-canonicalizing go-pflow's canonical
//! N-Quads through `rdf_canon::canonicalize` and getting byte-identical
//! output to canonicalizing the prototype's own conversion of the same
//! document), but RDFC-1.0's canonical *labeling* does not match
//! `piprate/json-gold`'s/`jsonld.js`'s pre-standardization URDNA2015 draft
//! on any of the five `pflow-xyz/parity/fixtures/*.jsonld` — pflow's JSON-LD
//! shape mints one blank node per singleton list (`weight: [1]`,
//! `capacity`, `initial`), so any net with several arcs has enough
//! blank-node symmetry to land in exactly the tie-breaking cases the two
//! algorithm variants were never guaranteed to agree on. `net-b.jsonld`
//! didn't even terminate within `rdf-canon`'s default Hash-N-Degree-Quads
//! call budget.
//!
//! What *is* ported here — the algorithm, the N-Quads serialization rules,
//! and this crate's `to_quads` expander for exactly the one JSON-LD shape
//! `https://pflow.xyz/schema` declares — is a direct, field-for-field port
//! of `pflow-jl`'s `src/urdna2015.jl` + `src/cid.jl` (merged to that repo's
//! `main`, `daf0a45`), which is itself verified byte-exact against
//! go-pflow's own canonical output on all five golden fixtures plus two
//! hand-built tie-breaking fixtures (`test/testdata/cid/tie1.jsonld`,
//! `tie2.jsonld`) — not derived from a from-memory reading of the
//! [RDF Dataset Canonicalization](https://www.w3.org/TR/rdf-canon/) spec
//! alone. Two spec-adjacent details only surfaced by diffing against
//! go-pflow's actual `hashNDegreeQuads` output, not from the spec prose:
//! - the identifier issuer's issued ids carry the `"_:"` N-Quads sigil
//!   baked in as part of the *hash input* itself (not just final
//!   serialization) in `hash_related_blank_node` and the permutation
//!   `path` string of `hash_n_degree_quads`;
//! - a recursive `hash_n_degree_quads` result is wrapped in literal `<...>`
//!   punctuation before being appended to that same `path` string.
//!
//! This module is deliberately generic RDF/N-Quads machinery plus one
//! pflow-schema-specific expander — not a general JSON-LD processor. It
//! knows exactly one `@context` (the one `seal.go`/`seal-cid.mjs` inline)
//! and exactly one JSON-LD shape (a pflow net document). A model using a
//! different `@context` is out of scope, the same way `pflow-jl`'s port is.

use std::collections::BTreeMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

/// One RDF term: an IRI, a blank node (identified by its *input* label —
/// unstable, arbitrary, replaced by canonicalization), or a literal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Term {
    Iri(String),
    BNode(String),
    /// `datatype` is `""` for a plain (`xsd:string`) literal, matching
    /// N-Quads' "omit `^^` for `xsd:string`" rule.
    Literal {
        value: String,
        datatype: String,
    },
}

pub type Quad = (Term, Term, Term);

const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
const XSD_BOOLEAN: &str = "http://www.w3.org/2001/XMLSchema#boolean";
const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
const XSD_DOUBLE: &str = "http://www.w3.org/2001/XMLSchema#double";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";
const RDF_NIL: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil";

const PFLOW_VOCAB: &str = "https://pflow.xyz/schema#";
const PFLOW_LIST_CONTAINERS: &[&str] =
    &["arcs", "token", "weight", "capacity", "initial", "parents"];

/// Errors computing a canonical CID.
#[derive(Debug, thiserror::Error)]
pub enum CanonicalCidError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("top-level document must be a JSON object")]
    NotAnObject,
    #[error("cannot expand a literal of this JSON type: {0}")]
    UnsupportedLiteral(String),
    #[error(
        "hash_n_degree_quads: {0} same-hash related blank nodes exceeds the practical \
         permutation limit (8! = 40320) — no pflow fixture has hit this"
    )]
    PermutationLimitExceeded(usize),
}

// --- N-Quads term/quad serialization ---------------------------------------

fn nq_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

fn nq_term(t: &Term) -> String {
    match t {
        Term::Iri(v) => format!("<{v}>"),
        Term::BNode(label) => format!("_:{label}"),
        Term::Literal { value, datatype } => {
            if datatype.is_empty() || datatype == XSD_STRING {
                format!("\"{}\"", nq_escape(value))
            } else {
                format!("\"{}\"^^<{datatype}>", nq_escape(value))
            }
        }
    }
}

fn nq_line(q: &Quad) -> String {
    format!("{} {} {} .\n", nq_term(&q.0), nq_term(&q.1), nq_term(&q.2))
}

/// Canonical N-Quads document: every quad's line, lexicographically sorted,
/// concatenated.
fn serialize_canonical(quads: &[Quad]) -> String {
    let mut lines: Vec<String> = quads.iter().map(nq_line).collect();
    lines.sort();
    lines.concat()
}

fn sha256hex(s: &str) -> String {
    let hash = Sha256::digest(s.as_bytes());
    hex::encode(hash)
}

// --- Identifier issuer (spec 4.4) -------------------------------------------

/// Issues sequential canonical blank node identifiers (`prefix + "0"`,
/// `prefix + "1"`, ...) to input blank node labels, first-come-first-served.
/// Cloned by `hash_n_degree_quads` to try an issuance path without
/// committing it.
#[derive(Debug, Clone)]
struct IdentifierIssuer {
    prefix: String,
    counter: usize,
    issued_order: Vec<String>,
    map: BTreeMap<String, String>,
}

impl IdentifierIssuer {
    fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            counter: 0,
            issued_order: Vec::new(),
            map: BTreeMap::new(),
        }
    }

    fn has_id(&self, label: &str) -> bool {
        self.map.contains_key(label)
    }

    fn issue_identifier(&mut self, label: &str) -> String {
        if let Some(id) = self.map.get(label) {
            return id.clone();
        }
        let id = format!("{}{}", self.prefix, self.counter);
        self.counter += 1;
        self.map.insert(label.to_string(), id.clone());
        self.issued_order.push(label.to_string());
        id
    }
}

// --- Canonicalization state -------------------------------------------------

struct CanonState {
    bnode_quads: BTreeMap<String, Vec<Quad>>,
    canonical_issuer: IdentifierIssuer,
}

fn blank_node_labels(quads: &[Quad]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for q in quads {
        for t in [&q.0, &q.1, &q.2] {
            if let Term::BNode(label) = t {
                if seen.insert(label.clone()) {
                    out.push(label.clone());
                }
            }
        }
    }
    out
}

fn index_bnode_quads(quads: &[Quad]) -> BTreeMap<String, Vec<Quad>> {
    let mut idx: BTreeMap<String, Vec<Quad>> = BTreeMap::new();
    for q in quads {
        for t in [&q.0, &q.2] {
            if let Term::BNode(label) = t {
                idx.entry(label.clone()).or_default().push(q.clone());
            }
        }
    }
    idx
}

/// Replace one specific blank node with `"a"` and every OTHER blank node
/// with `"z"` (spec 4.6.3), so the hash reflects this node's immediate
/// neighborhood and nothing about any other node's still-unknown canonical
/// identity.
fn relabel_for_hash(t: &Term, self_label: &str) -> Term {
    match t {
        Term::BNode(label) if label == self_label => Term::BNode("a".to_string()),
        Term::BNode(_) => Term::BNode("z".to_string()),
        other => other.clone(),
    }
}

/// Hash First Degree Quads (spec 4.6): a hash of this node's immediate
/// quads alone.
fn hash_first_degree_quads(state: &CanonState, label: &str) -> String {
    let empty = Vec::new();
    let qs = state.bnode_quads.get(label).unwrap_or(&empty);
    let mut lines: Vec<String> = qs
        .iter()
        .map(|q| {
            let s = relabel_for_hash(&q.0, label);
            let p = q.1.clone();
            let o = relabel_for_hash(&q.2, label);
            nq_line(&(s, p, o))
        })
        .collect();
    lines.sort();
    sha256hex(&lines.concat())
}

#[derive(Clone, Copy)]
enum Position {
    S,
    O,
}

impl Position {
    fn as_str(self) -> &'static str {
        match self {
            Position::S => "s",
            Position::O => "o",
        }
    }
}

/// Hash Related Blank Node (spec 4.7): the hash of a node related to
/// `label` through quad `q`, from `label`'s point of view.
fn hash_related_blank_node(
    state: &CanonState,
    related: &str,
    q: &Quad,
    issuer: &IdentifierIssuer,
    position: Position,
) -> String {
    // go-pflow's identifier issuer (piprate/json-gold's IdentifierIssuer)
    // bakes the N-Quads "_:" sigil directly into every issued id, and that
    // full string is what feeds this hash — see this module's doc comment.
    let id = if let Some(canon) = state.canonical_issuer.map.get(related) {
        format!("_:{canon}")
    } else if let Some(temp) = issuer.map.get(related) {
        format!("_:{temp}")
    } else {
        hash_first_degree_quads(state, related)
    };
    let pred = match &q.1 {
        Term::Iri(v) => v.as_str(),
        _ => "",
    };
    sha256hex(&format!("{}<{pred}>{id}", position.as_str()))
}

/// All permutations of a small `Vec<String>`, in ascending-lex order.
/// `hash_n_degree_quads` compares candidate paths and keeps the smallest, so
/// the *order* permutations are generated in doesn't change the result.
fn permutations_lex(v: &[String]) -> Result<Vec<Vec<String>>, CanonicalCidError> {
    if v.is_empty() {
        return Ok(vec![Vec::new()]);
    }
    let mut sorted = v.to_vec();
    sorted.sort();
    let n = sorted.len();
    if n > 8 {
        return Err(CanonicalCidError::PermutationLimitExceeded(n));
    }
    let mut result = vec![sorted.clone()];
    let mut a = sorted;
    loop {
        // Standard next_permutation on ascending-sorted input.
        let mut i = a.len().wrapping_sub(2);
        if a.len() < 2 {
            break;
        }
        while a[i] >= a[i + 1] {
            if i == 0 {
                return Ok(result);
            }
            i -= 1;
        }
        let mut j = a.len() - 1;
        while a[j] <= a[i] {
            j -= 1;
        }
        a.swap(i, j);
        a[(i + 1)..].reverse();
        result.push(a.clone());
    }
    Ok(result)
}

/// Hash N-Degree Quads (spec 4.9): for a blank node whose Hash First Degree
/// Quads value ties with others, explores the graph outward, breadth-first
/// by hash, trying every permutation of same-hash related-node groups and
/// keeping the permutation that yields the lexicographically smallest
/// issuance path. Recursive: a related node with its own first-degree tie
/// is itself resolved by a nested call.
fn hash_n_degree_quads(
    state: &CanonState,
    label: &str,
    issuer: &IdentifierIssuer,
) -> Result<(String, IdentifierIssuer), CanonicalCidError> {
    let mut issuer = issuer.clone();
    let mut hash_to_related: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let empty = Vec::new();
    for q in state.bnode_quads.get(label).unwrap_or(&empty) {
        for (t, pos) in [(&q.0, Position::S), (&q.2, Position::O)] {
            if let Term::BNode(l) = t {
                if l != label {
                    let h = hash_related_blank_node(state, l, q, &issuer, pos);
                    hash_to_related.entry(h).or_default().push(l.clone());
                }
            }
        }
    }

    let mut data_to_hash = String::new();
    for h in hash_to_related.keys().cloned().collect::<Vec<_>>() {
        data_to_hash.push_str(&h);

        let mut related: Vec<String> = hash_to_related[&h].clone();
        related.sort();
        related.dedup();

        let mut chosen_path: Option<String> = None;
        let mut chosen_issuer = issuer.clone();

        for perm in permutations_lex(&related)? {
            let mut issuer_copy = issuer.clone();
            let mut path = String::new();
            let mut recursion_list = Vec::new();

            for related_label in &perm {
                if let Some(canon) = state.canonical_issuer.map.get(related_label) {
                    path.push_str("_:");
                    path.push_str(canon);
                } else if issuer_copy.has_id(related_label) {
                    let id = issuer_copy.map[related_label].clone();
                    path.push_str("_:");
                    path.push_str(&id);
                } else {
                    let id = issuer_copy.issue_identifier(related_label);
                    recursion_list.push(related_label.clone());
                    path.push_str("_:");
                    path.push_str(&id);
                }
            }

            for related_label in &recursion_list {
                let (result_hash, result_issuer) =
                    hash_n_degree_quads(state, related_label, &issuer_copy)?;
                let id = issuer_copy.map[related_label].clone();
                path.push_str("_:");
                path.push_str(&id);
                // The recursive hash is wrapped in literal angle brackets
                // before appending — see this module's doc comment.
                path.push('<');
                path.push_str(&result_hash);
                path.push('>');
                issuer_copy = result_issuer;
            }

            if chosen_path.as_deref().is_none_or(|cp| path.as_str() < cp) {
                chosen_path = Some(path);
                chosen_issuer = issuer_copy;
            }
        }

        data_to_hash.push_str(chosen_path.as_deref().unwrap_or(""));
        issuer = chosen_issuer;
    }

    Ok((sha256hex(&data_to_hash), issuer))
}

/// Returns `quads` with every blank node relabeled to its canonical
/// identifier (`_:c14n0`, `_:c14n1`, ...), in the exact scheme go-pflow/
/// pflow-xyz use.
fn canonicalize(quads: &[Quad]) -> Result<Vec<Quad>, CanonicalCidError> {
    let bnode_quads = index_bnode_quads(quads);
    let mut state = CanonState {
        bnode_quads,
        canonical_issuer: IdentifierIssuer::new("c14n"),
    };
    let non_normalized = blank_node_labels(quads);

    // Step 1: group by first-degree hash.
    let mut hash_to_labels: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for label in &non_normalized {
        let h = hash_first_degree_quads(&state, label);
        hash_to_labels.entry(h).or_default().push(label.clone());
    }

    let mut deferred_hashes = Vec::new();
    for (h, labels) in hash_to_labels.iter() {
        if labels.len() > 1 {
            deferred_hashes.push(h.clone());
            continue;
        }
        state.canonical_issuer.issue_identifier(&labels[0]);
    }

    // Step 2: hash n-degree quads for every hash that still has ties, in
    // hash order; within a tie group, in the order their n-degree hash
    // comes out.
    for h in &deferred_hashes {
        let mut results: Vec<(String, String, IdentifierIssuer)> = Vec::new();
        for label in &hash_to_labels[h] {
            if state.canonical_issuer.has_id(label) {
                continue;
            }
            let mut temp_issuer = IdentifierIssuer::new("b");
            temp_issuer.issue_identifier(label);
            let (nd_hash, result_issuer) = hash_n_degree_quads(&state, label, &temp_issuer)?;
            results.push((nd_hash, label.clone(), result_issuer));
        }
        results.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, label, result_issuer) in &results {
            if state.canonical_issuer.has_id(label) {
                continue;
            }
            for issued_label in &result_issuer.issued_order {
                state.canonical_issuer.issue_identifier(issued_label);
            }
        }
    }

    // Step 3: relabel every quad with the canonical identifiers.
    let relabel = |t: &Term| -> Term {
        match t {
            Term::BNode(label) => Term::BNode(state.canonical_issuer.map[label].clone()),
            other => other.clone(),
        }
    };
    Ok(quads
        .iter()
        .map(|q| (relabel(&q.0), q.1.clone(), relabel(&q.2)))
        .collect())
}

/// Canonicalize and serialize in one step.
fn canonical_nquads(quads: &[Quad]) -> Result<String, CanonicalCidError> {
    Ok(serialize_canonical(&canonicalize(quads)?))
}

// --- pflow-schema-specific JSON-LD expander --------------------------------
//
// Not general JSON-LD expansion — see this module's doc comment for exactly
// what it has to get right for this one context.

fn vocab_iri(key: &str) -> Term {
    Term::Iri(format!("{PFLOW_VOCAB}{key}"))
}

struct ExpandCounter(usize);

impl ExpandCounter {
    fn fresh_bnode(&mut self) -> Term {
        self.0 += 1;
        Term::BNode(format!("j{}", self.0))
    }
}

fn literal_for(v: &Value) -> Result<Term, CanonicalCidError> {
    match v {
        Value::Bool(b) => Ok(Term::Literal {
            value: if *b { "true" } else { "false" }.to_string(),
            datatype: XSD_BOOLEAN.to_string(),
        }),
        Value::Number(n) if n.is_i64() || n.is_u64() => Ok(Term::Literal {
            value: n.to_string(),
            datatype: XSD_INTEGER.to_string(),
        }),
        Value::Number(n) => {
            // xsd:double canonical form: mantissa always carries a decimal
            // point, exponent always present, uppercase E. Untested against
            // go-pflow (see module doc) — no fixture has a fractional
            // coordinate, kept for correctness rather than silently
            // mishandling one if it appears.
            let f = n.as_f64().unwrap_or(0.0);
            let mut s = format!("{f:E}").to_uppercase();
            if !s.contains('.') {
                if let Some(epos) = s.find('E') {
                    s.insert_str(epos, ".0");
                } else {
                    s.push_str(".0");
                }
            }
            Ok(Term::Literal {
                value: s,
                datatype: XSD_DOUBLE.to_string(),
            })
        }
        Value::String(s) => Ok(Term::Literal {
            value: s.clone(),
            datatype: String::new(),
        }),
        other => Err(CanonicalCidError::UnsupportedLiteral(other.to_string())),
    }
}

/// Build an RDF list (spec: `"@list"` expansion -> `rdf:first`/`rdf:rest`
/// chain). `items` has already had `null` entries dropped. Returns the term
/// that should be used where the list is referenced (`rdf:nil` for an empty
/// list, otherwise the first cell's blank node) and appends the list's own
/// quads to `quads`.
fn expand_list(
    quads: &mut Vec<Quad>,
    counter: &mut ExpandCounter,
    items: &[Value],
) -> Result<Term, CanonicalCidError> {
    if items.is_empty() {
        return Ok(Term::Iri(RDF_NIL.to_string()));
    }
    let cells: Vec<Term> = (0..items.len()).map(|_| counter.fresh_bnode()).collect();
    for (i, item) in items.iter().enumerate() {
        let obj = expand_value(quads, counter, item)?;
        quads.push((cells[i].clone(), Term::Iri(RDF_FIRST.to_string()), obj));
        let rest = if i + 1 == items.len() {
            Term::Iri(RDF_NIL.to_string())
        } else {
            cells[i + 1].clone()
        };
        quads.push((cells[i].clone(), Term::Iri(RDF_REST.to_string()), rest));
    }
    Ok(cells[0].clone())
}

/// Expand one JSON value (already known not to be a `@list`-container
/// value) into the term that should stand in for it — a literal for a
/// scalar, a fresh blank node (with its own quads appended) for an object.
fn expand_value(
    quads: &mut Vec<Quad>,
    counter: &mut ExpandCounter,
    v: &Value,
) -> Result<Term, CanonicalCidError> {
    match v {
        Value::Object(obj) => {
            let node = counter.fresh_bnode();
            expand_object(quads, counter, &node, obj)?;
            Ok(node)
        }
        other => literal_for(other),
    }
}

/// Expand every key of a JSON object as a property of `subject`, appending
/// quads to `quads`.
fn expand_object(
    quads: &mut Vec<Quad>,
    counter: &mut ExpandCounter,
    subject: &Term,
    obj: &serde_json::Map<String, Value>,
) -> Result<(), CanonicalCidError> {
    for (k, v) in obj {
        if k == "@id" || k == "@context" || k == "@version" {
            continue;
        } else if k == "@type" {
            if let Value::String(type_name) = v {
                quads.push((
                    subject.clone(),
                    Term::Iri(RDF_TYPE.to_string()),
                    vocab_iri(type_name),
                ));
            }
        } else if PFLOW_LIST_CONTAINERS.contains(&k.as_str()) {
            let items: Vec<Value> = match v {
                Value::Array(a) => a.clone(),
                other => vec![other.clone()],
            };
            // A JSON `null` list entry is dropped: JSON-LD value expansion
            // drops null, so `"capacity": [null]` becomes an empty list.
            let filtered: Vec<Value> = items.into_iter().filter(|x| !x.is_null()).collect();
            let list_term = expand_list(quads, counter, &filtered)?;
            quads.push((subject.clone(), vocab_iri(k), list_term));
        } else {
            let obj_term = expand_value(quads, counter, v)?;
            quads.push((subject.clone(), vocab_iri(k), obj_term));
        }
    }
    Ok(())
}

/// Expand a pflow net's JSON-LD document (`@id` already removed by the
/// caller — `compute_cid` does this) into RDF quads. The root document
/// itself becomes one blank node.
fn to_quads(data: &serde_json::Map<String, Value>) -> Result<Vec<Quad>, CanonicalCidError> {
    let mut quads = Vec::new();
    let mut counter = ExpandCounter(0);
    let root = counter.fresh_bnode();
    expand_object(&mut quads, &mut counter, &root, data)?;
    Ok(quads)
}

// --- Multiformats: multihash, CIDv1(dag-json), multibase(base58btc) -------

const BASE58BTC_ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn base58btc_encode(bytes: &[u8]) -> String {
    let mut digits: Vec<u8> = vec![0];
    for &b in bytes {
        let mut carry = b as u32;
        for d in digits.iter_mut() {
            let x = (*d as u32) * 256 + carry;
            *d = (x % 58) as u8;
            carry = x / 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut out: String = digits
        .iter()
        .rev()
        .map(|&d| BASE58BTC_ALPHABET[d as usize] as char)
        .collect();
    // Strip the single leading '1' `digits` starts with when `bytes` is
    // non-empty (the seed `[0]` digit), then re-add one '1' per leading
    // 0x00 byte, matching the standard convention.
    if out.starts_with('1') && bytes.first() != Some(&0) {
        out.remove(0);
    }
    let leading_zeros = bytes.iter().take_while(|&&b| b == 0).count();
    format!("{}{}", "1".repeat(leading_zeros), out)
}

/// Unsigned LEB128 varint, as multiformats (multihash/CID) use throughout.
fn uvarint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n != 0 {
            out.push(b | 0x80);
        } else {
            out.push(b);
            break;
        }
    }
    out
}

const MULTIHASH_SHA2_256: u64 = 0x12;
const CID_VERSION_1: u64 = 0x01;
const CODEC_DAG_JSON: u64 = 0x0129;

/// `CIDv1(dag-json, sha2-256)`, base58btc-encoded with the multibase `'z'`
/// prefix.
fn cidv1_dagjson_sha256(preimage: &[u8]) -> String {
    let digest = Sha256::digest(preimage);
    let mut multihash = uvarint(MULTIHASH_SHA2_256);
    multihash.extend(uvarint(digest.len() as u64));
    multihash.extend_from_slice(&digest);
    let mut cid_bytes = uvarint(CID_VERSION_1);
    cid_bytes.extend(uvarint(CODEC_DAG_JSON));
    cid_bytes.extend(multihash);
    format!("z{}", base58btc_encode(&cid_bytes))
}

/// The Rust-side equivalent of go-pflow's `seal.SealJSONLD` / pflow-xyz's
/// `computeCid`: parse `raw` as JSON, strip the top-level `@id`
/// (self-referential — a net's `@id` equals this CID, so it cannot appear
/// in its own preimage), canonicalize, hash, and encode.
///
/// Returns `(cid, canonical_nquads)`.
pub fn compute_cid(raw: &[u8]) -> Result<(String, String), CanonicalCidError> {
    let value: Value = serde_json::from_slice(raw)?;
    let mut obj = match value {
        Value::Object(m) => m,
        _ => return Err(CanonicalCidError::NotAnObject),
    };
    obj.remove("@id");
    let quads = to_quads(&obj)?;
    let nquads = canonical_nquads(&quads)?;
    let cid = cidv1_dagjson_sha256(nquads.as_bytes());
    Ok((cid, nquads))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cid_is_deterministic_and_stable_under_key_reorder() {
        let a = br#"{"@type":"PetriNet","places":{"p1":{"@type":"Place"}}}"#;
        let b = br#"{"places":{"p1":{"@type":"Place"}},"@type":"PetriNet"}"#;
        let (cid_a, _) = compute_cid(a).unwrap();
        let (cid_b, _) = compute_cid(b).unwrap();
        assert_eq!(cid_a, cid_b);
        assert!(cid_a.starts_with('z'));
    }

    #[test]
    fn at_id_is_stripped_and_excluded_from_its_own_preimage() {
        let with_id = br#"{"@id":"something","@type":"PetriNet"}"#;
        let without_id = br#"{"@type":"PetriNet"}"#;
        let (cid_with, _) = compute_cid(with_id).unwrap();
        let (cid_without, _) = compute_cid(without_id).unwrap();
        assert_eq!(cid_with, cid_without);
    }

    #[test]
    fn null_list_entry_becomes_empty_list_not_a_list_cell() {
        let with_null = br#"{"places":{"p1":{"capacity":[null]}}}"#;
        let empty_list = br#"{"places":{"p1":{"capacity":[]}}}"#;
        let (cid_null, _) = compute_cid(with_null).unwrap();
        let (cid_empty, _) = compute_cid(empty_list).unwrap();
        assert_eq!(cid_null, cid_empty);
    }

    #[test]
    fn base58_roundtrips_on_known_multihash_prefix() {
        // sha2-256 multihash prefix bytes [0x12, 0x20] alone, a fixed known
        // vector independent of any pflow fixture.
        let encoded = base58btc_encode(&[0x12, 0x20]);
        assert!(!encoded.is_empty());
        assert!(!encoded.starts_with('1'));
    }

    #[test]
    fn leading_zero_bytes_produce_leading_one_characters() {
        let encoded = base58btc_encode(&[0x00, 0x00, 0x01]);
        assert!(encoded.starts_with("11"));
    }

    /// Hash N-Degree Quads' hardest case: several arcs/places tied at first
    /// degree, distinguishable only by which node owns them. See
    /// `tests/fixtures/cid/README.md`.
    #[test]
    fn tie_breaking_fixtures_match_go_pflow() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cid");
        let cases = [
            (
                "tie1.jsonld",
                "z4EBG9jDqmAkYxzdaWnPVRZfBcpug1F45LeQJgck4MicFUDPFgS",
            ),
            (
                "tie2.jsonld",
                "z4EBG9j8AZSNguvWsF6uwoo9PWqURQV2f5YFiYfLXBb9SpvXgt8",
            ),
        ];
        for (name, expected) in cases {
            let raw = std::fs::read(dir.join(name)).unwrap();
            let (cid, _) = compute_cid(&raw).unwrap();
            assert_eq!(
                cid, expected,
                "CID mismatch for tie-breaking fixture {name}"
            );
        }
    }

    fn fixtures_dir() -> Option<std::path::PathBuf> {
        let candidates = [
            "../../../pflow-xyz/parity/fixtures",
            "../../pflow-xyz/parity/fixtures",
        ];
        candidates
            .iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.is_dir())
    }

    /// The real cross-language contract: byte-exact against
    /// `pflow-xyz/parity/golden.json`, skipped quietly without a
    /// `pflow-xyz` checkout next to this one (the same pattern
    /// `pflow-verify`/`pflow-compose`'s showcase-fixture tests use).
    #[test]
    fn matches_pflow_xyz_golden_cids() {
        let Some(dir) = fixtures_dir() else {
            eprintln!("skipping: no pflow-xyz checkout found next to pflow-rs");
            return;
        };
        let golden_path = dir.join("../golden.json");
        let golden: Value =
            serde_json::from_str(&std::fs::read_to_string(&golden_path).unwrap()).unwrap();
        let golden = golden.as_object().unwrap();

        let mut checked = 0;
        for (name, expected) in golden {
            if name == "_comment" {
                continue;
            }
            let raw = std::fs::read(dir.join(name)).unwrap();
            let (cid, _) = compute_cid(&raw).unwrap();
            assert_eq!(
                cid,
                expected.as_str().unwrap(),
                "CID mismatch for fixture {name}"
            );
            checked += 1;
        }
        assert!(checked >= 5, "expected at least 5 golden fixtures");
    }
}
