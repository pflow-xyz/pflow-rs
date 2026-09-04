//! Cross-implementation ODE parity: pflow-solver's Tsit5 against the
//! trajectories go-pflow recorded for the shared fixture corpus.
//!
//! `tests/fixtures/ode/{fixtures,expected}.json` are byte-identical copies of
//! `pflow-xyz/parity/ode/` (see the README there). go-pflow generated
//! `expected.json` with `solver.DefaultOptions()`, which `Options::default_opts`
//! mirrors field for field; pflow-xyz's `petri-solver.js` is held to the same
//! file. Three integrators, one record.
//!
//! Rust matches Go BIT-FOR-BIT, and the test demands it. The `solve` loop is
//! a line-for-line port of go-pflow's: same tableau constants, same stage /
//! weight loop order, same `ucur + (dt*a) * k` and `dt * b[j] * k[j][i]`
//! evaluation order, same error norm and controller. Go on amd64 (GOAMD64=v1,
//! the default) does not fuse multiply-adds, and Rust never contracts, so the
//! arithmetic is identical operation for operation. The step controller's
//! `(1/err).powf(1/6)` is the one call that COULD differ — Rust's `powf` is
//! the platform libm `pow`, Go's `math.Pow` is Go's own implementation — but
//! on this corpus every controller call agreed to the bit (a pure-Python
//! replica using libm `pow` also reproduces Go exactly). If a future
//! platform's `pow` differs by an ulp, the bit-stable fixtures will fail here
//! with a last-bit `t[i]` mismatch, and that is the moment to loosen to a
//! relative bound — not before.
//!
//! The one thing that is NOT deterministic is flux accumulation order: both
//! sides iterate transitions from a hash map (Go's randomized per run, Rust's
//! `HashMap` per process), so a place summing three or more flux terms —
//! `sir`'s `I`: two from `infect`, one from `recover` — can differ in the last
//! bit. go-pflow's generator solves each fixture several times and flags such
//! fixtures `bitStable: false`; those are held to `REL_TOL` instead. Sorting
//! transition labels in both `build_vec_ode_function` and go-pflow's
//! `buildVecODEFunction` would make them exact too.
//!
//! Trap, recorded because it cost an hour: serde_json's default float parser
//! is NOT correctly rounded — `"11.928215568647225"` came back one ulp off
//! and looked like a solver divergence at step 1. The `float_roundtrip`
//! feature (enabled in this crate's dev-dependencies) is required for any
//! bit-exact comparison against JSON goldens.
//!
//! Multi-color fixtures (a `token` list or any per-place `initial` / arc
//! `weight` vector with more than one entry) are skipped: go-pflow and
//! petri-solver.js unfold colors before integrating, and pflow-solver has no
//! color unfolding yet — its `Arc::weight_sum` collapses a `[1, 0]` arc to
//! weight 1 over a summed pool, exactly the failure the `two-color-starved`
//! fixture documents. The skip list is asserted, so gaining unfolding (or a
//! new multi-color fixture upstream) fails loudly rather than silently
//! shrinking the gate.

use std::collections::HashMap;

use pflow_core::net::PetriNet;
use pflow_solver::{methods, ode};
use serde_json::Value;
use sha2::{Digest, Sha256};

const FIXTURES: &str = include_str!("fixtures/ode/fixtures.json");
const EXPECTED: &str = include_str!("fixtures/ode/expected.json");

/// Relative bound for fixtures go-pflow could NOT reproduce bit-for-bit
/// across its own runs (`bitStable: false`, flux-order jitter — see module
/// doc). Bit-stable fixtures are asserted exactly. Measured jitter <= ~1e-14.
const REL_TOL: f64 = 1e-12;

fn rel_err(a: f64, b: f64) -> f64 {
    (a - b).abs() / a.abs().max(b.abs()).max(1.0)
}

fn net_from_json(model: &Value) -> PetriNet {
    let mut net = PetriNet::new();
    if let Some(tokens) = model.get("token").and_then(Value::as_array) {
        net.token = tokens
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
    }
    for (label, p) in model["places"].as_object().expect("places") {
        let initial: Vec<f64> = p
            .get("initial")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        let capacity: Vec<f64> = p
            .get("capacity")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        net.add_place(label.clone(), initial, capacity, 0.0, 0.0, None);
    }
    for label in model["transitions"]
        .as_object()
        .expect("transitions")
        .keys()
    {
        net.add_transition(label.clone(), "default", 0.0, 0.0, None);
    }
    for arc in model["arcs"].as_array().expect("arcs") {
        let weight: Vec<f64> = arc
            .get("weight")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        net.add_arc(
            arc["source"].as_str().unwrap(),
            arc["target"].as_str().unwrap(),
            weight,
            arc.get("inhibitTransition")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        );
    }
    net
}

