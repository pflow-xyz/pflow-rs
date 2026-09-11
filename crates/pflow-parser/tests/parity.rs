//! Editor-shape golden parity.
//!
//! `tests/fixtures/editor-shape/*.json` are byte-identical copies of
//! `go-pflow/parser/testdata/editor-shape/<name>.json` (see the README
//! there and `go-pflow.lock` at the repo root). Each pins `parsed`,
//! `expanded` and `unfolded` for the golden's `input`. This test replays
//! `input` through `pflow_core::from_json` / `expand_colors` /
//! `pflow_parser::normalize` (for `parsed`/`expanded`) and
//! `pflow_parser::model_from_json` (for `unfolded`), deserializing each
//! golden section into its real Rust type ([`NormalizedNet`], [`Model`])
//! rather than a bare `serde_json::Value` — see the comment on [`Golden`]
//! for why — and comparing with `==`. A mismatch is never fixed by
//! regenerating the golden (ROADMAP.md ground rule 1).

use pflow_metamodel::Model;
use pflow_parser::NormalizedNet;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

// `parsed`/`expanded`/`unfolded` are deserialized into their real Rust types
// rather than compared as `serde_json::Value`: a `serde_json::Number` built
// from an integral literal ("1") and one built from a float that happens to
// be integral (1.0_f64 serializing through `ryu`) are distinct enum variants
// and are *not* `==` even though they mean the same number, which would make
// every x/y/weight coordinate here a false mismatch. Comparing the typed
// structs instead compares the `f64`s themselves.
#[derive(Debug, Deserialize)]
struct Golden {
    input: Value,
    parsed: NormalizedNet,
    expanded: NormalizedNet,
    unfolded: Model,
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/editor-shape")
}

fn fixture_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(fixture_dir())
        .expect("tests/fixtures/editor-shape exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort();
    files
}

/// The fixtures go-pflow's generator produces. Every one must be present: a
/// missing file would let the parity test pass vacuously.
const FIXTURE_NAMES: [&str; 7] = [
    "cafe-showcase",
    "coffee-shop",
    "knapsack",
    "net-a",
    "net-b",
    "tic-tac-toe",
    "with-parents",
];

#[test]
fn all_fixtures_present() {
    let have: Vec<String> = fixture_files()
        .iter()
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    for name in FIXTURE_NAMES {
        assert!(
            have.iter().any(|h| h == name),
            "missing editor-shape golden {name:?}; run go-pflow-goldens.sh sync"
        );
    }
}

#[test]
fn editor_shape_goldens_match() {
    let files = fixture_files();
    assert!(!files.is_empty(), "no editor-shape goldens");

    for path in files {
        let raw = fs::read_to_string(&path).unwrap();
        let golden: Golden = serde_json::from_str(&raw).unwrap_or_else(|e| {
            panic!("{}: failed to parse golden: {e}", path.display());
        });
        let input_bytes = serde_json::to_vec(&golden.input).unwrap();
        let name = path.display();

        let net = pflow_core::from_json(&input_bytes)
            .unwrap_or_else(|e| panic!("{name}: from_json: {e}"));
        let got_parsed = pflow_parser::normalize(&net);
        assert_eq!(
            got_parsed, golden.parsed,
            "{name}: parsed differs from the golden"
        );

        let (expanded_net, _) = net.expand_colors();
        let got_expanded = pflow_parser::normalize(&expanded_net);
        assert_eq!(
            got_expanded, golden.expanded,
            "{name}: expanded differs from the golden"
        );

        let (got_unfolded, _color_map) = pflow_parser::model_from_json(&input_bytes)
            .unwrap_or_else(|e| panic!("{name}: model_from_json: {e}"));
        assert_eq!(
            got_unfolded, golden.unfolded,
            "{name}: unfolded differs from the golden"
        );
    }
}

/// The exit criterion named in ROADMAP.md Phase 0: `cafe.jsonld` (here,
/// `cafe-showcase.json`, the showcase café model) -> `unfolded` equals the
/// golden byte-for-byte after canonical JSON serialisation.
#[test]
fn cafe_showcase_unfolded_matches_golden() {
    let path = fixture_dir().join("cafe-showcase.json");
    let raw = fs::read_to_string(&path).unwrap();
    let golden: Golden = serde_json::from_str(&raw).unwrap();
    let input_bytes = serde_json::to_vec(&golden.input).unwrap();

    let (model, _color_map) = pflow_parser::model_from_json(&input_bytes).unwrap();
    assert_eq!(model, golden.unfolded);
}
