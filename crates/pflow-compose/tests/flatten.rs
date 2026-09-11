//! Flatten behavior pinned against go-pflow's `metamodel/compose_test.go`
//! cases (read directly from source, not regenerated from a Go run — see
//! the crate's module docs and ROADMAP.md Phase 3 for why no
//! go-pflow-produced flatten golden exists yet).

use pflow_compose::bundle::{Bundle, Endpoint, Link, LinkKind, NetType, Subnet};
use pflow_metamodel::{Arc, Model, Place, StateKind, Transition};

fn tok(id: &str, initial: i64) -> Place {
    Place {
        id: id.to_string(),
        kind: Some(StateKind::Token),
        initial,
        ..Default::default()
    }
}

fn orders_net() -> Model {
    Model {
        name: "orders".to_string(),
        places: vec![
            Place { initial: 1, ..tok("pending", 1) },
            Place { exported: true, ..tok("confirmed", 0) },
            Place { exported: true, ..tok("shipped", 0) },
        ],
        transitions: vec![
            Transition { id: "confirm".to_string(), http_method: "POST".to_string(), http_path: "/api/confirm".to_string(), ..Default::default() },
            Transition { id: "ship".to_string(), http_method: "POST".to_string(), http_path: "/api/ship".to_string(), ..Default::default() },
        ],
        arcs: vec![
            Arc { from: "pending".into(), to: "confirm".into(), weight: 1, ..Default::default() },
            Arc { from: "confirm".into(), to: "confirmed".into(), weight: 1, ..Default::default() },
            Arc { from: "confirmed".into(), to: "ship".into(), weight: 1, ..Default::default() },
            Arc { from: "ship".into(), to: "shipped".into(), weight: 1, ..Default::default() },
        ],
        ..Default::default()
    }
}

fn inventory_net() -> Model {
    Model {
        name: "inventory".to_string(),
        places: vec![
            Place { exported: true, ..tok("available", 10) },
            tok("reserved", 0),
            tok("consumed", 0),
        ],
        transitions: vec![
            Transition { id: "reserve".to_string(), http_method: "POST".to_string(), http_path: "/api/reserve".to_string(), ..Default::default() },
            Transition { id: "ship_out".to_string(), ..Default::default() },
        ],
        arcs: vec![
            Arc { from: "available".into(), to: "reserve".into(), weight: 1, ..Default::default() },
            Arc { from: "reserve".into(), to: "reserved".into(), weight: 1, ..Default::default() },
            Arc { from: "reserved".into(), to: "ship_out".into(), weight: 1, ..Default::default() },
            Arc { from: "ship_out".into(), to: "consumed".into(), weight: 1, ..Default::default() },
        ],
        ..Default::default()
    }
}

fn orders_inventory_bundle() -> Bundle {
    let mut b = Bundle::new("shop");
    b.add_subnet(Subnet { id: "orders".to_string(), net_type: NetType::Workflow, model: orders_net(), ..Default::default() });
    b.add_subnet(Subnet { id: "inventory".to_string(), net_type: NetType::Resource, model: inventory_net(), ..Default::default() });
    b.add_link(Link {
        kind: LinkKind::Event,
        from: Endpoint { subnet: "orders".into(), transition: "confirm".into(), ..Default::default() },
        to: Endpoint { subnet: "inventory".into(), transition: "reserve".into(), ..Default::default() },
        ..Default::default()
    });
    b.add_link(Link {
        kind: LinkKind::Event,
        from: Endpoint { subnet: "orders".into(), transition: "ship".into(), ..Default::default() },
        to: Endpoint { subnet: "inventory".into(), transition: "ship_out".into(), ..Default::default() },
        ..Default::default()
    });
    b
}

fn place_ids(m: &Model) -> Vec<String> {
    let mut v: Vec<String> = m.places.iter().map(|p| p.id.clone()).collect();
    v.sort();
    v
}
fn transition_ids(m: &Model) -> Vec<String> {
    let mut v: Vec<String> = m.transitions.iter().map(|t| t.id.clone()).collect();
    v.sort();
    v
}

#[test]
fn identity_flatten_is_an_exact_deep_copy() {
    for model in [orders_net(), inventory_net()] {
        let mut b = Bundle::new(&model.name);
        b.add_subnet(Subnet { id: model.name.clone(), model: model.clone(), ..Default::default() });
        let got = b.flatten().unwrap();
        // Name is stamped from the bundle when set; here bundle name == model name.
        assert_eq!(got.places, model.places);
        assert_eq!(got.transitions, model.transitions);
        assert_eq!(got.arcs, model.arcs);
    }
}

