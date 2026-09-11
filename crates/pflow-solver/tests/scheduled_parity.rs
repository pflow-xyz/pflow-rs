//! Parity of the scheduled+staged engine (`pflow_solver::stochastic`)
//! against go-pflow's `cmd/scheduled-goldens` golden, plus the one branch of
//! `stochastic.Forecast` this crate ports: the refusal a model-declared
//! schedule gets before any ODE integration is attempted.
//!
//! `tests/fixtures/scheduled/cafe-service.json` is a byte-identical copy of
//! `go-pflow/stochastic/testdata/scheduled/cafe-service.json` — see the
//! README there. Two claims are checked, and they are not the same claim:
//!
//! 1. **The `forecastRefusal` reason string is byte-exact.** go-pflow's
//!    `Forecast` checks `m.HasSchedules()` before touching the ODE at all
//!    and returns a fixed `Result`; that check and that string are cheap to
//!    port in full (`forecast_schedule_refusal` below) and are asserted
//!    `==`, not just "contains".
//! 2. **The scheduled/staged `Result` is compared, not asserted `==`.**
//!    `pflow_solver::stochastic`'s own module doc is explicit that it
//!    "has no byte-parity contract": it reuses this crate's portable
//!    Xoshiro256/`plog` generator for its randomness rather than
//!    reproducing go-pflow's RNG draw order number for number. This test
//!    still runs the golden's exact model/options through it and reports
//!    the shape-level agreement (method, place set, times grid, `Final`
//!    keys) plus how far the actual doubles diverge, so a future run that
//!    *does* land on byte-exact parity is a one-line change away from
//!    promoting the loose checks to `==` — see the `#[test]` doc comments
//!    below for exactly what is and is not asserted today.

use pflow_metamodel::Model;
use pflow_solver::stochastic::{simulate, Options};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scheduled/cafe-service.json")
}

fn load() -> Value {
    let text = fs::read_to_string(fixture_path()).expect("tests/fixtures/scheduled golden exists");
    serde_json::from_str(&text).expect("golden is valid JSON")
}

fn load_model(doc: &Value) -> Model {
    serde_json::from_value(doc["model"].clone()).expect("model parses as pflow_metamodel::Model")
}

/// The one branch of go-pflow's `stochastic.Forecast` this crate ports: the
/// schedule refusal, verbatim (`stochastic/stochastic.go`'s
/// `m.HasSchedules()` branch). The rest of `Forecast` — the continuous
/// mass-action dispatch, `checkDivergence`, the non-schedule `Gating()`
/// refusal — is out of scope for this phase (ROADMAP.md Phase 1, "Not
/// ported in this phase, by design"); this fragment exists only because the
/// golden's `forecastRefusal` field needs somewhere to hold a byte-exact
/// test.
fn forecast_schedule_refusal(m: &Model) -> Option<(String, Vec<String>, String)> {
    if !m.has_schedules() {
        return None;
    }
    Some((
        "this model declares rate schedules, and a continuous solution here integrates one \
         constant rate per transition; the declared day shape would be run flat. Use the \
         discrete engine (Simulate), which honours the schedule segment by segment."
            .to_string(),
        vec!["model-declared schedule: a time-varying rate is not a mass-action constant".to_string()],
        "ode".to_string(),
    ))
}

/// go-pflow's schedule-refusal reason, byte-exact — the string a Rust
/// `Forecast` caller is expected to see for `cafe-service.json`, matched
/// against `forecastRefusal` in the golden rather than a paraphrase.
#[test]
fn forecast_refuses_cafe_service_with_go_pflow_reason() {
    let doc = load();
    let model = load_model(&doc);
    let want = &doc["forecastRefusal"];

    assert_eq!(want["diverged"], true, "golden's own forecastRefusal.diverged");

    let (reason, caveats, method) =
        forecast_schedule_refusal(&model).expect("cafe-service.json declares a schedule");

    assert_eq!(
        reason,
        want["reason"].as_str().unwrap(),
        "forecast refusal reason must match go-pflow verbatim, not a paraphrase"
    );
    let want_caveats: Vec<String> = want["caveats"]
        .as_array()
        .expect("forecastRefusal.caveats")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(caveats, want_caveats);
    assert_eq!(method, want["method"].as_str().unwrap());
}

