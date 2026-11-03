//! `bundle_net`, `bundle_prove` and `bundle_check_proof`: tools of this port
//! with no hosted golden (the hosted server has no such tools yet); held by
//! what they must satisfy instead.

use pflow_hosted::call;
use serde_json::{json, Value};

fn example(instances: Value, links: Value) -> Value {
    call("bundle_example", &json!({"instances": instances, "links": links})).unwrap()["bundle"].clone()
}

fn default_bundle() -> Value {
    call("bundle_example", &json!({})).unwrap()["bundle"].clone()
}

fn tok(sub: &str, port: &str, sub2: &str, port2: &str) -> Value {
    json!({"kind":"token","from":{"subnet":sub,"port":port},"to":{"subnet":sub2,"port":port2}})
}

#[test]
fn tools_are_listed() {
    for t in ["bundle_net", "bundle_prove", "bundle_check_proof"] {
        assert!(pflow_hosted::TOOLS.contains(&t), "{t}");
    }
}

#[test]
fn every_net_tool_reads_the_composite() {
    let b = default_bundle();
    let r = call("bundle_net", &json!({"bundle": b})).unwrap();
    assert_eq!(r["ok"], true, "{r}");
    let net = r["net"].clone();
    assert_eq!(r["places"], 21);
    for t in ["check", "info", "invariants", "incidence", "validate", "cid", "export"] {
        call(t, &json!({"net": net})).unwrap_or_else(|e| panic!("{t}: {e}"));
    }
    // verify reads it and agrees with the composition page about deadlocks
    let v = call("verify", &json!({"net": net, "properties": [{"kind": "bounded"}, {"kind": "deadlock-free"}]})).unwrap();
    assert_eq!(v["verdicts"][0]["status"], "proved", "{v}");
    // the incidence matrix is the flat one (rows and columns by id)
    let flat = call("bundle_incidence", &json!({"bundle": b})).unwrap();
    let inc = call("incidence", &json!({"net": net})).unwrap();
    let cell = |m: &Value, p: &str, t: &str| -> i64 {
        let pi = m["places"].as_array().unwrap().iter().position(|x| x.as_str().unwrap().replace(':', "~") == p).unwrap();
        let ti = m["transitions"].as_array().or(m["cols"].as_array()).unwrap().iter().position(|x| x.as_str().unwrap().replace(':', "~") == t).unwrap();
        m["matrix"][pi][ti].as_i64().unwrap()
    };
    let flat_m = json!({"places": flat["rows"], "cols": flat["cols"], "matrix": flat["matrix"]});
    for p in flat["rows"].as_array().unwrap() {
        for t in flat["cols"].as_array().unwrap() {
            let (p, t) = (p.as_str().unwrap().replace(':', "~"), t.as_str().unwrap().replace(':', "~"));
            assert_eq!(cell(&flat_m, &p, &t), cell(&inc, &p, &t), "{p} {t}");
        }
    }
}

#[test]
fn the_cid_of_the_composite_is_stable() {
    let b = default_bundle();
    let cid = |b: &Value| {
        let r = call("bundle_net", &json!({"bundle": b})).unwrap();
        call("cid", &json!({"net": r["net"]})).unwrap()["cid"].as_str().unwrap().to_string()
    };
    assert_eq!(cid(&b), cid(&b));
    let other = example(json!(["orders", "workers"]), json!([]));
    assert_ne!(cid(&b), cid(&other));
}

#[test]
fn guards_are_refused_not_dropped() {
    // a transition guard has no place in the editor's format
    let mut b = default_bundle();
    b["subnets"][0]["model"]["transitions"][0]["guard"] = json!("1 == 1");
    let r = call("bundle_net", &json!({"bundle": b})).unwrap();
    assert_eq!(r["ok"], false);
    assert!(r["unrepresentable"][0].as_str().unwrap().contains("guard"), "{r}");
    assert!(r.get("net").is_none());
}

fn prove(b: &Value) -> Value {
    call("bundle_prove", &json!({"bundle": b})).unwrap()
}

fn check(b: &Value, p: &Value) -> Value {
    call("bundle_check_proof", &json!({"bundle": b, "proof": p})).unwrap()
}

