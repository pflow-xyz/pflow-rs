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
//! This is a sibling-repo integration check, not a copied golden: no Go
//! reference output for this exact property set has been captured and
//! pinned here (that would need a `go-pflow` change to emit one, per
//! ROADMAP.md's ground rules — there is nothing to copy yet). It skips
//! quietly when `pflow-xyz` isn't checked out alongside this repo, so CI
//! elsewhere in the ecosystem isn't broken by a path that only resolves on
//! the workspace machine.
use std::path::PathBuf;

use pflow_metamodel::Model;
use pflow_verify::{Kind, Property, Verifier};

fn showcase_path() -> Option<PathBuf> {
    let candidate =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../pflow-xyz/examples/showcase/cafe-order.json");
    candidate.exists().then_some(candidate)
}

#[test]
fn cafe_order_deserializes_and_verifies() {
    let Some(path) = showcase_path() else {
        eprintln!("pflow-xyz not checked out alongside pflow-rs; skipping");
        return;
    };
    let raw = std::fs::read_to_string(path).expect("read cafe-order.json");
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