#[test]
fn identity_flatten_does_not_alias_input() {
    let src = orders_net();
    let mut b = Bundle::new("orders");
    b.add_subnet(Subnet { id: "orders".to_string(), model: src.clone(), ..Default::default() });

    let mut got = b.flatten().unwrap();
    got.places[0].id = "mutated".to_string();
    got.transitions[0].bindings.clear();

    assert_ne!(src.places[0].id, "mutated");
}

fn chain_bundle() -> Bundle {
    let mk = |name: &str| Model {
        name: name.to_string(),
        places: vec![tok("in", 1), tok("out", 0)],
        transitions: vec![Transition { id: "go".to_string(), ..Default::default() }],
        arcs: vec![
            Arc { from: "in".into(), to: "go".into(), weight: 1, ..Default::default() },
            Arc { from: "go".into(), to: "out".into(), weight: 1, ..Default::default() },
        ],
        ..Default::default()
    };
    let mut b = Bundle::new("chain");
    for id in ["a", "b", "c"] {
        b.add_subnet(Subnet { id: id.to_string(), model: mk(id), ..Default::default() });
    }
    b.add_link(Link {
        kind: LinkKind::Event,
        from: Endpoint { subnet: "a".into(), transition: "go".into(), ..Default::default() },
        to: Endpoint { subnet: "b".into(), transition: "go".into(), ..Default::default() },
        ..Default::default()
    });
    b.add_link(Link {
        kind: LinkKind::Event,
        from: Endpoint { subnet: "b".into(), transition: "go".into(), ..Default::default() },
        to: Endpoint { subnet: "c".into(), transition: "go".into(), ..Default::default() },
        ..Default::default()
    });
    b
}

#[test]
fn flatten_is_associative_regardless_of_link_or_subnet_order() {
    let base = chain_bundle().flatten().unwrap();
    assert_eq!(base.transitions.len(), 1);
    assert_eq!(base.transitions[0].id, "fused:a/go+b/go+c/go");

    let mut b1 = chain_bundle();
    b1.links.swap(0, 1);
    let got1 = b1.flatten().unwrap();
    assert_eq!(place_ids(&got1), place_ids(&base));
    assert_eq!(transition_ids(&got1), transition_ids(&base));
    assert_eq!(got1.arcs.len(), base.arcs.len());

    let mut b2 = chain_bundle();
    b2.subnets.swap(0, 2);
    let got2 = b2.flatten().unwrap();
    assert_eq!(place_ids(&got2), place_ids(&base));
    assert_eq!(transition_ids(&got2), transition_ids(&base));
}

#[test]
fn chained_token_links_collapse_to_one_canonical_wire() {
    let mk = |name: &str| Model {
        name: name.to_string(),
        places: vec![Place { exported: true, ..tok("shared", 0) }],
        transitions: vec![Transition { id: format!("t_{name}"), ..Default::default() }],
        arcs: vec![Arc { from: format!("t_{name}"), to: "shared".into(), weight: 1, ..Default::default() }],
        ..Default::default()
    };
    let mut b = Bundle::new("wires");
    for id in ["a", "b", "c"] {
        b.add_subnet(Subnet { id: id.to_string(), net_type: NetType::Resource, model: mk(id), ..Default::default() });
    }
    b.add_link(Link {
        kind: LinkKind::Token,
        from: Endpoint { subnet: "a".into(), place: "shared".into(), ..Default::default() },
        to: Endpoint { subnet: "b".into(), place: "shared".into(), ..Default::default() },
        ..Default::default()
    });
    b.add_link(Link {
        kind: LinkKind::Token,
        from: Endpoint { subnet: "b".into(), place: "shared".into(), ..Default::default() },
        to: Endpoint { subnet: "c".into(), place: "shared".into(), ..Default::default() },
        ..Default::default()
    });

    let (got, fm) = b.flatten_with_map().unwrap();
    assert_eq!(got.places.len(), 1);
    assert_eq!(got.places[0].id, "wire:a/shared");
    assert_eq!(fm.wires["wire:a/shared"].len(), 3);
}