#[test]
fn the_default_composite_is_consistent_and_its_record_checks() {
    let b = default_bundle();
    let p = prove(&b);
    assert_eq!(p["verdict"]["status"], "consistent", "{}", p["says"]);
    assert_eq!(p["claims"][0]["id"], "bounded");
    assert_eq!(p["claims"][0]["status"], "proved");
    // event links share no place: every law of every part is preserved
    assert!(p["laws"].as_array().unwrap().iter().all(|l| l["status"] == "preserved"));
    let c = check(&b, &p);
    assert_eq!(c["valid"], true, "{}", c["failed"]);
    assert!(c["checked"].as_u64().unwrap() > 10);
}

#[test]
fn fusing_two_queues_breaks_a_declared_law_with_a_counterexample() {
    let b = example(json!(["orders", "orders2"]), json!([tok("orders", "out", "orders2", "in")]));
    let p = prove(&b);
    assert_eq!(p["verdict"]["status"], "inconsistent");
    assert_eq!(p["links"][0]["verdict"], "fail");
    let broken: Vec<&Value> = p["laws"].as_array().unwrap().iter().filter(|l| l["holds_on_reachable"] == "refuted").collect();
    assert_eq!(broken.len(), 2);
    // the law is explained: it is half of a larger conserved sum
    assert_eq!(broken[0]["status"], "extended");
    assert!(broken[0]["extended_by"]["z"].is_object());
    assert!(broken[0]["counterexample"]["trace"].is_array());
    assert_eq!(check(&b, &p)["valid"], true);
}

#[test]
fn a_tampered_record_does_not_check() {
    let b = example(json!(["orders", "orders2"]), json!([tok("orders", "out", "orders2", "in")]));
    let good = prove(&b);
    // a law claimed preserved that is not
    let mut p = good.clone();
    p["laws"][0]["status"] = json!("preserved");
    p["laws"][0]["residual"] = json!({});
    let c = check(&b, &p);
    assert_eq!(c["valid"], false);
    // a counterexample trace that does not replay
    let mut p = good.clone();
    p["laws"][0]["counterexample"]["trace"] = json!(["orders2/dequeue"]);
    assert_eq!(check(&b, &p)["valid"], false);
    // a bound claimed on an invariant that is not one
    let default = default_bundle();
    let mut p = prove(&default);
    p["claims"][0]["certificate"]["z"]["orders/items"] = json!(5);
    assert_eq!(check(&default, &p)["valid"], false);
    // a record for a different composition
    let other = example(json!(["orders", "workers"]), json!([]));
    assert_eq!(check(&other, &prove(&default_bundle()))["valid"], false);
    // a deadlock witness that stops short of a dead marking
    let mut p = prove(&default);
    p["claims"][1]["certificate"]["trace"] = json!([]);
    assert_eq!(check(&default, &p)["valid"], false);
}

#[test]
fn shared_accounts_are_explained_not_failed() {
    let b = example(
        json!(["payment", "payment2"]),
        json!([tok("payment", "account:customer", "payment2", "account:customer"), tok("payment", "account:merchant", "payment2", "account:merchant")]),
    );
    let p = prove(&b);
    assert_eq!(p["verdict"]["status"], "consistent_with_notes", "{}", p["says"]);
    assert!(p["links"].as_array().unwrap().iter().all(|l| l["verdict"] == "explained"));
    assert_eq!(check(&b, &p)["valid"], true);
}

#[test]
fn a_search_that_stops_proves_nothing() {
    let b = default_bundle();
    let p = call("bundle_prove", &json!({"bundle": b, "max_states": 5})).unwrap();
    assert_eq!(p["search"]["complete"], false);
    for c in p["claims"].as_array().unwrap() {
        if c["method"].as_str().unwrap().contains("search") {
            assert_ne!(c["status"], "proved", "{c}");
        }
    }
    assert_eq!(call("bundle_check_proof", &json!({"bundle": b, "proof": p})).unwrap()["valid"], true);
}

#[test]
fn arguments_are_checked() {
    let b = default_bundle();
    assert!(call("bundle_prove", &json!({"bundle": b, "max_states": 0})).unwrap_err().contains("max_states"));
    assert!(call("bundle_prove", &json!({"bundle": b, "nope": 1})).unwrap_err().contains("unknown argument"));
    assert!(call("bundle_check_proof", &json!({"bundle": b})).unwrap_err().contains("proof"));
}
