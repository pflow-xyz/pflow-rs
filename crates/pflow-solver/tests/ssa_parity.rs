//! Byte-exact parity of the portable SSA against the go-pflow goldens.
//!
//! Every `tests/fixtures/ssa/*.json` is a byte-identical copy of
//! `go-pflow/stochastic/testdata/portable/<name>.json` (see the README there).
//! The test hand-loads the spec §3.1 subset of the model — arrays in
//! declaration order, never a map — runs `simulate`, and asserts `==` on every
//! parsed double of `times`, each place's `values`/`stddev`, and `final`.
//! A mismatch is never fixed by regenerating the golden.
//!
//! serde_json is compiled with `float_roundtrip` here (dev-dependency only):
//! its default float parser is not correctly rounded and can misread a
//! shortest-round-trip literal by one ulp, which would make a correct port
//! look wrong. `golden_floats_round_trip` proves every number in every fixture
//! parses to the same bits as `str::parse::<f64>` (correctly rounded).

use pflow_solver::ssa::{simulate, ArcKind, SsaArc, SsaModel, SsaOptions, SsaPlace, SsaTransition};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// The fixtures go-pflow's generator produces. Every one must be present:
/// a missing file would let the parity test pass vacuously.
const FIXTURE_NAMES: [&str; 6] = ["chain", "sir", "dimer", "coffeeshop", "gates", "timed"];

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ssa")
}

fn fixture_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(fixture_dir())
        .expect("tests/fixtures/ssa exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort();
    files
}

fn as_i64(v: Option<&Value>, what: &str) -> i64 {
    match v {
        None | Some(Value::Null) => 0,
        Some(v) => v
            .as_i64()
            .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
            .unwrap_or_else(|| panic!("{what}: not an integer: {v}")),
    }
}

fn as_f64(v: &Value, what: &str) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| panic!("{what}: not a number: {v}"))
}

fn reject_unknown(obj: &serde_json::Map<String, Value>, allowed: &[&str], what: &str) {
    for k in obj.keys() {
        assert!(
            allowed.contains(&k.as_str()),
            "{what}: key {k:?} is outside the portable fixture contract"
        );
    }
}

/// Load the spec §3.1 model subset from a fixture's `model` object.
fn load_model(model: &Value) -> SsaModel {
    let places = model["places"]
        .as_array()
        .expect("model.places is an array")
        .iter()
        .map(|p| {
            let o = p.as_object().expect("place object");
            reject_unknown(o, &["id", "initial", "capacity", "kind"], "place");
            if let Some(kind) = o.get("kind") {
                assert_eq!(kind, "token", "portable fixtures carry only token places");
            }
            SsaPlace {
                id: o["id"].as_str().expect("place id").to_string(),
                initial: as_i64(o.get("initial"), "place.initial"),
                capacity: as_i64(o.get("capacity"), "place.capacity"),
            }
        })
        .collect();
    let transitions = model["transitions"]
        .as_array()
        .expect("model.transitions is an array")
        .iter()
        .map(|t| {
            let o = t.as_object().expect("transition object");
            reject_unknown(o, &["id", "rate", "delay"], "transition");
            assert!(o.get("guard").is_none(), "guards are out of scope");
            SsaTransition {
                id: o["id"].as_str().expect("transition id").to_string(),
                rate: o.get("rate").map(|r| as_f64(r, "rate")).unwrap_or(0.0),
                delay: o.get("delay").map(|d| as_f64(d, "delay")).unwrap_or(0.0),
            }
        })
        .collect();
    let arcs = model["arcs"]
        .as_array()
        .expect("model.arcs is an array")
        .iter()
        .map(|a| {
            let o = a.as_object().expect("arc object");
            reject_unknown(o, &["from", "to", "weight", "type", "kinetic"], "arc");
            let kind = match o.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "" => ArcKind::Flow,
                "inhibitor" => ArcKind::Inhibitor,
                "read" => ArcKind::Read,
                other => panic!("unknown arc type {other:?}"),
            };
            SsaArc {
                from: o["from"].as_str().expect("arc from").to_string(),
                to: o["to"].as_str().expect("arc to").to_string(),
                weight: as_i64(o.get("weight"), "arc.weight"),
                kind,
                kinetic: o.get("kinetic").and_then(|k| k.as_bool()).unwrap_or(true),
            }
        })
        .collect();
    SsaModel {
        places,
        transitions,
        arcs,
    }
}

fn load_options(opts: &Value) -> SsaOptions {
    SsaOptions {
        horizon: as_f64(&opts["horizon"], "options.horizon"),
        samples: as_i64(opts.get("samples"), "options.samples") as usize,
        realizations: as_i64(opts.get("realizations"), "options.realizations") as usize,
        seed: opts["seed"]
            .as_u64()
            .expect("options.seed is an unsigned integer"),
    }
}

fn assert_series_eq(actual: &[f64], expected: &[Value], what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: length");
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        let e = as_f64(e, what);
        assert!(
            *a == e,
            "{what}[{i}]: got {a:?} ({:#018x}), want {e:?} ({:#018x})",
            a.to_bits(),
            e.to_bits()
        );
    }
}

