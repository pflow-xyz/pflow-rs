//! L4 liveness (`Kind::L4Live`, and `Analyzer::analyze`'s verdicts). There is no
//! go-pflow golden to replay; the philosophers net is the same one as go-pflow's
//! `reachability/liveness_test.go`, with the same expected verdicts. The netting nets are pflow-jl's
//! (`test/test_netting.jl`, `docs/netting-experiment.md`), and its testset
//! "liveness does not survive gluing" states the expected results.

use std::collections::{HashMap, HashSet};

use pflow_metamodel::{Arc, Model, Place, Transition};
use pflow_reachability::{Analyzer, L4Status, Verdict, WitnessKind};
use pflow_verify::{Kind, Method, Property, Status, Verifier};

fn model(places: &[(&str, i64)], flows: &[(&str, &[&str], &[&str])]) -> Model {
    let mut arcs = Vec::new();
    for (t, ins, outs) in flows {
        for p in *ins {
            arcs.push(Arc { from: p.to_string(), to: t.to_string(), ..Default::default() });
        }
        for p in *outs {
            arcs.push(Arc { from: t.to_string(), to: p.to_string(), ..Default::default() });
        }
    }
    Model {
        name: "t".into(),
        places: places
            .iter()
            .map(|(id, initial)| Place { id: id.to_string(), initial: *initial, ..Default::default() })
            .collect(),
        transitions: flows
            .iter()
            .map(|(t, _, _)| Transition { id: t.to_string(), ..Default::default() })
            .collect(),
        arcs,
        ..Default::default()
    }
}

/// The three-channel Settle cycle: a -> b -> c -> a, one token on each balance.
const CYCLE_PLACES: &[(&str, i64)] = &[
    ("a", 1),
    ("b", 1),
    ("c", 1),
    ("pending_ab", 0),
    ("pending_bc", 0),
    ("pending_ca", 0),
];
const CYCLE: &[(&str, &[&str], &[&str])] = &[
    ("send_ab", &["a"], &["pending_ab"]),
    ("settle_ab", &["pending_ab"], &["b"]),
    ("send_bc", &["b"], &["pending_bc"]),
    ("settle_bc", &["pending_bc"], &["c"]),
    ("send_ca", &["c"], &["pending_ca"]),
    ("settle_ca", &["pending_ca"], &["a"]),
];

fn cycle() -> Model {
    model(CYCLE_PLACES, CYCLE)
}

/// The cycle with the rho = 3 observer glued on along the pending places.
fn netting() -> Model {
    let mut places = CYCLE_PLACES.to_vec();
    places.push(("cleared", 0));
    let mut flows = CYCLE.to_vec();
    flows.push(("net_abc", &["pending_ab", "pending_bc", "pending_ca"], &["cleared"]));
    model(&places, &flows)
}

fn l4(m: &Model) -> pflow_verify::Verdict {
    Verifier::new(m).check_one(Property::new(Kind::L4Live))
}

/// Replays `trace` through the one firing rule and checks where it ends.
fn assert_replays(m: &Model, trace: &[String], marking: &HashMap<String, i64>) {
    let (ok, end) = Analyzer::new(m).can_fire(trace);
    assert!(ok, "trace {trace:?} does not fire");
    let nonzero = |x: &HashMap<String, i64>| -> HashMap<String, i64> {
        x.iter().filter(|(_, &v)| v != 0).map(|(k, &v)| (k.clone(), v)).collect()
    };
    assert_eq!(nonzero(&end), nonzero(marking), "trace {trace:?}");
}

#[test]
fn the_settle_cycle_is_l4_live() {
    let m = cycle();
    let v = l4(&m);
    assert_eq!(v.status, Status::Proved, "{}", v.detail);
    assert_eq!(v.method, Method::Exhaustive);
    assert_eq!(v.counterexample.map(|c| c.trace), None);
    assert_eq!(v.liveness.len(), 6);
    assert!(v.liveness.iter().all(|t| t.status == L4Status::Live && t.witness.is_none()));
    // Every distribution of 3 tokens over 6 places is reachable: C(8,3).
    assert_eq!(Analyzer::new(&m).l4_liveness().state_count, 56);
}

