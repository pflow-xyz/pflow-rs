//! Byte-exact parity of `Bundle::flatten` against go-pflow's flatten golden.
//!
//! `tests/fixtures/bundle/cafe.flatten.json` is a byte-identical copy of
//! `go-pflow/metamodel/testdata/bundle/cafe.flatten.json`, produced by
//! go-pflow's `cmd/bundle-goldens` flattening the pflow-xyz showcase's own
//! `cafe.bundle.json` (three subnets, seven links spanning all four link
//! kinds) and re-encoding the result as canonical JSON — every object's keys
//! sorted alphabetically at every nesting level, array order left exactly as
//! produced. This test flattens the same source bundle through
//! `pflow_compose::Bundle::flatten`, canonicalizes the result the same way,
//! and asserts the bytes are identical. **Never fixed by regenerating the
//! golden** — a byte difference is a finding about `flatten`.
//!
//! Skips quietly without a `pflow-xyz` checkout as a sibling of this repo
//! (the same source `tests/flatten.rs`'s
//! `cafe_bundle_showcase_fixture_flattens` already reads), since the source
//! bundle lives there, not in this repo.

use pflow_compose::bundle::Bundle;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

fn golden_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bundle/cafe.flatten.json")
}

fn source_bundle_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../pflow-xyz/examples/showcase/cafe.bundle.json")
}

/// Re-encodes JSON with every object's keys sorted alphabetically,
/// recursively, and 2-space indentation — the same trick go-pflow's
/// `cmd/bundle-goldens` `canonicalize` uses (decode into a generic value,
/// `serde_json::Map` already sorts by `BTreeMap` insertion... no: we sort
/// explicitly below, since `serde_json::Map`'s default `IndexMap`-style
/// ordering is insertion order, not alphabetical). Array order is left
/// exactly as produced.
fn canonicalize(value: &Value) -> Vec<u8> {
    let sorted = sort_value(value);
    let text = serde_json::to_string_pretty(&sorted).expect("value serializes");
    // Go's encoding/json formats a whole-number float64 without a trailing
    // ".0" (`20`, not `20.0`); serde_json's ryu-based float writer always
    // keeps the decimal point. Rewrite every bare number TOKEN (never inside
    // a quoted string) accordingly, so a byte diff reflects a difference in
    // the flattened net and not the two languages' float formatters.
    let mut out = go_style_numbers(&text).into_bytes();
    // serde_json's pretty printer emits no trailing newline; go's
    // json.Encoder.Encode always appends exactly one, and
    // cmd/bundle-goldens explicitly re-appends it after trimming — match
    // that.
    out.push(b'\n');
    out
}

/// Rewrites every bare (unquoted) JSON number token ending in `.0` to drop
/// the fractional part — `"20.0"` (inside a string) is left untouched,
/// `20.0` (a number token) becomes `20`. Mirrors Go's `encoding/json` float
/// formatting, which never writes a trailing `.0` for an integral value.
fn go_style_numbers(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
                out.push_str(&text[start..i]);
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                while i < bytes.len()
                    && matches!(bytes[i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    i += 1;
                }
                let tok = &text[start..i];
                out.push_str(tok.strip_suffix(".0").unwrap_or(tok));
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

fn sort_value(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> =
                map.iter().map(|(k, v)| (k.clone(), sort_value(v))).collect();
            let mut out = serde_json::Map::new();
            for (k, v) in sorted {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sort_value).collect()),
        other => other.clone(),
    }
}

#[test]
fn cafe_bundle_flattens_byte_exact_against_go_pflow_golden() {
    let Ok(source) = std::fs::read_to_string(source_bundle_path()) else {
        eprintln!(
            "skip: no pflow-xyz checkout at {}",
            source_bundle_path().display()
        );
        return;
    };
    let bundle: Bundle = serde_json::from_str(&source).expect("cafe.bundle.json parses");
    let flat = bundle.flatten().expect("cafe bundle flattens");

    let got_value = serde_json::to_value(&flat).expect("flattened model serializes");
    let got = canonicalize(&got_value);

    let want = std::fs::read(golden_path()).expect("tests/fixtures/bundle golden exists");

    if got != want {
        let got_s = String::from_utf8_lossy(&got);
        let want_s = String::from_utf8_lossy(&want);
        panic!(
            "flattened cafe.bundle.json does not match go-pflow's golden byte-for-byte.\n\
             --- got ---\n{got_s}\n--- want ---\n{want_s}"
        );
    }
}
