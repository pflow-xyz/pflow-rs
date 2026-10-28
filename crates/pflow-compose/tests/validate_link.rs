//! `Bundle::validate`'s link checks against go-pflow's `validateLink`
//! (metamodel/compose.go): codes and messages, word for word.

use pflow_compose::bundle::{Bundle, Endpoint, Link, LinkKind};
use pflow_compose::{new_queue, new_resource_pool, QueueSpec};

fn queue(id: &str) -> pflow_compose::Subnet {
    new_queue(QueueSpec {
        id: id.into(),
        capacity: 3,
        initial: 0,
        description: String::new(),
    })
    .unwrap()
}

fn port(subnet: &str, port: &str) -> Endpoint {
    Endpoint {
        subnet: subnet.into(),
        port: port.into(),
        ..Default::default()
    }
}

/// (codes, messages) of a bundle of a queue `a`, a queue `b`, a pool `w`
/// and one link.
fn verdict(kind: LinkKind, from: Endpoint, to: Endpoint) -> (Vec<String>, Vec<String>) {
    let mut b = Bundle::new("t");
    b.add_subnet(queue("a"));
    b.add_subnet(queue("b"));
    b.add_subnet(new_resource_pool("w", 2));
    b.add_link(Link {
        kind,
        from,
        to,
        ..Default::default()
    });
    let r = b.validate();
    (
        r.errors.iter().map(|e| e.code.clone()).collect(),
        r.errors.iter().map(|e| e.message.clone()).collect(),
    )
}

#[test]
fn token_link_ports_face_the_flow() {
    let (codes, msgs) = verdict(LinkKind::Token, port("a", "in"), port("b", "out"));
    assert_eq!(codes, ["E_PORT_DIRECTION", "E_PORT_DIRECTION"]);
    assert_eq!(
        msgs,
        [
            r#"links[0]: token link's from port "in" is an in-port"#,
            r#"links[0]: token link's to port "out" is an out-port"#,
        ]
    );
    let (codes, _) = verdict(LinkKind::Token, port("a", "out"), port("b", "in"));
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn port_schemas_must_agree_when_both_are_set() {
    let (codes, msgs) = verdict(LinkKind::Token, port("a", "out"), port("w", "available"));
    assert_eq!(codes, ["E_SCHEMA_MISMATCH"]);
    assert_eq!(
        msgs,
        [r#"links[0]: port schemas differ ("queue:items" vs "resource")"#]
    );
    // Checked before the kind's own checks, which still run.
    let (codes, _) = verdict(LinkKind::Data, port("a", "out"), port("w", "available"));
    assert_eq!(codes, ["E_SCHEMA_MISMATCH", "E_DATALINK_CONSUMES"]);
    // Transition ports carry no schema.
    let (codes, _) = verdict(LinkKind::Event, port("a", "dequeue"), port("w", "acquire"));
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn endpoint_kind_names_the_link_kind_as_written() {
    let (_, msgs) = verdict(LinkKind::Token, port("a", "dequeue"), port("b", "in"));
    assert_eq!(
        msgs,
        ["links[0]: a token link connects places, but an endpoint names a transition"]
    );
    let (_, msgs) = verdict(LinkKind::Data, port("a", "out"), port("b", "enqueue"));
    assert_eq!(
        msgs,
        ["links[0]: a data link connects places, but an endpoint names a transition"]
    );
}