fn check_fixture(path: &Path) {
    let name = path.file_stem().unwrap().to_string_lossy().to_string();
    let text = fs::read_to_string(path).unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    let model = load_model(&doc["model"]);
    let opts = load_options(&doc["options"]);
    let expected = &doc["expected"];

    let res = simulate(&model, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));

    assert_eq!(
        res.times.len(),
        opts.samples,
        "{name}: times length == samples"
    );
    assert_series_eq(
        &res.times,
        expected["times"].as_array().expect("expected.times"),
        &format!("{name}.times"),
    );

    let series = expected["series"].as_object().expect("expected.series");
    let mut model_ids: Vec<&str> = model.places.iter().map(|p| p.id.as_str()).collect();
    let mut golden_ids: Vec<&str> = series.keys().map(|k| k.as_str()).collect();
    model_ids.sort();
    golden_ids.sort();
    assert_eq!(model_ids, golden_ids, "{name}: place id sets");
    assert_eq!(
        res.places,
        model
            .places
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>(),
        "{name}: result place order"
    );

    let stddev = res
        .stddev
        .as_ref()
        .expect("realizations >= 2 in every fixture");
    let final_ = expected["final"].as_object().expect("expected.final");
    for (p, id) in res.places.iter().enumerate() {
        let s = &series[id];
        assert_series_eq(
            &res.values[p],
            s["values"].as_array().expect("values"),
            &format!("{name}.{id}.values"),
        );
        assert_series_eq(
            &stddev[p],
            s["stddev"].as_array().expect("stddev"),
            &format!("{name}.{id}.stddev"),
        );
        let f = as_f64(&final_[id], "final");
        assert!(
            res.final_[p] == f,
            "{name}.final.{id}: got {:?}, want {f:?}",
            res.final_[p]
        );
        assert_eq!(
            res.final_[p].to_bits(),
            res.values[p][opts.samples - 1].to_bits(),
            "{name}.final.{id} == values[S-1]"
        );
    }
}

fn fixture_names() -> Vec<String> {
    fixture_files()
        .iter()
        .map(|p| p.file_stem().unwrap().to_string_lossy().to_string())
        .collect()
}

/// Every fixture the spec and the Go generator name must be on disk. This is
/// the guard against `ssa_goldens_are_byte_exact` passing on an empty
/// directory.
#[test]
fn ssa_all_fixtures_present() {
    let present = fixture_names();
    assert!(
        present.len() >= FIXTURE_NAMES.len(),
        "expected at least {} fixtures in {}, found {present:?}",
        FIXTURE_NAMES.len(),
        fixture_dir().display()
    );
    for name in FIXTURE_NAMES {
        assert!(
            present.iter().any(|p| p == name),
            "missing fixture {name}.json (have {present:?})"
        );
    }
}

#[test]
fn ssa_goldens_are_byte_exact() {
    let files = fixture_files();
    assert!(
        files.len() >= FIXTURE_NAMES.len(),
        "only {} fixture(s) present; the parity test must not pass vacuously",
        files.len()
    );
    for f in &files {
        check_fixture(f);
    }
    eprintln!("checked {} SSA golden(s)", files.len());
}

/// Every JSON number token in `text`, in document order, as its source text.
/// A minimal scanner: skips strings (with escapes) and collects maximal runs
/// of number characters starting outside a string.
fn number_tokens(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                while i < bytes.len()
                    && matches!(bytes[i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    i += 1;
                }
                out.push(&text[start..i]);
            }
            _ => i += 1,
        }
    }
    out
}

fn collect_number_bits(v: &Value, out: &mut Vec<u64>) {
    match v {
        Value::Number(n) => out.push(
            n.as_f64()
                .unwrap_or_else(|| panic!("number {n} has no f64 value"))
                .to_bits(),
        ),
        Value::Array(a) => a.iter().for_each(|x| collect_number_bits(x, out)),
        Value::Object(o) => o.values().for_each(|x| collect_number_bits(x, out)),
        _ => {}
    }
}

/// Proof that the golden loader is correctly rounded: the multiset of doubles
/// serde_json produces for a fixture equals the multiset obtained by parsing
/// every number token of the same text with `str::parse::<f64>`, which is
/// correctly rounded by Rust's guarantee. A one-ulp misread by serde_json's
/// fast path would show up as a bit pattern with no counterpart.
#[test]
fn golden_floats_round_trip() {
    let files = fixture_files();
    assert!(!files.is_empty());
    let mut total = 0usize;
    for path in &files {
        let text = fs::read_to_string(path).unwrap();
        let tokens = number_tokens(&text);
        let mut want: Vec<u64> = tokens
            .iter()
            .map(|tok| {
                tok.parse::<f64>()
                    .unwrap_or_else(|e| panic!("{}: {tok:?}: {e}", path.display()))
                    .to_bits()
            })
            .collect();
        let doc: Value = serde_json::from_str(&text).unwrap();
        let mut got = Vec::new();
        collect_number_bits(&doc, &mut got);
        assert_eq!(
            got.len(),
            want.len(),
            "{}: number-token count vs parsed-number count",
            path.display()
        );
        want.sort_unstable();
        got.sort_unstable();
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(
                g,
                w,
                "{}: sorted number #{i}: serde_json {:?} vs str::parse {:?}",
                path.display(),
                f64::from_bits(*g),
                f64::from_bits(*w)
            );
        }
        // Every double also re-serializes to something that parses back to
        // the same bits (shortest round-trip on both sides).
        for &bits in &got {
            let x = f64::from_bits(bits);
            assert_eq!(format!("{x:?}").parse::<f64>().unwrap().to_bits(), bits);
        }
        total += got.len();
    }
    assert!(
        total > 1000,
        "only {total} numbers checked across the goldens"
    );
    eprintln!(
        "round-tripped {total} numbers across {} golden(s)",
        files.len()
    );
}
