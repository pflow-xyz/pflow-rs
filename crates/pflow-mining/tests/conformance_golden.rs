//! Conformance golden: replays `tests/fixtures/event-log.json` (byte-copied
//! from `pflow-xyz/examples/showcase/fixtures/event-log.json`) against
//! `tests/fixtures/cafe-order.json` (byte-copied from
//! `pflow-xyz/examples/showcase/cafe-order.json`) and checks the result
//! against the numbers the live petri-pilot `petri_conformance` MCP tool
//! reports for the same two files (captured 2026-09-10; the showcase
//! README's "fitness 0.90, precision 0.80" is the same result, rounded).
//!
//! Both fixtures are go-pflow's metamodel JSON shape. `pflow-mining` has no
//! model-JSON reader of its own (that's `pflow-parser`'s job, out of this
//! crate's scope), so this test parses just the fields conformance checking
//! needs directly.

use std::collections::HashMap;
use std::fs;

use pflow_core::PetriNet;
use pflow_eventlog::{Event, EventLog};
use pflow_mining::check_full_conformance;
use serde_json::Value;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

/// Parses go-pflow's metamodel JSON shape (arrays of `{id}` places/
/// transitions, `{from,to}` arcs) into a [`PetriNet`]. Token-only, matching
/// what `cafe-order.json` (showcase variation III) declares.
fn parse_metamodel_net(src: &str) -> PetriNet {
    let doc: Value = serde_json::from_str(src).expect("valid JSON");
    let mut net = PetriNet::new();

    for p in doc["places"].as_array().expect("places array") {
        let id = p["id"].as_str().expect("place id").to_string();
        let initial = p.get("initial").and_then(Value::as_f64).unwrap_or(0.0);
        net.add_place(id, vec![initial], vec![], 0.0, 0.0, None);
    }
    for t in doc["transitions"].as_array().expect("transitions array") {
        let id = t["id"].as_str().expect("transition id").to_string();
        net.add_transition(id.clone(), "default", 0.0, 0.0, Some(id));
    }
    for a in doc["arcs"].as_array().expect("arcs array") {
        let from = a["from"].as_str().expect("arc from").to_string();
        let to = a["to"].as_str().expect("arc to").to_string();
        net.add_arc(from, to, vec![1.0], false);
    }

    net
}

/// Parses the showcase's `event-log.json` shape:
/// `[{"case","activity","timestamp","resource"}]`.
fn parse_showcase_log(src: &str) -> EventLog {
    let events: Vec<HashMap<String, Value>> = serde_json::from_str(src).expect("valid JSON array");
    let mut log = EventLog::new();

    for (i, raw) in events.iter().enumerate() {
        let case = raw["case"].as_str().expect("case").to_string();
        let activity = raw["activity"].as_str().expect("activity").to_string();
        let ts = raw
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|s| pflow_eventlog::parse_timestamp(s, pflow_eventlog::DEFAULT_FORMATS))
            .unwrap_or(i as i64);

        let mut event = Event::new(case, activity, ts);
        if let Some(r) = raw.get("resource").and_then(Value::as_str) {
            event.resource = r.to_string();
        }
        log.add_event(event);
    }

    log.sort_traces();
    log
}

#[test]
fn showcase_conformance_matches_the_pilot_golden() {
    let net = parse_metamodel_net(&fixture("cafe-order.json"));
    let log = parse_showcase_log(&fixture("event-log.json"));

    let result = check_full_conformance(&log, &net);

    assert!(
        (result.fitness.fitness - 0.895_721_925_133_689_9).abs() < 1e-12,
        "fitness = {}",
        result.fitness.fitness
    );
    assert_eq!(result.precision.precision, 0.8);
    assert!(
        (result.f_score - 0.845_159_255_755_282_3).abs() < 1e-12,
        "f_score = {}",
        result.f_score
    );
    assert_eq!(result.fitness.fitting_traces, 3);
    assert_eq!(result.fitness.total_traces, 5);

    let non_fitting: std::collections::HashSet<&str> = result
        .fitness
        .non_fitting_traces()
        .iter()
        .map(|t| t.case_id.as_str())
        .collect();
    assert_eq!(non_fitting, ["order-4", "order-5"].into_iter().collect());
}
