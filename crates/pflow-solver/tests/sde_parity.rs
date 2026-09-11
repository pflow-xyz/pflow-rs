//! Byte-exact parity of the portable chemical Langevin (SDE) engine against
//! the go-pflow goldens.
//!
//! Every `tests/fixtures/sde/*.json` is a byte-identical copy of
//! `go-pflow/stochastic/testdata/sde/<name>.json` (see the README there).
//! Same loader shape as `tests/ssa_parity.rs`: arrays in declaration order,
//! never a map. `gates` and `coffeeshop` are refused fixtures (`diverged:
//! true`) — the `reason`/`caveats` strings are asserted `==`, not just
//! "contains", since `sde.rs`'s `gating_reasons` was rewritten specifically
//! to match go-pflow's `Model.Gating()` wording. `chain`, `sir` and `dimer`
//! carry a normal series and are asserted `==` on every parsed double, same
//! as the SSA goldens. A mismatch is never fixed by regenerating the golden.

use pflow_solver::ssa::sde::{simulate_sde, SdeOptions};
use pflow_solver::ssa::{ArcKind, SsaArc, SsaModel, SsaPlace, SsaTransition};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_NAMES: [&str; 5] = ["chain", "sir", "dimer", "gates", "coffeeshop"];

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sde")
}

fn fixture_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(fixture_dir())
        .expect("tests/fixtures/sde exists")
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

fn load_model(model: &Value) -> SsaModel {
    let places = model["places"]
        .as_array()
        .expect("model.places is an array")
        .iter()
        .map(|p| {
            let o = p.as_object().expect("place object");
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

fn load_options(opts: &Value) -> SdeOptions {
    SdeOptions {
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

    let res = simulate_sde(&model, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));

    let want_diverged = doc
        .get("diverged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert_eq!(
        res.diverged, want_diverged,
        "{name}: diverged flag must match the golden"
    );

    if want_diverged {
        assert_eq!(
            res.reason,
            doc["reason"].as_str().expect("golden reason"),
            "{name}: SimulateSDE's refusal reason must match go-pflow verbatim"
        );
        let want_caveats: Vec<&str> = doc["caveats"]
            .as_array()
            .expect("golden caveats")
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            res.caveats, want_caveats,
            "{name}: SimulateSDE's caveats must match go-pflow verbatim"
        );
        assert!(
            res.values.is_empty() && res.times.len() == opts.samples,
            "{name}: a diverged result still reports the sample grid, never a fabricated series"
        );
        return;
    }

    let expected = &doc["expected"];
    assert_series_eq(
        &res.times,
        expected["times"].as_array().expect("expected.times"),
        &format!("{name}.times"),
    );

    let series = expected["series"].as_object().expect("expected.series");
    let final_ = expected["final"].as_object().expect("expected.final");
    let stddev = res
        .stddev
        .as_ref()
        .expect("every non-diverged fixture uses realizations >= 2");
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
    }
}

fn fixture_names() -> Vec<String> {
    fixture_files()
        .iter()
        .map(|p| p.file_stem().unwrap().to_string_lossy().to_string())
        .collect()
}

/// Every fixture the Go generator produces must be on disk, so the parity
/// test cannot pass vacuously on an empty directory.
#[test]
fn sde_all_fixtures_present() {
    let present = fixture_names();
    for name in FIXTURE_NAMES {
        assert!(
            present.iter().any(|p| p == name),
            "missing fixture {name}.json (have {present:?})"
        );
    }
}

#[test]
fn sde_goldens_are_byte_exact() {
    let files = fixture_files();
    assert!(
        files.len() >= FIXTURE_NAMES.len(),
        "only {} fixture(s) present; the parity test must not pass vacuously",
        files.len()
    );
    for f in &files {
        check_fixture(f);
    }
    eprintln!("checked {} SDE golden(s)", files.len());
}
