//! The Shape A writer against the cross-language CID goldens
//! (`tests/fixtures/cid/`, vendored from pflow-xyz, and the two pflow-jl
//! tie-breaking fixtures). A failing golden is a bug here, never a reason to
//! change the fixture.

use std::path::PathBuf;

use pflow_core::PetriNet;
use pflow_tokenmodel::{compute_cid, seal_shape_a, to_shape_a, to_shape_a_string};
use serde_json::{json, Value};

const TIE1: &str = "z4EBG9jDqmAkYxzdaWnPVRZfBcpug1F45LeQJgck4MicFUDPFgS";
const TIE2: &str = "z4EBG9j8AZSNguvWsF6uwoo9PWqURQV2f5YFiYfLXBb9SpvXgt8";

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cid")
}

/// Every golden: (name, the fixture's bytes, its CID). Fails if one is missing.
fn goldens() -> Vec<(String, Vec<u8>, String)> {
    let golden: Value =
        serde_json::from_slice(&std::fs::read(dir().join("parity/golden.json")).unwrap()).unwrap();
    let mut out: Vec<_> = golden
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| !k.starts_with('_'))
        .map(|(name, cid)| {
            let bytes = std::fs::read(dir().join("parity/fixtures").join(name)).unwrap();
            (name.clone(), bytes, cid.as_str().unwrap().to_string())
        })
        .collect();
    for (name, cid) in [("tie1.jsonld", TIE1), ("tie2.jsonld", TIE2)] {
        out.push((name.to_string(), std::fs::read(dir().join(name)).unwrap(), cid.to_string()));
    }
    assert_eq!(out.len(), 7);
    out
}

fn net_of(bytes: &[u8]) -> PetriNet {
    pflow_core::from_json(bytes).unwrap()
}

fn id_of(doc: &Value) -> &str {
    doc["@id"].as_str().unwrap()
}

/// `net-b` holds nothing a `PetriNet` cannot, so the full read, net, write path
/// comes back with the golden CID go-pflow and pflow-xyz produce.
#[test]
fn a_net_with_nothing_lost_keeps_its_golden_cid() {
    let (_, bytes, cid) = goldens().into_iter().find(|(n, _, _)| n == "net-b.jsonld").unwrap();
    assert_eq!(id_of(&to_shape_a(&net_of(&bytes)).unwrap()), cid);
}

/// The other six each have something the net does not carry (module docs). The
/// written document is the fixture with only those things changed, shown by
/// making the same changes by hand: same CID. So the difference is those fields
/// and nothing else about the writer.
#[test]
fn the_other_goldens_differ_only_by_what_a_net_does_not_carry() {
    fn normalize_list(node: &mut Value, key: &str) {
        if let Some(list) = node.get_mut(key).and_then(Value::as_array_mut) {
            for x in list.iter_mut() {
                if x.is_null() {
                    *x = json!(0);
                }
            }
        }
    }
    let mut compared = 0;
    for (name, bytes, cid) in goldens() {
        if name == "net-b.jsonld" {
            continue;
        }
        let written = to_shape_a(&net_of(&bytes)).unwrap();
        assert_ne!(id_of(&written), cid, "{name} now round-trips: move it to the test above");

        let mut doc: Value = serde_json::from_slice(&bytes).unwrap();
        let fields = doc.as_object_mut().unwrap();
        fields.remove("parents");
        fields.remove("author");
        fields.insert("@version".into(), json!("1.1"));
        fields.entry("token").or_insert(json!([]));
        for p in doc["places"].as_object_mut().unwrap().values_mut() {
            for key in ["capacity", "initial"] {
                p.as_object_mut().unwrap().entry(key).or_insert(json!([]));
                normalize_list(p, key);
            }
        }
        for t in doc["transitions"].as_object_mut().unwrap().values_mut() {
            t.as_object_mut().unwrap().entry("role").or_insert(json!("default"));
        }
        for a in doc["arcs"].as_array_mut().unwrap() {
            a.as_object_mut().unwrap().entry("inhibitTransition").or_insert(json!(false));
        }
        let (projected, _) = compute_cid(&serde_json::to_vec(&doc).unwrap()).unwrap();
        assert_eq!(id_of(&written), projected, "{name}");
        compared += 1;
    }
    assert_eq!(compared, 6);
}

/// Sealing a document as it stands reproduces all seven goldens, whatever `@id`
/// it arrived with.
#[test]
fn sealing_a_document_reproduces_every_golden() {
    for (name, bytes, cid) in goldens() {
        let mut doc: Value = serde_json::from_slice(&bytes).unwrap();
        doc["@id"] = json!("not the cid");
        let sealed = seal_shape_a(doc).unwrap();
        assert_eq!(id_of(&sealed), cid, "{name}");

        // Only @id moved.
        let mut before: Value = serde_json::from_slice(&bytes).unwrap();
        before.as_object_mut().unwrap().remove("@id");
        let mut after = sealed.clone();
        after.as_object_mut().unwrap().remove("@id");
        assert_eq!(before, after, "{name}");

        // The sealed bytes verify, and read back.
        let text = serde_json::to_vec(&sealed).unwrap();
        assert_eq!(compute_cid(&text).unwrap().0, cid, "{name}");
        net_of(&text);
    }
}

#[test]
fn writing_is_stable_across_a_round_trip() {
    for (name, bytes, _) in goldens() {
        let net = net_of(&bytes);
        let once = to_shape_a_string(&net).unwrap();
        let twice = to_shape_a_string(&net_of(once.as_bytes())).unwrap();
        assert_eq!(once, twice, "{name}");
    }
}

