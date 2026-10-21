//! A port of go-pflow's `mining/conformance_gates_test.go` (added in
//! `84ae2a9`, "mining: honour inhibitor and read arcs in conformance
//! replay"): every case, with the same net, log and expected numbers.
//!
//! go-pflow's `isEnabled`/`fireTransitionSilent` take the `petri.PetriNet`;
//! here they take the [`Model`] replay builds from it ([`replay_model`]), so
//! the tests build that model the same way replay does.

use super::*;
use pflow_eventlog::{parse_jsonl_str, JsonlConfig};

/// `gatedModel`: arrive -> q -> serve -> done, where serve is blocked by an
/// inhibitor from "block" and requires a read of "key" (encoded, as
/// metapetri does, as an inhibitor arc drawn transition -> place).
fn gated_model(block_tokens: f64, key_tokens: f64) -> PetriNet {
    let mut net = PetriNet::new();
    net.add_place("src", vec![1.0], vec![], 0.0, 0.0, None);
    net.add_place("q", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("block", vec![block_tokens], vec![], 0.0, 0.0, None);
    net.add_place("key", vec![key_tokens], vec![], 0.0, 0.0, None);
    net.add_place("done", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_transition("t_arrive", "default", 0.0, 0.0, Some("arrive".into()));
    net.add_transition("t_serve", "default", 0.0, 0.0, Some("serve".into()));
    net.add_arc("src", "t_arrive", vec![1.0], false);
    net.add_arc("t_arrive", "q", vec![1.0], false);
    net.add_arc("q", "t_serve", vec![1.0], false);
    net.add_arc("block", "t_serve", vec![1.0], true); // inhibitor: serve only while block is empty
    net.add_arc("t_serve", "key", vec![1.0], true); // read: serve only while key holds >= 1
    net.add_arc("t_serve", "done", vec![1.0], false);
    net
}

const GATED_TRACE: &str = r#"{"case_id": "c1", "activity": "arrive", "timestamp": "2024-01-01T10:00:00Z"}
{"case_id": "c1", "activity": "serve", "timestamp": "2024-01-01T11:00:00Z"}"#;

fn parse_log(jsonl: &str) -> EventLog {
    parse_jsonl_str(jsonl, &JsonlConfig::default()).expect("failed to parse log")
}

fn marking(pairs: &[(&str, i64)]) -> TokenState {
    pairs.iter().map(|&(p, n)| (p.to_string(), n)).collect()
}

/// `TestConformanceGatesAreNotTokens`: a trace through an inhibitor-guarded,
/// read-guarded transition replays perfectly when the gates are satisfied:
/// the inhibitor's empty place is not a missing token, and the read arc's
/// place is neither consumed nor filled.
#[test]
fn conformance_gates_are_not_tokens() {
    let net = gated_model(0.0, 1.0);
    let res = check_conformance(&parse_log(GATED_TRACE), &net);
    assert!(
        res.fitting_traces == 1 && res.missing_tokens == 0,
        "gated trace should fit: fitting={} missing={} results={:?}",
        res.fitting_traces,
        res.missing_tokens,
        res.trace_results
    );
    // The read place must still hold exactly its token after replay, and
    // the inhibitor place must not have been produced into.
    let model = replay_model(&net);
    let mut mk = marking(&[("src", 1), ("q", 0), ("block", 0), ("key", 1), ("done", 0)]);
    fire_transition_silent(&model, "t_arrive", &mut mk);
    fire_transition_silent(&model, "t_serve", &mut mk);
    assert!(
        mk["key"] == 1 && mk["block"] == 0 && mk["done"] == 1,
        "gates moved tokens: {mk:?}"
    );
}

/// `TestConformanceInhibitorBlocks`: the same trace with the inhibitor's
/// place marked does not fit — the gate is real — and the shortfall is one
/// unmet condition, not the token count.
#[test]
fn conformance_inhibitor_blocks() {
    let res = check_conformance(&parse_log(GATED_TRACE), &gated_model(3.0, 1.0));
    assert_eq!(
        res.fitting_traces, 0,
        "inhibited serve should not fit: {:?}",
        res.trace_results
    );
    assert_eq!(
        res.trace_results[0].missing_tokens, 1,
        "a blocked inhibitor is one unmet condition"
    );
    let model = replay_model(&gated_model(0.0, 1.0));
    assert!(
        is_enabled(&model, "t_serve", &marking(&[("q", 1), ("key", 1)])),
        "serve should be enabled with q marked, block empty, key present"
    );
    assert!(
        !is_enabled(&model, "t_serve", &marking(&[("q", 1), ("block", 1), ("key", 1)])),
        "serve should be blocked while block holds a token"
    );
}

/// `TestConformanceReadArcRequiresWithoutConsuming`: a read arc short of its
/// weight is missing tokens, and the transition is not enabled; satisfied,
/// it is neither consumed nor produced.
#[test]
fn conformance_read_arc_requires_without_consuming() {
    let res = check_conformance(&parse_log(GATED_TRACE), &gated_model(0.0, 0.0));
    assert!(
        res.fitting_traces == 0 && res.trace_results[0].missing_tokens == 1,
        "serve without its read token should be missing exactly one: {:?}",
        res.trace_results
    );
    let model = replay_model(&gated_model(0.0, 0.0));
    assert!(
        !is_enabled(&model, "t_serve", &marking(&[("q", 1)])),
        "serve should not be enabled without the read place marked"
    );
}

// -----------------------------------------------------------------------------
// Not in go-pflow's test file: pins the choices `replay_model` makes so the
// shared firing rule reproduces go-pflow's replay numbers (see its doc).
// -----------------------------------------------------------------------------

/// go-pflow's replay never consults capacity, so a full place does not stop
/// a transition from producing into it.
#[test]
fn replay_ignores_capacity_as_go_pflow_does() {
    let mut net = PetriNet::new();
    net.add_place("p", vec![1.0], vec![], 0.0, 0.0, None);
    net.add_place("full", vec![1.0], vec![1.0], 0.0, 0.0, None);
    net.add_transition("t", "default", 0.0, 0.0, Some("t".into()));
    net.add_arc("p", "t", vec![1.0], false);
    net.add_arc("t", "full", vec![1.0], false);
    let model = replay_model(&net);
    assert!(is_enabled(&model, "t", &marking(&[("p", 1), ("full", 1)])));
}

/// A second arc in the same role between the same place and transition
/// replaces the first (go-pflow's `inputs[place] = weight`), and an arc to
/// an undeclared node still demands tokens there.
#[test]
fn replay_model_last_arc_wins_and_dangling_endpoints_are_places() {
    let mut net = PetriNet::new();
    net.add_place("p", vec![3.0], vec![], 0.0, 0.0, None);
    net.add_transition("t", "default", 0.0, 0.0, Some("t".into()));
    net.add_arc("p", "t", vec![1.0], false);
    net.add_arc("p", "t", vec![2.0], false);
    let model = replay_model(&net);
    let mut mk = marking(&[("p", 3)]);
    let (missing, consumed, _) = fire_transition(&model, "t", &mut mk);
    assert_eq!((missing, consumed, mk["p"]), (0, 2, 1));

    net.add_arc("ghost", "t", vec![1.0], false);
    let model = replay_model(&net);
    assert!(!is_enabled(&model, "t", &marking(&[("p", 3)])));
}
