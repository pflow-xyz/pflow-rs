//! Exercises `Verifier` against the pflow showcase's `cafe-order.json`
//! (Variation III), the model ROADMAP.md Phase 2 names as its exit
//! criterion ("`petri_verify`-equivalent output for `cafe-order.json`
//! matches Go field for field. Unlocks: order (fully).").
//!
//! `cafe-order.json` is a token-only workflow net — no schedules, stages or
//! kinetics — so it deserializes directly as a `pflow-metamodel::Model` with
//! no gap between what this crate reads and what go-pflow's `verify`
//! package would see on the same file.
//!
//! This is an integration check, not a copied golden: the go-pflow
//! reference output for the full property set is held field for field by
//! `tests/showcase_properties.rs`. The model is
//! `tests/fixtures/showcase/cafe-order.json`, a byte-identical copy of
//! `pflow-xyz/examples/showcase/cafe-order.json` pinned in `pflow-xyz.lock`;
//! a missing copy fails this test, it never skips.
use std::path::PathBuf;

use pflow_metamodel::Model;
use pflow_verify::{Kind, Property, Verifier};

fn showcase_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/showcase/cafe-order.json")
}

#[test]
fn cafe_order_deserializes_and_verifies() {
    let path = showcase_path();
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "vendored {} is missing ({e}); restore it with ./scripts/pflow-xyz-fixtures.sh sync",
            path.display()
        )
    });
    let model: Model = serde_json::from_str(&raw).expect("cafe-order.json deserializes as a Model");

    assert_eq!(model.places.len(), 7);
    assert_eq!(model.transitions.len(), 6);
    assert_eq!(model.arcs.len(), 12);

    let v = Verifier::new(&model);
    let report = v.check(&[
        Property::new(Kind::Bounded),
        Property::new(Kind::Live),
        Property::invariant(
            "new + paid + brewing + ready + picked_up + cancelled + refunded == 1",
        ),
    ]);

    // Bounded: every place is covered by the workflow's own one-state
    // constraint, so this is a structural proof, not an exhaustive search.
    assert_eq!(report.verdicts[0].status, pflow_verify::Status::Proved);
    assert_eq!(report.verdicts[0].method, pflow_verify::Method::Structural);

    // Live: every transition (pay, start_brew, finish_brew, pick_up,
    // cancel, refund) fires on some path from "new".
    assert_eq!(report.verdicts[1].status, pflow_verify::Status::Proved);

    // The model's own declared constraint — "an order is always in exactly
    // one state" — is exactly the workflow's P-invariant, so it proves
    // structurally.
    assert_eq!(report.verdicts[2].status, pflow_verify::Status::Proved);
    assert_eq!(report.verdicts[2].method, pflow_verify::Method::Structural);

    assert!(report.ok);

    // Deadlock-free is checked separately and expected to REFUTE: the
    // shared go-pflow heuristic ("terminal + non-empty marking, given the
    // run started non-empty, is a deadlock" — `reachability/analyzer.go`'s
    // `BuildGraph`) does not distinguish an accepting final state
    // (`picked_up`, `refunded`) from a stuck one, so both terminal places
    // this workflow ends in are flagged. This is go-pflow's own documented
    // behavior, not a gap in this port — see `pflow_reachability::analyzer`.
    let deadlock_free = v.check_one(Property::new(Kind::DeadlockFree));
    assert_eq!(deadlock_free.status, pflow_verify::Status::Refuted);
}