#[test]
fn expected_json_was_generated_from_this_fixtures_json() {
    let expected: Value = serde_json::from_str(EXPECTED).unwrap();
    let digest = hex(&Sha256::digest(FIXTURES.as_bytes()));
    assert_eq!(
        digest, expected["fixturesSha256"].as_str().unwrap(),
        "fixtures.json does not match the hash expected.json records: re-copy both from pflow-xyz/parity/ode/"
    );
}

/// Number of colors a model declares: the `token` list, or — when unnamed —
/// the widest per-place `initial` / arc `weight` vector (the parity fixtures
/// include `two-color-unnamed`, which declares colors by shape alone).
fn color_count(model: &Value) -> usize {
    let named = model
        .get("token")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let widest_initial = model["places"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .filter_map(|p| p.get("initial").and_then(Value::as_array))
        .map(Vec::len)
        .max()
        .unwrap_or(0);
    let widest_weight = model["arcs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a.get("weight").and_then(Value::as_array))
        .map(Vec::len)
        .max()
        .unwrap_or(0);
    named.max(widest_initial).max(widest_weight).max(1)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn tsit5_matches_go_pflow_trajectories() {
    let fixtures: Value = serde_json::from_str(FIXTURES).unwrap();
    let expected: Value = serde_json::from_str(EXPECTED).unwrap();
    let by_name: HashMap<&str, &Value> = expected["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["name"].as_str().unwrap(), m))
        .collect();

    let opts = ode::Options::default_opts();
    let tsit5 = methods::tsit5();
    let mut compared = 0;
    let mut skipped = Vec::new();

    for entry in fixtures["models"].as_array().unwrap() {
        let name = entry["name"].as_str().unwrap();
        let model = &entry["model"];
        if color_count(model) > 1 {
            skipped.push(name.to_string());
            continue;
        }
        let want = by_name
            .get(name)
            .unwrap_or_else(|| panic!("no expected trajectory for {name}"));

        let net = net_from_json(model);
        let state = net.set_state(None);
        let rates: HashMap<String, f64> = entry["rates"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_f64().unwrap()))
            .collect();
        let rates = net.set_rates(Some(&rates));
        let tspan = [
            entry["tspan"][0].as_f64().unwrap(),
            entry["tspan"][1].as_f64().unwrap(),
        ];
        let prob = ode::Problem::new(net, state, tspan, rates);
        let sol = ode::solve(&prob, &tsit5, &opts);

        let want_t = want["t"].as_array().unwrap();
        assert_eq!(
            sol.t.len() - 1,
            want["steps"].as_u64().unwrap() as usize,
            "{name}: accepted step count (a different count means a step acceptance flipped)"
        );
        assert_eq!(sol.t.len(), want_t.len(), "{name}: trajectory length");

        let bit_stable = want["bitStable"].as_bool().unwrap();
        let mut worst = 0.0f64;
        let mut exact = true;
        let mut check = |ctx: &str, got: f64, w: f64| {
            let e = rel_err(got, w);
            worst = worst.max(e);
            if got.to_bits() != w.to_bits() {
                exact = false;
            }
            if bit_stable {
                assert!(
                    got.to_bits() == w.to_bits(),
                    "{name}: {ctx}: rust {got:?} ({:#018x}) vs go {w:?} ({:#018x}) — bit-stable fixture, must be exact",
                    got.to_bits(),
                    w.to_bits()
                );
            } else {
                assert!(
                    e <= REL_TOL,
                    "{name}: {ctx}: rust {got:?} vs go {w:?} (rel {e:e} > {REL_TOL:e})"
                );
            }
        };
        for (i, wt) in want_t.iter().enumerate() {
            check(&format!("t[{i}]"), sol.t[i], wt.as_f64().unwrap());
            let st = sol.get_state(i).unwrap();
            for place in want["places"].as_array().unwrap() {
                let place = place.as_str().unwrap();
                let got = *st
                    .get(place)
                    .unwrap_or_else(|| panic!("{name}: place {place} missing from Rust state"));
                let w = want["u"][place][i].as_f64().unwrap();
                check(&format!("u[{i}][{place}]"), got, w);
            }
        }
        for (place, w) in want["final"].as_object().unwrap() {
            check(
                &format!("final[{place}]"),
                sol.get_final_state().unwrap()[place],
                w.as_f64().unwrap(),
            );
        }
        println!(
            "{name:<22} {:>4} steps  bitStable(go)={bit_stable:<5}  rust-vs-go exact={exact:<5}  worst rel {worst:.2e}",
            sol.t.len() - 1
        );
        compared += 1;
    }

    println!("skipped (multi-color, no unfolding in pflow-solver): {skipped:?}");
    assert_eq!(
        compared, 5,
        "single-color fixture count changed upstream: extend or re-check this test"
    );
    assert_eq!(
        skipped,
        [
            "two-color-starved",
            "two-color-selective",
            "two-color-coupled",
            "three-color-gap",
            "two-color-unnamed"
        ],
        "the multi-color skip list changed — if pflow-solver gained color unfolding, compare them too"
    );
}
