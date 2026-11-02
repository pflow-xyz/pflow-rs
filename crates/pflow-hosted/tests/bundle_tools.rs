//! What the composition tools do that the native goldens do not hold: the
//! hosted tools' argument checks, limits and hints (pflow-xyz
//! `cmd/webserver/mcp_model_read.go`, `mcp_model.go`), worded as there.

use pflow_hosted::call;
use serde_json::{json, Value};

fn example() -> Value {
    call("bundle_example", &json!({})).unwrap()["bundle"].clone()
}

fn err(tool: &str, args: Value) -> String {
    match call(tool, &args) {
        Ok(v) => panic!("{tool} answered {v}"),
        Err(e) => e,
    }
}

#[test]
fn every_tool_is_listed() {
    for t in [
        "bundle_example",
        "bundle_validate",
        "bundle_add_link",
        "bundle_candidates",
        "bundle_flatten",
        "bundle_run",
        "bundle_incidence",
    ] {
        assert!(pflow_hosted::TOOLS.contains(&t), "{t}");
    }
}

#[test]
fn arguments_are_checked() {
    assert_eq!(
        err("bundle_validate", json!({"bundle": example(), "model": {}})),
        "pflow_bundle_validate: unknown argument \"model\"; takes [\"bundle\"]"
    );
    assert_eq!(
        err(
            "bundle_candidates",
            json!({"bundle": example(), "from": "orders:out", "only_ok": "yes"})
        ),
        "pflow_bundle_candidates: argument \"only_ok\" should be a boolean, got \"yes\""
    );
    assert_eq!(
        err(
            "bundle_run",
            json!({"bundle": example(), "script": ["a", 1.0]})
        ),
        "pflow_bundle_run: argument \"script\" should be a list of strings, got [\"a\",1]"
    );
    assert_eq!(
        err("bundle_flatten", json!({})),
        "pflow_bundle_flatten: bundle: a PetriNetBundle (a JSON object with subnets and links) is required; pflow_bundle_example returns one"
    );
    assert_eq!(
        err("bundle_flatten", json!({"bundle": {"subnets": []}})),
        "pflow_bundle_flatten: bundle: needs a non-empty \"subnets\" array"
    );
    assert_eq!(
        err(
            "bundle_add_link",
            json!({"bundle": example(), "kind": "event", "from": "orders:dequeue"})
        ),
        "pflow_bundle_add_link: missing argument \"to\""
    );
}

#[test]
fn a_bundle_is_held_to_its_limits() {
    let mut b = example();
    b["subnets"][0]["model"]["places"][0]["initial"] = json!(9007199254740993u64);
    assert_eq!(
        err("bundle_validate", json!({"bundle": b})),
        "pflow_bundle_validate: bundle: subnets[0].model.places[0].initial is 9007199254740992; a count or weight is at most 9007199254740991 (2^53 - 1), the largest integer JSON-RPC carries exactly (a bigger one would arrive rounded)"
    );
    let mut b = example();
    b["subnets"][1]["net_type"] = json!("Resourcenet");
    assert_eq!(
        err("bundle_validate", json!({"bundle": b})),
        "pflow_bundle_validate: bundle: subnets[1].net_type is \"Resourcenet\"; it is one of \"\", \"WorkflowNet\", \"ResourceNet\", \"GameNet\", \"ComputationNet\", \"ClassificationNet\""
    );
    let mut b = example();
    b["subnets"][0]["id"] = json!("x".repeat(129));
    assert_eq!(
        err("bundle_validate", json!({"bundle": b})),
        "pflow_bundle_validate: bundle: subnets[0].id is 129 bytes; names are at most 128 bytes"
    );
    let mut b = example();
    b["@type"] = json!("PetriNet");
    assert_eq!(
        err("bundle_validate", json!({"bundle": b})),
        "pflow_bundle_validate: bundle: @type is \"PetriNet\", not \"PetriNetBundle\""
    );
}

#[test]
fn what_go_reads_as_zero_is_read() {
    // a null, a missing list, a count written 2.0: the hosted tool reads them
    let b = json!({"subnets": [
        {"id": "a", "net_type": null, "model": {"places": [{"id": "p", "initial": 2.0}], "transitions": [{"id": "t"}], "arcs": [{"from": "p", "to": "t", "weight": 1}]}},
    ]});
    let out = call(
        "bundle_run",
        &json!({"bundle": b, "script": ["a/t", "a/t", "a/t"]}),
    )
    .unwrap();
    assert_eq!(out["stopped_at"], json!(2));
}