/// Liveness does not survive gluing: same initial marking, same channels, and
/// the net can now clear itself into a dead marking that kills every transition.
#[test]
fn the_netting_composite_is_not_l4_live() {
    let m = netting();
    let v = l4(&m);
    assert_eq!(v.status, Status::Refuted);
    assert_eq!(v.method, Method::Exhaustive);
    assert_eq!(v.liveness.len(), 7);

    for t in &v.liveness {
        assert_eq!(t.status, L4Status::NotLive, "{}", t.transition);
        let w = t.witness.as_ref().unwrap();
        assert_replays(&m, &w.trace, &w.marking);
        assert_eq!(w.region_size, 1);
        assert_eq!(w.marking["cleared"], 1);
        assert_eq!(w.marking.values().sum::<i64>(), 1);
        // Three sends and one netting, whichever order the search met them in.
        assert_eq!(w.trace.len(), 4);
        assert_eq!(w.trace[3], "net_abc");
        let sends: HashSet<&str> = w.trace[..3].iter().map(String::as_str).collect();
        assert_eq!(sends, HashSet::from(["send_ab", "send_bc", "send_ca"]));
    }

    let c = v.counterexample.unwrap();
    assert_eq!(c.trace.len(), 4);
    assert!(c.explanation.contains("send_ab can never fire again"), "{}", c.explanation);

    // Each of the other properties still says what it said.
    assert_eq!(Verifier::new(&m).check_one(Property::new(Kind::Live)).status, Status::Proved);
    assert_eq!(Verifier::new(&m).check_one(Property::new(Kind::DeadlockFree)).status, Status::Refuted);
}

/// A region the net can enter and never leave, with no deadlock anywhere: every
/// reachable marking has an enabled transition. L1 and the deadlock check both
/// pass. Only L4 sees it, and only for the transitions shut out.
#[test]
fn a_dead_end_region_with_no_global_deadlock() {
    let m = model(
        &[("s", 1), ("a", 0), ("x", 0), ("y", 0)],
        &[
            ("go", &["s"], &["a"]),
            ("back", &["a"], &["s"]),
            ("trap", &["s"], &["x"]),
            ("xy", &["x"], &["y"]),
            ("yx", &["y"], &["x"]),
        ],
    );
    let check = |k| Verifier::new(&m).check_one(Property::new(k));
    assert_eq!(check(Kind::DeadlockFree).status, Status::Proved);
    assert_eq!(check(Kind::Live).status, Status::Proved);

    let v = check(Kind::L4Live);
    assert_eq!(v.status, Status::Refuted);
    let by_name: HashMap<&str, _> = v.liveness.iter().map(|t| (t.transition.as_str(), t)).collect();
    for t in ["go", "back", "trap"] {
        assert_eq!(by_name[t].status, L4Status::NotLive, "{t}");
        let w = by_name[t].witness.as_ref().unwrap();
        assert_eq!(w.trace, vec!["trap".to_string()], "{t}");
        assert_eq!(w.marking["x"], 1);
        assert_eq!(w.region_size, 2, "a livelock, not a dead marking");
        assert_replays(&m, &w.trace, &w.marking);
    }
    for t in ["xy", "yx"] {
        assert_eq!(by_name[t].status, L4Status::Live, "{t}");
        assert!(by_name[t].witness.is_none());
    }

    let c = v.counterexample.unwrap();
    assert!(c.explanation.contains("closed region of 2 markings"), "{}", c.explanation);
    assert_eq!(v.detail, "not L4-live: go, back, trap");
}

#[test]
fn a_transition_must_fire_in_every_terminal_component() {
    // Two separate traps: u loops only in the first, v only in the second, so
    // neither is L4-live though each fires forever in its own region.
    let m = model(
        &[("s", 1), ("p", 0), ("q", 0)],
        &[
            ("to_p", &["s"], &["p"]),
            ("to_q", &["s"], &["q"]),
            ("u", &["p"], &["p"]),
            ("v", &["q"], &["q"]),
        ],
    );
    let v = l4(&m);
    assert_eq!(v.status, Status::Refuted);
    assert!(v.liveness.iter().all(|t| t.status == L4Status::NotLive));
    let by_name: HashMap<&str, _> = v.liveness.iter().map(|t| (t.transition.as_str(), t)).collect();
    // u's witness must be the region u is shut out of.
    assert_eq!(by_name["u"].witness.as_ref().unwrap().trace, vec!["to_q".to_string()]);
    assert_eq!(by_name["v"].witness.as_ref().unwrap().trace, vec!["to_p".to_string()]);
}

#[test]
fn an_empty_end_marking_still_ends_liveness() {
    // The deadlock heuristic accepts an empty terminal marking. L4 does not:
    // nothing can fire from there.
    let m = model(&[("p", 1)], &[("t", &["p"], &[])]);
    assert_eq!(Verifier::new(&m).check_one(Property::new(Kind::DeadlockFree)).status, Status::Proved);
    let v = l4(&m);
    assert_eq!(v.status, Status::Refuted);
    assert_eq!(v.liveness[0].witness.as_ref().unwrap().trace, vec!["t".to_string()]);
}