/// The scheduled/staged engine runs the golden's exact model and options
/// without error and produces the same *shape* of result: same dispatch
/// method, same place set, same sample grid, a `Final` entry for every
/// reported place. This is the part of the claim this crate can make today.
#[test]
fn scheduled_run_matches_go_pflow_shape() {
    let doc = load();
    let model = load_model(&doc);
    assert!(model.has_schedules(), "cafe-service.json declares a schedule");

    let opts_doc = &doc["options"];
    let mut rates: HashMap<String, f64> = HashMap::new();
    if let Some(obj) = opts_doc.get("rates").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            rates.insert(k.clone(), v.as_f64().unwrap());
        }
    }
    let opts = Options {
        horizon: opts_doc["horizon"].as_f64().unwrap(),
        samples: opts_doc["samples"].as_u64().unwrap() as usize,
        realizations: opts_doc["realizations"].as_u64().unwrap() as usize,
        seed: opts_doc["seed"].as_u64().unwrap(),
        rates,
        ..Options::default()
    };

    let res = simulate(&model, &HashMap::new(), &opts).expect("scheduled simulate succeeds");

    let want = &doc["result"];
    assert_eq!(res.method, want["method"].as_str().unwrap());

    let want_times = want["times"].as_array().expect("result.times");
    assert_eq!(res.times.len(), want_times.len(), "sample grid length");
    // The grid itself IS byte-exact: it is `sample_times`, pure arithmetic
    // over horizon/samples with no randomness in it at all.
    for (i, (got, want)) in res.times.iter().zip(want_times).enumerate() {
        let want = want.as_f64().unwrap();
        assert_eq!(*got, want, "times[{i}]");
    }

    let want_series = want["series"].as_array().expect("result.series");
    let mut want_places: Vec<&str> = want_series
        .iter()
        .map(|s| s["place"].as_str().unwrap())
        .collect();
    let mut got_places: Vec<&str> = res.series.iter().map(|s| s.place.as_str()).collect();
    want_places.sort_unstable();
    got_places.sort_unstable();
    assert_eq!(got_places, want_places, "place set");

    let want_final = want["final"].as_object().expect("result.final");
    for place in &want_places {
        assert!(
            res.final_.contains_key(*place),
            "Final is missing place {place:?} go-pflow reports"
        );
        assert!(want_final.contains_key(*place));
    }
}

/// Documents the actual state of numeric parity rather than asserting it:
/// counts how many of the ~780 reported doubles (12 places x 65 samples)
/// this crate's own portable-RNG scheduled engine reproduces bit-for-bit
/// against go-pflow's `Options{Portable: true}` run of the same model,
/// options and seed. `pflow_solver::stochastic`'s module doc already states
/// this engine "has no byte-parity contract" (different RNG draw order from
/// stage/schedule bookkeeping, not just a different generator family), so
/// this test's job is to keep that claim honest rather than to pass or fail
/// on a threshold — see ROADMAP.md's Status table row for `stochastic`
/// stages/schedules, which still reads "unit tests", not this golden, for
/// exactly that reason.
#[test]
fn scheduled_numeric_divergence_is_measured_not_claimed() {
    let doc = load();
    let model = load_model(&doc);

    let opts_doc = &doc["options"];
    let mut rates: HashMap<String, f64> = HashMap::new();
    if let Some(obj) = opts_doc.get("rates").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            rates.insert(k.clone(), v.as_f64().unwrap());
        }
    }
    let opts = Options {
        horizon: opts_doc["horizon"].as_f64().unwrap(),
        samples: opts_doc["samples"].as_u64().unwrap() as usize,
        realizations: opts_doc["realizations"].as_u64().unwrap() as usize,
        seed: opts_doc["seed"].as_u64().unwrap(),
        rates,
        ..Options::default()
    };
    let res = simulate(&model, &HashMap::new(), &opts).expect("scheduled simulate succeeds");

    let want = &doc["result"];
    let want_series = want["series"].as_array().unwrap();

    let mut total = 0usize;
    let mut exact = 0usize;
    for ws in want_series {
        let place = ws["place"].as_str().unwrap();
        let want_values = ws["values"].as_array().unwrap();
        if let Some(got) = res.series.iter().find(|s| s.place == place) {
            for (w, g) in want_values.iter().zip(&got.values) {
                total += 1;
                if w.as_f64().unwrap() == *g {
                    exact += 1;
                }
            }
        }
    }
    eprintln!(
        "scheduled_numeric_divergence_is_measured_not_claimed: {exact}/{total} doubles bit-exact \
         against go-pflow's Options{{Portable: true}} scheduled run"
    );
    assert!(total > 0, "compared zero doubles; the golden or the run is empty");
}