/// Everything the reader put in the net comes back from the written document.
#[test]
fn a_round_trip_keeps_the_net() {
    for (name, bytes, _) in goldens() {
        let net = net_of(&bytes);
        let back = net_of(to_shape_a_string(&net).unwrap().as_bytes());
        assert_eq!(back.token, net.token, "{name}");
        assert_eq!(back.places.len(), net.places.len(), "{name}");
        for (id, p) in &net.places {
            let q = &back.places[id];
            assert_eq!(
                (&q.initial, &q.capacity, q.x, q.y, &q.label_text, q.offset),
                (&p.initial, &p.capacity, p.x, p.y, &p.label_text, p.offset),
                "{name} {id}"
            );
        }
        assert_eq!(back.transitions.len(), net.transitions.len(), "{name}");
        for (id, t) in &net.transitions {
            let u = &back.transitions[id];
            assert_eq!(
                (&u.role, u.x, u.y, &u.label_text, u.offset),
                (&t.role, t.x, t.y, &t.label_text, t.offset),
                "{name} {id}"
            );
        }
        assert_eq!(back.arcs.len(), net.arcs.len(), "{name}");
        for (a, b) in net.arcs.iter().zip(&back.arcs) {
            assert_eq!(
                (&a.source, &a.target, &a.weight, a.inhibit_transition),
                (&b.source, &b.target, &b.weight, b.inhibit_transition),
                "{name}"
            );
        }
    }
}

/// `HashMap` order differs per instance, so reading afresh is the real test.
#[test]
fn the_same_net_gives_the_same_bytes() {
    // Small on purpose: canonicalizing net-b takes seconds in a debug build.
    let (_, bytes, _) = goldens().into_iter().find(|(n, _, _)| n == "tie2.jsonld").unwrap();
    let first = to_shape_a_string(&net_of(&bytes)).unwrap();
    for _ in 0..25 {
        assert_eq!(to_shape_a_string(&net_of(&bytes)).unwrap(), first);
    }
}

fn small() -> PetriNet {
    PetriNet::build()
        .place("a", 1.0)
        .place_with_capacity("g", 2.0, 5.0)
        .transition("t")
        .arc("a", "t", 1.0)
        .read_arc("t", "g", 2.0)
        .done()
}

#[test]
fn the_document_has_the_identity_fields() {
    let doc = to_shape_a(&small()).unwrap();
    assert_eq!(doc["@context"], "https://pflow.xyz/schema");
    assert_eq!(doc["@type"], "PetriNet");
    assert_eq!(doc["@version"], "1.1");
    assert_eq!(doc["places"]["a"]["@type"], "Place");
    assert_eq!(doc["transitions"]["t"]["@type"], "Transition");
    assert_eq!(doc["arcs"][0]["@type"], "Arrow");
    assert_eq!(compute_cid(&serde_json::to_vec(&doc).unwrap()).unwrap().0, id_of(&doc));
}

#[test]
fn numbers_are_written_as_the_cid_needs_them() {
    let mut net = small();
    net.places.get_mut("a").unwrap().x = 1.5;
    net.places.get_mut("g").unwrap().capacity = vec![f64::INFINITY];
    let doc = to_shape_a(&net).unwrap();
    // 2.0 must be the integer 2, not the float: they hash differently.
    assert!(doc["places"]["g"]["initial"][0].is_i64());
    assert!(doc["arcs"][0]["weight"][0].is_i64());
    assert!(doc["places"]["g"]["y"].is_i64());
    assert_eq!(doc["places"]["a"]["x"], 1.5);
    assert_eq!(doc["places"]["g"]["capacity"][0], Value::Null);
}

/// A read arc keeps the direction that tells it from an inhibitor.
#[test]
fn a_contextual_arc_keeps_its_drawn_direction() {
    let doc = to_shape_a(&small()).unwrap();
    assert_eq!(doc["arcs"][1]["source"], "t");
    assert_eq!(doc["arcs"][1]["target"], "g");
    assert_eq!(doc["arcs"][1]["inhibitTransition"], true);
    let back = net_of(&serde_json::to_vec(&doc).unwrap());
    assert_eq!(back.arc_kind(&back.arcs[1]), pflow_core::ArcKind::Read);
}

#[test]
fn optional_node_fields_are_omitted_when_the_net_has_none() {
    let doc = to_shape_a(&small()).unwrap();
    let p = doc["places"]["a"].as_object().unwrap();
    assert!(!p.contains_key("label") && !p.contains_key("offset"));
    let t = doc["transitions"]["t"].as_object().unwrap();
    assert!(!t.contains_key("label") && !t.contains_key("offset"));
    assert_eq!(t["role"], "default");
}

#[test]
fn the_cid_follows_the_content() {
    let base = id_of(&to_shape_a(&small()).unwrap()).to_string();
    let mut changed = small();
    changed.arcs[0].weight = vec![2.0];
    assert_ne!(id_of(&to_shape_a(&changed).unwrap()), base);

    // Arc order is part of the list, so part of the identity.
    let mut reordered = small();
    reordered.arcs.reverse();
    assert_ne!(id_of(&to_shape_a(&reordered).unwrap()), base);
}

#[test]
fn sealing_something_that_is_not_an_object_is_an_error() {
    assert!(seal_shape_a(json!([1, 2])).is_err());
}