#[test]
fn a_dead_initial_marking_has_an_empty_witness_trace() {
    let m = model(&[("p", 0)], &[("t", &["p"], &[])]);
    let v = l4(&m);
    assert_eq!(v.status, Status::Refuted);
    assert!(v.liveness[0].witness.as_ref().unwrap().trace.is_empty());
}

#[test]
fn a_truncated_state_space_is_unknown_not_live() {
    // A source: unbounded, and every explored state is a frontier state.
    let m = model(&[("p", 0)], &[("gen", &[], &["p"])]);
    let v = Verifier::new(&m).with_max_states(20).check_one(Property::new(Kind::L4Live));
    assert_eq!(v.status, Status::Unknown);
    assert_eq!(v.method, Method::Partial);
    assert_eq!(v.liveness[0].status, L4Status::Unknown);
    assert!(v.counterexample.is_none());
}

#[test]
fn a_closed_region_found_before_truncation_still_refutes() {
    // `gen` grows p without limit from s, but `stop` reaches a real dead
    // marking, fully expanded, so the refutation holds on a partial search.
    let m = model(
        &[("s", 1), ("p", 0), ("d", 0)],
        &[("gen", &["s"], &["s", "p"]), ("stop", &["s"], &["d"])],
    );
    let v = Verifier::new(&m).with_max_states(20).check_one(Property::new(Kind::L4Live));
    assert_eq!(v.status, Status::Refuted);
    assert_eq!(v.method, Method::Partial);
    for t in &v.liveness {
        assert_eq!(t.status, L4Status::NotLive, "{}", t.transition);
        let w = t.witness.as_ref().unwrap();
        assert_eq!(w.trace, vec!["stop".to_string()]);
        assert_replays(&m, &w.trace, &w.marking);
    }
}

#[test]
fn a_long_chain_does_not_overflow_the_stack() {
    let n = 200_000;
    let m = model(&[("p", n), ("q", 0)], &[("t", &["p"], &["q"])]);
    let r = Analyzer::new(&m).with_max_tokens(n).with_max_states(n as usize + 2).l4_liveness();
    assert!(!r.truncated);
    assert_eq!(r.state_count, n as usize + 1);
    assert_eq!(r.terminal_components, 1);
    assert_eq!(r.transitions[0].status, L4Status::NotLive);
    assert_eq!(r.transitions[0].witness.as_ref().unwrap().trace.len(), n as usize);
}

#[test]
fn the_verdict_is_the_same_on_every_run() {
    let a = l4(&netting());
    let b = l4(&netting());
    assert_eq!(a.liveness, b.liveness);
    assert_eq!(a.detail, b.detail);
}

/// The definition itself, checked by brute force on small random nets: t is
/// L4-live exactly when every reachable marking can reach one enabling t.
#[test]
fn terminal_components_agree_with_the_definition() {
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |n: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let names = ["p0", "p1", "p2", "p3"];
    let (mut checked, mut live, mut not_live) = (0, 0, 0);
    for _ in 0..1500 {
        let places: Vec<(&str, i64)> = names.iter().map(|&p| (p, next(3) as i64)).collect();
        let tnames = ["t0", "t1", "t2", "t3", "t4"];
        let mut sides: Vec<(Vec<&str>, Vec<&str>)> = Vec::new();
        for _ in 0..tnames.len() {
            let mut ins = Vec::new();
            let mut outs = Vec::new();
            for &p in &names {
                match next(5) {
                    0 => ins.push(p),
                    1 => outs.push(p),
                    _ => {}
                }
            }
            sides.push((ins, outs));
        }
        let flows: Vec<(&str, &[&str], &[&str])> = tnames
            .iter()
            .zip(&sides)
            .map(|(t, (i, o))| (*t, i.as_slice(), o.as_slice()))
            .collect();
        let m = model(&places, &flows);

        let az = Analyzer::new(&m).with_max_tokens(8).with_max_states(400);
        let report = az.l4_liveness();
        if report.truncated {
            continue;
        }
        checked += 1;

        let built = az.build_graph();
        let g = &built.graph;
        let can_reach_enabling = |from: &str, t: &str| {
            let mut seen = HashSet::from([from.to_string()]);
            let mut stack = vec![from.to_string()];
            while let Some(k) = stack.pop() {
                let s = &g.states[&k];
                if s.enabled.iter().any(|e| e == t) {
                    return true;
                }
                for &e in &s.successors {
                    let to = &g.edges[e].to;
                    if seen.insert(to.clone()) {
                        stack.push(to.clone());
                    }
                }
            }
            false
        };
        for t in &report.transitions {
            let expected = g.states.keys().all(|k| can_reach_enabling(k, &t.transition));
            assert_eq!(t.status == L4Status::Live, expected, "{} on {m:?}", t.transition);
            if let Some(w) = &t.witness {
                assert_replays(&m, &w.trace, &w.marking);
                assert!(!can_reach_enabling(&pflow_reachability::marking::key(&w.marking), &t.transition));
                not_live += 1;
            } else {
                live += 1;
            }
        }
    }
    // Guard the test itself: it has to have seen both answers on real nets.
    assert!(checked > 100, "checked {checked}");
    assert!(live > 50 && not_live > 50, "live {live}, not live {not_live}");
}

