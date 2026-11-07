//! Bug B5 (pflow-mcp 0.4.0 test report): sealing a net (its CID) was about
//! cubic in net size, 53 s at 200 places, because the URDNA2015 n-degree walk
//! cloned its identifier issuer at every step along an RDF list of tied cells.
//! It is now quadratic. The CIDs below were computed by the cubic version, so
//! these tests hold the fix to the same bytes, and the time bound fails on the
//! old code (debug build: 100 places took 17 s cubic, 3 s now; 150 places
//! about 60 s cubic, 7 s now).
use serde_json::{json, Value};
use std::time::Instant;

fn chain(n: usize) -> Value {
    let places: Vec<_> = (0..n)
        .map(|i| json!({"id": format!("p{i}"), "initial": if i == 0 { 1 } else { 0 }}))
        .collect();
    let transitions: Vec<_> = (0..n - 1).map(|i| json!({"id": format!("t{i}")})).collect();
    let arcs: Vec<_> = (0..n - 1)
        .flat_map(|i| {
            [
                json!({"from": format!("p{i}"), "to": format!("t{i}")}),
                json!({"from": format!("t{i}"), "to": format!("p{}", i + 1)}),
            ]
        })
        .collect();
    json!({"name": format!("chain{n}"), "places": places, "transitions": transitions, "arcs": arcs})
}

fn seal(n: usize) -> String {
    let out =
        pflow_hosted::call("net_to_jsonld", &json!({"model": chain(n), "cid": true})).unwrap();
    out["cid"].as_str().unwrap().to_string()
}

#[test]
fn seal_cids_are_unchanged_by_the_b5_fix() {
    assert_eq!(
        seal(25),
        "z4EBG9j7Y34D91Z6rbCYtA5NhNt7dAmwvmJMMsUDiSBzi1C9sKd"
    );
    assert_eq!(
        seal(50),
        "z4EBG9j89NW7Zfa4ADkTB2beLEmzrqNagDQZAgs7F8f1s4ukBTw"
    );
}

#[test]
fn sealing_150_places_is_not_cubic() {
    assert_eq!(
        seal(100),
        "z4EBG9j6wXgg5Vcre6WFKvokfn9ns2FCnmj4uwCRjERsYPGX2dF"
    );
    let t = Instant::now();
    assert_eq!(
        seal(150),
        "z4EBG9j5bH9EzmYh6NsR7CmWo6Je4izAXKWRw8MgRtKdTsdvpRy"
    );
    let s = t.elapsed().as_secs_f64();
    assert!(
        s < 30.0,
        "sealing a 150-place chain took {s:.1} s (cubic: about 60 s in a debug build)"
    );
}