#[test]
fn a_missing_port_names_the_ports_there_are() {
    let e = err(
        "bundle_add_link",
        json!({"bundle": example(), "kind": "token", "from": "orders:sideways", "to": "workers:available"}),
    );
    assert_eq!(
        e,
        "pflow_bundle_add_link: [E_BAD_ENDPOINT] links[0]: from: subnet \"orders\" has no port \"sideways\". subnet \"orders\" has the ports \"depth\", \"dequeue\", \"enqueue\", \"in\", \"out\""
    );
    let e = err(
        "bundle_candidates",
        json!({"bundle": example(), "from": "orders:sideways"}),
    );
    assert!(
        e.starts_with("pflow_bundle_candidates: subnet \"orders\" has no port \"sideways\". subnet \"orders\" has the ports "),
        "{e}"
    );
}

#[test]
fn ends_are_ports() {
    let e = err(
        "bundle_add_link",
        json!({"bundle": example(), "kind": "token", "from": {"subnet": "orders", "place": "items"}, "to": "workers:available"}),
    );
    assert_eq!(
        e,
        "pflow_bundle_add_link: endpoint {\"place\":\"items\",\"subnet\":\"orders\"} names a place or a transition: these tools link ports"
    );
}

#[test]
fn run_reads_its_predicates() {
    let out = call(
        "bundle_run",
        &json!({"bundle": example(), "script": ["orders/dequeue"], "expect": [{"place": "orders/items", "n": 1, "says": "one left"}]}),
    )
    .unwrap();
    assert_eq!(out["expect"][0]["pass"], json!(true));
    assert_eq!(
        err(
            "bundle_run",
            json!({"bundle": example(), "script": [], "expect": [{"place": "orders/itemz", "n": 0}]})
        ),
        "pflow_bundle_run: expect[0]: no place \"orders/itemz\" in the composition"
    );
    assert_eq!(
        err("bundle_run", json!({"bundle": example(), "script": [], "expect": [{"place": "orders/items", "n": 0.5}]})),
        "pflow_bundle_run: expect[0] should be {place: \"<subnet>/<place>\", n: an integer below 2^53, says?}, got {\"n\":0.5,\"place\":\"orders/items\"}"
    );
    assert_eq!(
        err(
            "bundle_run",
            json!({"bundle": example(), "scenario": "commit", "expect": []})
        ),
        "pflow_bundle_run: `expect` goes with `script`; a scenario brings its own"
    );
    let long: Vec<&str> = vec!["orders/dequeue"; 1001];
    assert_eq!(
        err("bundle_run", json!({"bundle": example(), "script": long})),
        "pflow_bundle_run: script has 1001 steps; at most 1000"
    );
}

#[test]
fn weightless_arcs_get_a_note() {
    let b = json!({"subnets": [
        {"id": "a", "model": {"name": "a", "places": [{"id": "p", "initial": 1}], "transitions": [{"id": "t"}], "arcs": [{"from": "p", "to": "t"}]}},
    ]});
    let f = call("bundle_flatten", &json!({"bundle": b})).unwrap();
    assert_eq!(
        f["notes"][0].as_str().unwrap().split(':').next(),
        Some("1 of the 1 arcs have no weight")
    );
    let i = call("bundle_incidence", &json!({"bundle": b})).unwrap();
    assert!(i["notes"][0]
        .as_str()
        .unwrap()
        .contains("the flat matrix (1) and the subnets' own blocks (1)"));
}

#[test]
fn an_invalid_bundle_flattens_to_ok_false() {
    let mut b = example();
    b["links"][0]["to"]["port"] = json!("ghost");
    let f = call("bundle_flatten", &json!({"bundle": b})).unwrap();
    assert_eq!(f["ok"], json!(false));
    assert!(f["error"]
        .as_str()
        .unwrap()
        .starts_with("invalid bundle: [E_BAD_ENDPOINT]"));
}

#[test]
fn incidence_is_bounded() {
    let places: Vec<Value> = (0..500).map(|i| json!({"id": format!("p{i}")})).collect();
    let transitions: Vec<Value> = (0..500).map(|i| json!({"id": format!("t{i}")})).collect();
    let b = json!({"subnets": [{"id": "a", "model": {"name": "a", "places": places, "transitions": transitions, "arcs": []}}]});
    let e = err("bundle_incidence", json!({"bundle": b}));
    assert!(e.starts_with("pflow_bundle_incidence: the matrices would have 500000 cells (500 flat places x 500 flat transitions"), "{e}");
}