/// Two philosophers, each taking their left fork first. Every transition fires
/// somewhere, and both holding a fork is a dead marking.
fn philosophers() -> Model {
    model(
        &[
            ("fork1", 1),
            ("fork2", 1),
            ("p1_thinking", 1),
            ("p1_has_left", 0),
            ("p1_eating", 0),
            ("p2_thinking", 1),
            ("p2_has_left", 0),
            ("p2_eating", 0),
        ],
        &[
            ("p1_take_left", &["p1_thinking", "fork1"], &["p1_has_left"]),
            ("p1_take_right", &["p1_has_left", "fork2"], &["p1_eating"]),
            ("p1_release", &["p1_eating"], &["p1_thinking", "fork1", "fork2"]),
            ("p2_take_left", &["p2_thinking", "fork2"], &["p2_has_left"]),
            ("p2_take_right", &["p2_has_left", "fork1"], &["p2_eating"]),
            ("p2_release", &["p2_eating"], &["p2_thinking", "fork1", "fork2"]),
        ],
    )
}

#[test]
fn analyze_says_a_deadlocking_net_is_not_live() {
    let m = philosophers();
    let r = Analyzer::new(&m).analyze();
    assert!(r.is_complete);
    assert!(r.has_deadlock);
    assert!(!r.live, "a reachable dead marking is not live");
    assert!(r.quasi_live, "the old meaning of live still holds here");
    let l = &r.liveness;
    assert_eq!(l.deadlock_free, Verdict::False);
    assert_eq!(l.live, Verdict::False);
    assert_eq!(l.quasi_live, Verdict::True);
    assert_eq!(l.not_live.len(), 6);

    let w = l.witness.as_ref().expect("a reachable dead marking comes with a witness");
    assert_eq!(w.kind, WitnessKind::Deadlock);
    assert_eq!(w.region_size, 1);
    assert_eq!(w.trace, vec!["p1_take_left".to_string(), "p2_take_left".to_string()]);
    assert_eq!(w.marking["p1_has_left"], 1);
    assert_eq!(w.marking["p2_has_left"], 1);
    assert_eq!(w.never_fires.len(), 6);
    assert_replays(&m, &w.trace, &w.marking);
}

#[test]
fn analyze_calls_ordered_forks_live() {
    let m = model(
        &[
            ("fork1", 1),
            ("fork2", 1),
            ("p1_thinking", 1),
            ("p1_has_first", 0),
            ("p1_eating", 0),
            ("p2_thinking", 1),
            ("p2_has_first", 0),
            ("p2_eating", 0),
        ],
        &[
            ("p1_first", &["p1_thinking", "fork1"], &["p1_has_first"]),
            ("p1_second", &["p1_has_first", "fork2"], &["p1_eating"]),
            ("p1_release", &["p1_eating"], &["p1_thinking", "fork1", "fork2"]),
            ("p2_first", &["p2_thinking", "fork1"], &["p2_has_first"]),
            ("p2_second", &["p2_has_first", "fork2"], &["p2_eating"]),
            ("p2_release", &["p2_eating"], &["p2_thinking", "fork1", "fork2"]),
        ],
    );
    let r = Analyzer::new(&m).analyze();
    assert!(r.live);
    assert_eq!(r.liveness.deadlock_free, Verdict::True);
    assert_eq!(r.liveness.live, Verdict::True);
    assert_eq!(r.liveness.quasi_live, Verdict::True);
    assert!(r.liveness.witness.is_none());
}

