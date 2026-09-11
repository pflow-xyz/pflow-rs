//! Cross-validates the reachability graph builder against pflow-polyglot's
//! `parity/reachability.golden` — an independent contract for the same
//! coffee-machine net, already checked by twenty programs across six
//! languages (Go, Rust, Python, JavaScript, Bash, Lean) in that repo.
//! ROADMAP.md Phase 2 names this as the second golden, alongside the
//! showcase's `fixtures/properties.json`.
//!
//! `tests/coffee-machine.model.json` and
//! `tests/coffee-machine.reachability.golden` are byte-identical copies of
//! `pflow-polyglot/model.json` and `pflow-polyglot/parity/reachability.golden`
//! at commit `d25f0cf400786fd7431faba6159504e3f075dd65` (2026-08-08), per
//! ROADMAP.md ground rule 1: copied, never hand-edited to pass.
//!
//! pflow-polyglot's `model.json` is a third JSON shape (`places` keyed by
//! id with `offset`/`initial`/`capacity`, arcs with `source`/`target`/
//! `inhibit`) — go-pflow's own reachability graph never reads it either;
//! the coffee machine is polyglot's own fixture. This test converts it into
//! a `pflow-metamodel::Model` directly (a few dozen lines, inline below)
//! rather than adding a fourth parser to the workspace for one fixture.

use std::collections::BTreeMap;
use std::fs;

use pflow_metamodel::schema::{Arc, ArcType, Model, Place, Transition};
use pflow_reachability::{marking, Analyzer};

fn load_model() -> Model {
    let raw = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/coffee-machine.model.json"))
        .expect("read model.json");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("parse model.json");

    let mut places = Vec::new();
    for (id, p) in v["places"].as_object().unwrap() {
        places.push(Place {
            id: id.clone(),
            initial: p.get("initial").and_then(|x| x.as_i64()).unwrap_or(0),
            capacity: p.get("capacity").and_then(|x| x.as_i64()).unwrap_or(0),
            ..Default::default()
        });
    }
    places.sort_by(|a, b| a.id.cmp(&b.id));

    let mut transitions = Vec::new();
    for id in v["transitions"].as_object().unwrap().keys() {
        transitions.push(Transition { id: id.clone(), ..Default::default() });
    }
    transitions.sort_by(|a, b| a.id.cmp(&b.id));

    let place_ids: std::collections::HashSet<String> =
        v["places"].as_object().unwrap().keys().cloned().collect();

    let mut arcs = Vec::new();
    for a in v["arcs"].as_array().unwrap() {
        let inhibit = a.get("inhibit").and_then(|x| x.as_bool()).unwrap_or(false);
        let source = a["source"].as_str().unwrap().to_string();
        let target = a["target"].as_str().unwrap().to_string();

        if inhibit && place_ids.contains(&target) {
            // A transition-to-place inhibitor is go-pflow's own "output-side
            // inhibitor" convention (`reachability/graph.go`'s output test
            // arc, and the same shape `parser.ModelFromJSON` rewrites into an
            // explicit read arc): the place must already hold a token for the
            // transition to fire, and firing does not consume it. Shape B has
            // no transition-to-place read arc, so this is spelled as the
            // equivalent place-to-transition read arc, direction reversed —
            // exactly what go-pflow's own converter does with this shape.
            arcs.push(Arc { from: target, to: source, typ: ArcType::Read, weight: 1, ..Default::default() });
        } else {
            arcs.push(Arc {
                from: source,
                to: target,
                typ: if inhibit { ArcType::Inhibitor } else { ArcType::Normal },
                weight: 1,
                ..Default::default()
            });
        }
    }

    Model { name: "coffee-machine".into(), places, transitions, arcs, ..Default::default() }
}

/// Renders the graph in `reachability.golden`'s format: markings and
/// terminal counts as a header, then one block per state (sorted by
/// canonical marking rendering) listing its enabled transitions in sorted
/// order with their successor markings, then the sorted terminal markings.
fn render_canonical(model: &Model, result: &pflow_reachability::AnalysisResult) -> String {
    let mut states: Vec<_> = result.graph.states_list();
    states.sort_by_key(|s| canonical_key(&s.marking));

    let mut out = String::new();
    out.push_str(&format!("markings    {}\n", states.len()));
    out.push_str(&format!("edges       {}\n", result.edge_count));
    out.push_str(&format!("terminals   {}\n\n", count_terminals(&states)));

    for state in &states {
        out.push_str(&format!("marking {{{}}}\n", canonical_key(&state.marking)));
        if state.enabled.is_empty() {
            out.push_str("  terminal\n");
            continue;
        }
        let mut enabled = state.enabled.clone();
        enabled.sort();
        for trans in &enabled {
            let next = model.fire(trans, &state.marking);
            out.push_str(&format!("  {trans} -> {{{}}}\n", canonical_key(&next)));
        }
    }

    out.push('\n');
    let mut terminals: Vec<String> =
        states.iter().filter(|s| s.enabled.is_empty()).map(|s| canonical_key(&s.marking)).collect();
    terminals.sort();
    for t in terminals {
        out.push_str(&format!("terminal {{{t}}}\n"));
    }
    out
}

fn count_terminals(states: &[&pflow_reachability::State]) -> usize {
    states.iter().filter(|s| s.enabled.is_empty()).count()
}

/// The golden's marking rendering lists only the places holding a token
/// (the net is 1-safe), sorted, comma-joined — matching pflow-polyglot's
/// `petri.Marking.Key`. `marking::render` is close but uses `place:value`
/// pairs for a general (non-1-safe) marking; this net is 1-safe, so the two
/// coincide once the `:1` suffix is stripped.
fn canonical_key(m: &marking::Marking) -> String {
    let mut held: BTreeMap<&str, i64> = BTreeMap::new();
    for (p, &v) in m {
        if v != 0 {
            held.insert(p.as_str(), v);
        }
    }
    held.keys().cloned().collect::<Vec<_>>().join(",")
}

#[test]
fn coffee_machine_reachability_matches_the_polyglot_golden() {
    let model = load_model();
    let analyzer = Analyzer::new(&model).with_max_states(1000);
    let result = analyzer.analyze();

    assert_eq!(result.state_count, 16, "markings");
    assert_eq!(result.edge_count, 26, "edges");
    assert!(result.bounded, "the net is 1-safe by construction (every place has capacity 1)");

    let terminal_markings: Vec<String> = result
        .graph
        .states_list()
        .into_iter()
        .filter(|s| s.enabled.is_empty())
        .map(|s| canonical_key(&s.marking))
        .collect();
    assert_eq!(terminal_markings, vec!["Payment".to_string()]);

    let golden = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/coffee-machine.reachability.golden"
    ))
    .expect("read golden");
    let ours = render_canonical(&model, &result);

    // Compare only the body (markings/edges/terminals + per-state blocks),
    // since the golden's header is hand-written prose this crate does not
    // reproduce.
    let golden_body = golden_body(&golden);
    assert_eq!(ours.trim_end(), golden_body.trim_end(), "reachability graph diverges from pflow-polyglot's golden");
}

fn golden_body(golden: &str) -> String {
    let idx = golden.find("markings    ").expect("golden has a markings header");
    golden[idx..].to_string()
}