fn place_merge_bundle(a: Place, b: Place) -> Bundle {
    let mut bundle = Bundle::new("merge");
    bundle.add_subnet(Subnet {
        id: "a".to_string(),
        net_type: NetType::Resource,
        model: Model {
            name: "a".to_string(),
            places: vec![a.clone()],
            transitions: vec![Transition { id: "ta".to_string(), ..Default::default() }],
            arcs: vec![Arc { from: "ta".into(), to: a.id.clone(), weight: 1, ..Default::default() }],
            ..Default::default()
        },
        ..Default::default()
    });
    bundle.add_subnet(Subnet {
        id: "b".to_string(),
        net_type: NetType::Resource,
        model: Model {
            name: "b".to_string(),
            places: vec![b.clone()],
            transitions: vec![Transition { id: "tb".to_string(), ..Default::default() }],
            arcs: vec![Arc { from: "tb".into(), to: b.id.clone(), weight: 1, ..Default::default() }],
            ..Default::default()
        },
        ..Default::default()
    });
    bundle.add_link(Link {
        kind: LinkKind::Token,
        from: Endpoint { subnet: "a".into(), place: a.id.clone(), ..Default::default() },
        to: Endpoint { subnet: "b".into(), place: b.id.clone(), ..Default::default() },
        ..Default::default()
    });
    bundle
}

#[test]
fn place_merge_initial_sums() {
    let got = place_merge_bundle(
        Place { exported: true, ..tok("p", 3) },
        Place { exported: true, ..tok("p", 4) },
    )
    .flatten()
    .unwrap();
    assert_eq!(got.places[0].initial, 7);
}

#[test]
fn place_merge_capacity_takes_tightest_bound() {
    let got = place_merge_bundle(
        Place { capacity: 10, exported: true, ..tok("p", 0) },
        Place { capacity: 4, exported: true, ..tok("p", 0) },
    )
    .flatten()
    .unwrap();
    assert_eq!(got.places[0].capacity, 4);
}

#[test]
fn place_merge_zero_capacity_means_unbounded() {
    let got = place_merge_bundle(
        Place { capacity: 0, exported: true, ..tok("p", 0) },
        Place { capacity: 6, exported: true, ..tok("p", 0) },
    )
    .flatten()
    .unwrap();
    assert_eq!(got.places[0].capacity, 6);
}

#[test]
fn place_merge_exported_ors() {
    let got = place_merge_bundle(
        Place { exported: true, ..tok("p", 0) },
        Place { exported: true, ..tok("p", 0) },
    )
    .flatten()
    .unwrap();
    assert!(got.places[0].exported);
}

#[test]
fn place_merge_rejects_kind_mismatch() {
    let err = place_merge_bundle(
        Place { exported: true, ..tok("p", 0) },
        Place {
            id: "p".to_string(),
            kind: Some(StateKind::Data),
            typ: "string".to_string(),
            exported: true,
            ..Default::default()
        },
    )
    .flatten()
    .unwrap_err();
    assert!(err.contains("E_KIND_MISMATCH"), "{err}");
}

#[test]
fn place_merge_rejects_type_conflict() {
    let mk = |id: &str, typ: &str| Model {
        name: id.to_string(),
        places: vec![Place {
            id: "p".to_string(),
            kind: Some(StateKind::Data),
            typ: typ.to_string(),
            exported: true,
            ..Default::default()
        }],
        transitions: vec![Transition { id: format!("t{id}"), ..Default::default() }],
        ..Default::default()
    };
    let mut bundle = Bundle::new("merge");
    bundle.add_subnet(Subnet { id: "a".to_string(), net_type: NetType::Resource, model: mk("a", "map[string]int64"), ..Default::default() });
    bundle.add_subnet(Subnet { id: "b".to_string(), net_type: NetType::Resource, model: mk("b", "map[string]string"), ..Default::default() });
    bundle.add_link(Link {
        kind: LinkKind::Data,
        from: Endpoint { subnet: "a".into(), place: "p".into(), ..Default::default() },
        to: Endpoint { subnet: "b".into(), place: "p".into(), ..Default::default() },
        ..Default::default()
    });
    let err = bundle.flatten().unwrap_err();
    assert!(err.contains("E_TYPE_MISMATCH"), "{err}");
}