/// A closed cycle shutting out the rest: no dead marking, every transition fires
/// once, and still not live.
#[test]
fn analyze_finds_a_livelock_no_deadlock_check_sees() {
    let m = model(
        &[("a", 1), ("b", 0), ("x", 0), ("y", 0)],
        &[
            ("go", &["a"], &["b"]),
            ("back", &["b"], &["a"]),
            ("trap", &["a"], &["x"]),
            ("xy", &["x"], &["y"]),
            ("yx", &["y"], &["x"]),
        ],
    );
    let l = Analyzer::new(&m).analyze().liveness;
    assert_eq!(l.deadlock_free, Verdict::True);
    assert_eq!(l.quasi_live, Verdict::True);
    assert_eq!(l.live, Verdict::False);
    let w = l.witness.unwrap();
    assert_eq!(w.kind, WitnessKind::Livelock);
    assert_eq!(w.region_size, 2);
    assert_eq!(w.never_fires, vec!["go".to_string(), "back".to_string(), "trap".to_string()]);
    assert_replays(&m, &w.trace, &w.marking);
}

/// Draining to empty is a dead marking by the definition, though `has_deadlock`'s
/// heuristic accepts it as an end state.
#[test]
fn analyze_counts_an_empty_dead_marking() {
    let m = model(&[("a", 1)], &[("drain", &["a"], &[])]);
    let r = Analyzer::new(&m).analyze();
    assert!(!r.has_deadlock);
    assert_eq!(r.dead_markings.len(), 1);
    assert_eq!(r.liveness.deadlock_free, Verdict::False);
    assert_eq!(r.liveness.live, Verdict::False);
    assert_eq!(r.liveness.quasi_live, Verdict::True);
}

/// No transition at all: the one marking is dead, and "every transition" holds
/// vacuously, the one case where live does not imply deadlock-free.
#[test]
fn analyze_with_no_transitions_is_vacuously_live() {
    let m = model(&[("a", 1)], &[]);
    let l = Analyzer::new(&m).analyze().liveness;
    assert_eq!(l.deadlock_free, Verdict::False);
    assert_eq!(l.live, Verdict::True);
    assert_eq!(l.quasi_live, Verdict::True);
    assert!(l.witness.unwrap().trace.is_empty());
}

#[test]
fn analyze_never_claims_true_when_truncated() {
    let m = model(&[("q", 0)], &[("arrive", &[], &["q"])]);
    for r in [
        Analyzer::new(&m).with_max_states(50).analyze(),
        Analyzer::new(&m).with_max_tokens(5).analyze(),
    ] {
        assert!(!r.is_complete);
        assert_eq!(r.liveness.deadlock_free, Verdict::Unknown);
        assert_eq!(r.liveness.live, Verdict::Unknown);
        assert_eq!(r.liveness.quasi_live, Verdict::Unknown);
        assert!(!r.live && !r.quasi_live);
        assert!(r.liveness.witness.is_none());
    }
}

/// A dead marking reached before the bound is real however little was explored.
#[test]
fn analyze_keeps_a_real_deadlock_when_truncated() {
    let m = model(
        &[("start", 1), ("dead", 0), ("q", 0), ("run", 0)],
        &[
            ("stop", &["start"], &["dead"]),
            ("begin", &["start"], &["run"]),
            ("arrive", &["run"], &["run", "q"]),
        ],
    );
    let r = Analyzer::new(&m).with_max_states(50).analyze();
    assert!(!r.is_complete);
    assert_eq!(r.liveness.deadlock_free, Verdict::False);
    assert_eq!(r.liveness.live, Verdict::False);
    assert_eq!(r.liveness.quasi_live, Verdict::Unknown);
    let w = r.liveness.witness.unwrap();
    assert_replays(&m, &w.trace, &w.marking);
}

/// A live ring cut by the bound: what the search did not expand refutes nothing.
#[test]
fn analyze_does_not_refute_from_an_unexpanded_region() {
    let m = model(
        &[("a", 1), ("b", 0), ("c", 0), ("d", 0)],
        &[("ab", &["a"], &["b"]), ("bc", &["b"], &["c"]), ("cd", &["c"], &["d"]), ("da", &["d"], &["a"])],
    );
    let r = Analyzer::new(&m).with_max_states(2).analyze();
    assert!(!r.is_complete);
    assert_eq!(r.liveness.live, Verdict::Unknown);
    assert!(r.liveness.witness.is_none());
    assert_eq!(Analyzer::new(&m).analyze().liveness.live, Verdict::True);
}

/// `analyze`'s verdict and `Kind::L4Live` must be one answer, on any net.
#[test]
fn analyze_agrees_with_kind_l4_live() {
    for m in [philosophers(), cycle(), netting()] {
        let live = Analyzer::new(&m).analyze().live;
        assert_eq!(l4(&m).status == Status::Proved, live, "{:?}", m.transitions.len());
    }
}
