//! `Bundle::validate_one_link`: one link judged against the subnets it names,
//! as if it were the bundle's only link.

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

fn link(kind: LinkKind, from: Endpoint, to: Endpoint) -> Link {
    Link {
        kind,
        from,
        to,
        ..Default::default()
    }
}

fn codes(errs: &[pflow_compose::ValidationError]) -> Vec<&str> {
    errs.iter().map(|e| e.code.as_str()).collect()
}

#[test]
fn a_legal_link_is_judged_alone() {
    let mut b = Bundle::new("t");
    b.add_subnet(queue("a"));
    b.add_subnet(new_resource_pool("w", 2));
    b.add_subnet(queue("c"));
    // a broken link already in the bundle does not refuse a new one
    b.add_link(link(
        LinkKind::Event,
        port("a", "nope"),
        port("w", "acquire"),
    ));
    let res = b.validate_one_link(&link(
        LinkKind::Event,
        port("a", "dequeue"),
        port("w", "acquire"),
    ));
    assert!(res.valid, "{:?}", res.errors);
    assert!(res.errors.is_empty());
    // the whole bundle is not valid
    assert!(!b.validate().valid);
}

#[test]
fn messages_label_the_link_links_0() {
    let mut b = Bundle::new("t");
    b.add_subnet(queue("a"));
    b.add_subnet(queue("b"));
    b.add_link(link(LinkKind::Token, port("a", "out"), port("b", "in")));
    let res = b.validate_one_link(&link(LinkKind::Token, port("a", "in"), port("b", "ghost")));
    assert!(!res.valid);
    assert_eq!(codes(&res.errors), ["E_BAD_ENDPOINT"]);
    assert_eq!(
        res.errors[0].message,
        "links[0]: to: subnet \"b\" has no port \"ghost\""
    );
}

#[test]
fn only_the_named_subnets_are_judged() {
    let mut b = Bundle::new("t");
    b.add_subnet(queue("a"));
    b.add_subnet(queue("b"));
    // a third subnet with a duplicate id elsewhere does not matter …
    b.add_subnet(queue("c"));
    b.add_subnet(queue("c"));
    let l = link(LinkKind::Token, port("a", "out"), port("b", "in"));
    assert!(b.validate_one_link(&l).valid);
    // … but one the link names is reported
    let l = link(LinkKind::Token, port("a", "out"), port("c", "in"));
    assert_eq!(
        codes(&b.validate_one_link(&l).errors),
        ["E_DUPLICATE_SUBNET"]
    );
}