#[test]
fn event_link_fuses_confirm_and_reserve_with_initiator_owning_the_route() {
    let (got, fm) = orders_inventory_bundle().flatten_with_map().unwrap();

    let mut ids = transition_ids(&got);
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "fused:inventory/reserve+orders/confirm".to_string(),
            "fused:inventory/ship_out+orders/ship".to_string(),
        ]
    );

    let fused = "fused:inventory/reserve+orders/confirm";
    let mut inputs: Vec<String> = got.arcs.iter().filter(|a| a.to == fused).map(|a| a.from.clone()).collect();
    inputs.sort();
    assert_eq!(inputs, vec!["inventory/available".to_string(), "orders/pending".to_string()]);

    let mut emits = fm.member_events[fused].clone();
    emits.sort();
    assert_eq!(emits, vec!["confirm".to_string(), "reserve".to_string()]);

    let tr = got.transition_by_id(fused).unwrap();
    assert_eq!(tr.http_path, "/api/confirm"); // orders/confirm is the initiator
    assert!(fm.warnings.iter().any(|w| w.code == "W_ROUTE_DROPPED" && w.message.contains("/api/reserve")));
}

#[test]
fn guard_link_lowers_structurally_and_expr_when_it_cannot() {
    let mk_place_net = |gated: bool| Model {
        name: "counter".to_string(),
        places: vec![tok("orders_placed", 0)],
        transitions: vec![Transition { id: "start".to_string(), guard_unrepresentable: gated, ..Default::default() }],
        arcs: vec![Arc { from: "start".into(), to: "orders_placed".into(), weight: 1, ..Default::default() }],
        ..Default::default()
    };
    let pantry = Model {
        name: "pantry".to_string(),
        places: vec![Place { exported: true, ..tok("beans", 5) }],
        ..Default::default()
    };

    let mut b = Bundle::new("cafe");
    b.add_subnet(Subnet { id: "counter".to_string(), model: mk_place_net(false), ..Default::default() });
    b.add_subnet(Subnet { id: "pantry".to_string(), model: pantry, ..Default::default() });
    b.add_link(Link {
        kind: LinkKind::Guard,
        from: Endpoint { subnet: "counter".into(), transition: "start".into(), ..Default::default() },
        to: Endpoint { subnet: "pantry".into(), place: "beans".into(), ..Default::default() },
        condition: ">= 2".to_string(),
        ..Default::default()
    });

    let got = b.flatten().unwrap();
    let start = got.transition_by_id("counter/start").unwrap();
    assert!(start.guard.is_empty(), "structural lowering must not restate the condition as text");
    assert!(got
        .arcs
        .iter()
        .any(|a| a.from == "pantry/beans" && a.to == "counter/start" && a.typ == pflow_metamodel::ArcType::Read && a.weight == 2));
}

#[test]
fn cafe_bundle_showcase_fixture_flattens() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../pflow-xyz/examples/showcase/cafe.bundle.json");
    let Ok(json) = std::fs::read_to_string(&path) else {
        eprintln!("skip: no pflow-xyz checkout at {}", path.display());
        return;
    };
    let bundle: Bundle = serde_json::from_str(&json).expect("cafe.bundle.json parses");
    assert_eq!(bundle.subnets.len(), 3);
    assert_eq!(bundle.links.len(), 7);

    let (flat, fm) = bundle.flatten_with_map().expect("cafe bundle flattens");
    // Three independently-authored nets (counter, pantry, staff) fuse
    // start_X with brew_X and acquire_X, and finish_X with release_X, via
    // EventLinks; a GuardLink gates start_espresso on the pantry's beans.
    assert!(flat.places.iter().any(|p| p.id == "pantry/beans"));
    // Links that fuse the same transition share an explicit id
    // ("espresso_started"), which becomes the fused transition's flat name
    // rather than a "fused:a+b+c" concatenation.
    let espresso_started = flat.transition_by_id("espresso_started").expect("fused transition named by the shared link id");
    assert!(fm.fused_groups["espresso_started"]
        .iter()
        .any(|m| m.contains("start_espresso")));
    assert!(fm.fused_groups["espresso_started"]
        .iter()
        .any(|m| m.contains("acquire_espresso")));
    let _ = espresso_started;
    // The guard link lowers structurally (>= 2 has a structural form), so no
    // transition should carry a leftover "beans" guard-expression text.
    assert!(!flat.transitions.iter().any(|t| t.guard.contains("beans")));
    assert!(flat
        .arcs
        .iter()
        .any(|a| a.from == "pantry/beans" && a.typ == pflow_metamodel::ArcType::Read && a.weight == 2));
    assert!(fm.fused_groups.len() >= 2);
}
