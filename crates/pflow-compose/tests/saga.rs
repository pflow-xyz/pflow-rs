//! A port of go-pflow's `saga/metasubnet_test.go` and the structure and
//! validation half of `saga/saga_test.go` (go-pflow `c802f34`; the package
//! was added in `4a827f4`): every case, with the same sagas and the same
//! numbers. go-pflow ships no saga golden or testdata, so there is no file to
//! vendor through `go-pflow.lock`; the cases themselves are the contract,
//! the same arrangement as `pflow-mining`'s `conformance_gates_tests.rs`.
//!
//! go-pflow analyses a bundle through `metapetri.ConvertBundle` and its
//! `reachability`/`verify` packages; here the bundle is lowered with
//! [`Bundle::flatten_with_map`] and analysed with `pflow-reachability` and
//! `pflow-verify` over the resulting `Model`. go-pflow's `saga/analyze.go`
//! (`Stranded`) is not part of the library port; [`stranded`] below is the
//! same backward search over the graph's predecessor edges.

use std::collections::{HashMap, HashSet};

use pflow_compose::bundle::{Bundle, Endpoint, FlattenMap, Link, LinkKind, NetType, Subnet};
use pflow_compose::{
    account_port, compensated_place, done_place, failed_place, new_queue, new_resource_pool,
    pending_place, undo_transition, unwind_place, Account, QueueSpec, Saga, SagaBuilder, Step,
    SAGA_COMPLETED, SAGA_FAIL_COMMIT, SAGA_ROLLED_BACK,
};
use pflow_metamodel::{Arc, Model, Place, StateKind, Transition};
use pflow_reachability::{AnalysisResult, Analyzer, Graph, InvariantAnalyzer, Marking, State};
use pflow_verify::{Kind, Method, Property, Status, Verifier};

// ---------------------------------------------------------------------------
// helpers (metasubnet_test.go:17-69, saga_test.go:8-13, analyze.go)
// ---------------------------------------------------------------------------

/// `transfer` (saga_test.go:8).
fn transfer() -> Saga {
    SagaBuilder::new("pay")
        .account("a", 10)
        .account("b", 0)
        .step("one")
        .move_tokens("a", "b", 3)
        .done()
        .must_build()
}

/// `chain` (metasubnet_test.go:19): an n-step saga that carries one unit
/// down a line of accounts; step i moves it from acct{i-1} to acct{i}.
fn chain(n: usize) -> Saga {
    let mut b = SagaBuilder::new(format!("chain{n}")).account("acct0", 1);
    for i in 1..=n {
        b = b.account(format!("acct{i}"), 0);
    }
    for i in 1..=n {
        b = b
            .step(format!("s{i}"))
            .move_tokens(format!("acct{}", i - 1), format!("acct{i}"), 1)
            .done();
    }
    b.must_build()
}

fn step_id(i: usize) -> String {
    format!("s{i}")
}

fn get(m: &Marking, p: &str) -> i64 {
    m.get(p).copied().unwrap_or(0)
}

/// A flattened bundle and the map from subnet-local to flat IDs.
struct Flat {
    model: Model,
    map: FlattenMap,
}

impl Flat {
    /// `analysed.flat` (metasubnet_test.go:38): resolves a subnet-local ID
    /// to the ID the analysed net uses.
    fn flat(&self, subnet: &str, local: &str) -> String {
        self.map
            .place
            .get(subnet)
            .and_then(|m| m.get(local))
            .cloned()
            .unwrap_or_else(|| local.to_string())
    }

    /// `analyse`'s state-space half (metasubnet_test.go:47): the space must
    /// be complete, or every verdict below would be partial.
    fn space(&self) -> AnalysisResult<'_> {
        let space = Analyzer::new(&self.model)
            .with_max_states(200_000)
            .analyze();
        assert!(
            space.is_complete,
            "state space truncated at {} states",
            space.state_count
        );
        space
    }

    fn verify(&self, p: Property) -> pflow_verify::Verdict {
        Verifier::new(&self.model)
            .with_max_states(200_000)
            .check_one(p)
    }
}

/// `analyse` (metasubnet_test.go:47): validate, lower, and check the lowering
/// is lossless. go-pflow asserts metapetri's diagnostics neither over- nor
/// under-approximate; the analogue here is that no guard was introduced,
/// since `pflow-verify` would explore a guarded transition as unguarded.
fn analyse(b: &Bundle) -> Flat {
    let v = b.validate();
    assert!(v.valid, "bundle does not validate: {:?}", v.errors);
    let (model, map) = b.flatten_with_map().expect("flatten");
    for t in &model.transitions {
        assert!(
            t.guard.is_empty(),
            "a saga carries no guards, but {} has {:?}",
            t.id,
            t.guard
        );
    }
    Flat { model, map }
}

/// `standalone` (metasubnet_test.go:66).
fn standalone(s: &Saga) -> Flat {
    let mut b = Bundle::new(s.id.clone());
    b.add_subnet(s.to_meta_subnet());
    analyse(&b)
}

/// `saga.Stranded` (go-pflow `saga/analyze.go`): the reachable states from
/// which none of `places` can ever be marked, by a backward search from the
/// states that mark one of them.
fn stranded<'g>(g: &'g Graph<'_>, places: &[String]) -> Vec<&'g State> {
    let mut can: HashSet<&str> = HashSet::new();
    let mut frontier: Vec<&str> = Vec::new();
    for st in g.states_list() {
        if places.iter().any(|p| get(&st.marking, p) > 0) {
            can.insert(&st.key);
            frontier.push(&st.key);
        }
    }
    while let Some(key) = frontier.pop() {
        for &e in &g.states[key].predecessors {
            let from = g.edges[e].from.as_str();
            if can.insert(from) {
                frontier.push(from);
            }
        }
    }
    g.states_list()
        .into_iter()
        .filter(|st| !can.contains(st.key.as_str()))
        .collect()
}

/// Whether the Farkas basis of `model` holds an invariant whose support is
/// exactly `places`, each with coefficient 1.
fn has_unit_invariant(model: &Model, places: &[String]) -> bool {
    InvariantAnalyzer::new(model)
        .find_p_invariants(&model.initial_marking())
        .iter()
        .any(|inv| {
            inv.coefficients.len() == places.len()
                && places.iter().all(|p| inv.coefficients.get(p) == Some(&1))
        })
}

// ---------------------------------------------------------------------------
// saga_test.go
// ---------------------------------------------------------------------------

/// `TestBuilderKeepsStepOrder` (saga_test.go:15): compensation order is
/// derived from the order steps were added.
#[test]
fn builder_keeps_step_order() {
    let s = SagaBuilder::new("s")
        .step("c")
        .done()
        .step("a")
        .done()
        .step("b")
        .done()
        .must_build();
    let got: Vec<&str> = s.steps.iter().map(|st| st.id.as_str()).collect();
    assert_eq!(got, ["c", "a", "b"]);
}

/// `TestBuildValidatedRejectsIllFormedSagas` (saga_test.go:31).
#[test]
fn build_validated_rejects_ill_formed_sagas() {
    let shared_with_balance = Saga {
        id: "s".into(),
        steps: vec![Step {
            id: "x".into(),
            ..Default::default()
        }],
        accounts: [(
            "a".to_string(),
            Account {
                id: "a".into(),
                shared: true,
                initial: 5,
            },
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let cases: Vec<(&str, Result<Saga, String>, &str)> = vec![
        (
            "no steps",
            SagaBuilder::new("s").build_validated(),
            "no steps",
        ),
        (
            "no id",
            SagaBuilder::new("").step("x").done().build_validated(),
            "no ID",
        ),
        (
            "duplicate step",
            SagaBuilder::new("s")
                .step("x")
                .done()
                .step("x")
                .done()
                .build_validated(),
            r#"duplicate step "x""#,
        ),
        (
            "empty step id",
            SagaBuilder::new("s").step("").done().build_validated(),
            "a step has no ID",
        ),
        (
            "step id with space",
            SagaBuilder::new("s").step("a b").done().build_validated(),
            "whitespace",
        ),
        (
            "duplicate account",
            SagaBuilder::new("s")
                .account("a", 1)
                .account("a", 2)
                .step("x")
                .done()
                .build_validated(),
            r#"duplicate account "a""#,
        ),
        (
            "unknown source",
            SagaBuilder::new("s")
                .account("b", 0)
                .step("x")
                .move_tokens("a", "b", 1)
                .done()
                .build_validated(),
            r#"unknown account "a""#,
        ),
        (
            "unknown target",
            SagaBuilder::new("s")
                .account("a", 1)
                .step("x")
                .move_tokens("a", "b", 1)
                .done()
                .build_validated(),
            r#"unknown account "b""#,
        ),
        (
            "zero amount",
            SagaBuilder::new("s")
                .account("a", 1)
                .account("b", 0)
                .step("x")
                .move_tokens("a", "b", 0)
                .done()
                .build_validated(),
            "must be positive",
        ),
        (
            "self move",
            SagaBuilder::new("s")
                .account("a", 1)
                .step("x")
                .move_tokens("a", "a", 1)
                .done()
                .build_validated(),
            "both",
        ),
        (
            "negative balance",
            SagaBuilder::new("s")
                .account("a", -1)
                .step("x")
                .done()
                .build_validated(),
            "negative",
        ),
        (
            "shared with a balance",
            shared_with_balance
                .validate()
                .map(|_| shared_with_balance.clone()),
            "shared account",
        ),
        // Two steps must never share a generated place. next_x is step x's,
        // so an account called next_x would silently become its control
        // place.
        (
            "account collides with a control place",
            SagaBuilder::new("s")
                .account("next_x", 1)
                .step("x")
                .done()
                .build_validated(),
            "collides",
        ),
        (
            "account collides with a terminal",
            SagaBuilder::new("s")
                .account("completed", 1)
                .step("x")
                .done()
                .build_validated(),
            "collides",
        ),
        // next_q_done is step "next_q"'s done place and step "q_done"'s next
        // place.
        (
            "steps whose generated ids overlap",
            SagaBuilder::new("s")
                .step("next_q")
                .done()
                .step("q_done")
                .done()
                .build_validated(),
            "collides",
        ),
    ];
    for (name, got, want) in cases {
        match got {
            Ok(_) => panic!("{name}: accepted; want an error mentioning {want:?}"),
            Err(e) => assert!(
                e.contains(want),
                "{name}: error {e:?} does not mention {want:?}"
            ),
        }
    }
}

/// `TestMustBuildPanicsOnIllFormed` (saga_test.go:70).
#[test]
#[should_panic(expected = "no steps")]
fn must_build_panics_on_ill_formed() {
    SagaBuilder::new("s").must_build();
}

/// `TestToMetaModelPanicsOnIllFormed` (saga_test.go:79).
#[test]
#[should_panic(expected = "duplicate step")]
fn to_meta_model_panics_on_ill_formed() {
    SagaBuilder::new("s")
        .step("x")
        .done()
        .step("x")
        .done()
        .build()
        .to_meta_model();
}

// `TestNetDeltaNetsOutPassThrough` (saga_test.go:88) exercises an
// unexported helper; it is ported as a unit test inside `src/saga.rs`.

/// `TestNamingHelpers` (saga_test.go:102).
#[test]
fn naming_helpers() {
    assert_eq!(pending_place("x"), "x_pending");
    assert_eq!(undo_transition("x"), "undo_x");
    assert_eq!(account_port("a"), "account:a");
    let _ = transfer();
}

// ---------------------------------------------------------------------------
// metasubnet_test.go
// ---------------------------------------------------------------------------

/// `TestMetaSubnetShape` (metasubnet_test.go:71).
#[test]
fn meta_subnet_shape() {
    let s = SagaBuilder::new("w")
        .account("funds", 5)
        .account("held", 0)
        .step("reserve")
        .move_tokens("funds", "held", 2)
        .done()
        .step("sign")
        .done()
        .must_build();
    let sub = s.to_meta_subnet();

    assert_eq!(
        sub.net_type,
        NetType::Resource,
        "it is the type that lets the account ports token-link"
    );

    let want_places: HashMap<&str, i64> = [
        ("next_reserve", 1),
        ("next_sign", 0),
        ("committing", 0),
        ("unwind_reserve", 0),
        ("unwind_sign", 0),
        ("completed", 0),
        ("rolled_back", 0),
        ("reserve_pending", 1),
        ("reserve_done", 0),
        ("reserve_failed", 0),
        ("reserve_compensated", 0),
        ("sign_pending", 1),
        ("sign_done", 0),
        ("sign_failed", 0),
        ("sign_compensated", 0),
        ("funds", 5),
        ("held", 0),
    ]
    .into_iter()
    .collect();
    assert_eq!(sub.model.places.len(), want_places.len());
    for (id, initial) in &want_places {
        let p = sub
            .model
            .place_by_id(id)
            .unwrap_or_else(|| panic!("missing place {id}"));
        assert_eq!(p.initial, *initial, "{id}");
    }

    for id in [
        "do_reserve",
        "fail_reserve",
        "undo_reserve",
        "do_sign",
        "fail_sign",
        "undo_sign",
        "commit",
        "fail_commit",
    ] {
        assert!(
            sub.model.transition_by_id(id).is_some(),
            "missing transition {id}"
        );
    }
    assert_eq!(
        sub.model.transitions.len(),
        8,
        "3 per step + commit + fail_commit"
    );

    // The reserve step's weight is one arc each way, not two parallel arcs.
    let weights = |from: &str, to: &str| -> Vec<i64> {
        sub.model
            .arcs
            .iter()
            .filter(|a| a.from == from && a.to == to)
            .map(|a| a.weight)
            .collect()
    };
    assert_eq!(weights("funds", "do_reserve"), [2]);
    assert_eq!(weights("undo_reserve", "funds"), [2]);

    let mut b = Bundle::new("w");
    b.add_subnet(sub);
    let v = b.validate();
    assert!(v.valid, "saga subnet does not validate: {:?}", v.errors);
}

/// `TestMetaSubnetPorts` (metasubnet_test.go:136).
#[test]
fn meta_subnet_ports() {
    let sub = SagaBuilder::new("w")
        .account("funds", 5)
        .shared_account("chain")
        .step("sign")
        .done()
        .must_build()
        .to_meta_subnet();

    let want = [
        ("account:funds", false),
        ("account:chain", false),
        ("completed", false),
        ("rolled_back", false),
        ("do:sign", true),
        ("fail:sign", true),
        ("undo:sign", true),
        ("commit", true),
        ("fail_commit", true),
    ];
    for (id, is_transition) in want {
        let p = sub
            .port_by_id(id)
            .unwrap_or_else(|| panic!("missing port {id}"));
        assert_eq!(p.is_transition(), is_transition, "port {id}");
    }
    assert_eq!(sub.ports.len(), want.len());
    for id in ["funds", "chain", "completed", "rolled_back"] {
        assert!(
            sub.model.place_by_id(id).is_some_and(|p| p.exported),
            "place {id} backs a port and must be exported"
        );
    }
    assert!(
        !sub.model.place_by_id("next_sign").unwrap().exported,
        "control places are internal; exporting one invites a token link into the cursor"
    );
}

/// `TestMetaSubnetDeterministic` (metasubnet_test.go:171).
#[test]
fn meta_subnet_deterministic() {
    let first = serde_json::to_string(&chain(4).to_meta_subnet()).unwrap();
    for i in 0..20 {
        let next = serde_json::to_string(&chain(4).to_meta_subnet()).unwrap();
        assert_eq!(
            next, first,
            "run {i} marshals differently: account iteration order leaked into the output"
        );
    }
}

/// `TestStateSpaceIsExactlyTheSagaProtocol` (metasubnet_test.go:190): for n
/// steps the reachable markings are the forward run (n+2), the failure of
/// step k followed by its k-1 compensations (k each), and the failed commit
/// followed by n compensations (n+1).
#[test]
fn state_space_is_exactly_the_saga_protocol() {
    for n in 1..=6 {
        let want = (n + 2) + n * (n + 1) / 2 + (n + 1);
        let a = standalone(&chain(n));
        assert_eq!(a.space().state_count, want, "n={n}");
    }
}

/// `TestCompensationRunsInReverse` (metasubnet_test.go:203).
#[test]
fn compensation_runs_in_reverse() {
    const N: usize = 5;
    let a = standalone(&chain(N));
    let space = a.space();
    let g = &space.graph;

    for st in g.states_list() {
        for i in 1..=N {
            if get(&st.marking, &unwind_place(&step_id(i))) == 0 {
                continue;
            }
            // The token is on unwind_i: undo_i is the only move the saga has.
            assert_eq!(
                st.enabled,
                vec![undo_transition(&step_id(i))],
                "token on unwind_s{i}"
            );
            // Everything above i has already been compensated or never ran.
            for j in i + 1..=N {
                assert_eq!(
                    get(&st.marking, &done_place(&step_id(j))),
                    0,
                    "unwinding s{i} while s{j} is still done"
                );
            }
            // Everything at or below i is still waiting for its undo.
            for j in 1..=i {
                assert_eq!(
                    get(&st.marking, &done_place(&step_id(j))),
                    1,
                    "unwinding s{i} but s{j} is not done"
                );
            }
        }
    }

    // Every undo_i must leave the token on unwind_{i-1} (or rolled_back).
    for e in &g.edges {
        let Some(i) = e.transition.strip_prefix("undo_s") else {
            continue;
        };
        let i: usize = i.parse().unwrap();
        let next = if i > 1 {
            unwind_place(&step_id(i - 1))
        } else {
            SAGA_ROLLED_BACK.to_string()
        };
        assert_eq!(
            get(&g.states[&e.to].marking, &next),
            1,
            "{} did not hand the token to {next}",
            e.transition
        );
    }
}

/// `TestNoStrandedState` (metasubnet_test.go:254): from every reachable
/// marking that has not completed, the fully rolled-back terminal is
/// reachable.
#[test]
fn no_stranded_state() {
    for n in 1..=5 {
        let a = standalone(&chain(n));
        let space = a.space();
        let g = &space.graph;

        let failed_any = |m: &Marking| {
            (1..=n).any(|i| get(m, &failed_place(&step_id(i))) > 0) || get(m, SAGA_ROLLED_BACK) > 0
        };
        let partial = g
            .states_list()
            .into_iter()
            .filter(|st| failed_any(&st.marking))
            .count();
        assert!(
            partial > 0,
            "n={n}: no partial-failure states found; the test would be vacuous"
        );

        let terminals = [SAGA_ROLLED_BACK.to_string(), SAGA_COMPLETED.to_string()];
        let s = stranded(g, &terminals);
        assert!(
            s.is_empty(),
            "n={n}: {} stranded states, e.g. {:?}",
            s.len(),
            s.first().map(|s| &s.marking)
        );

        // Stronger: only the completed terminal is exempt from reaching
        // rolled_back.
        for st in stranded(g, &[SAGA_ROLLED_BACK.to_string()]) {
            assert_eq!(
                get(&st.marking, SAGA_COMPLETED),
                1,
                "n={n}: {:?} cannot reach rolled_back",
                st.marking
            );
        }
    }
}

/// `TestOnlyTerminalsAreDeadEnds` (metasubnet_test.go:296).
#[test]
fn only_terminals_are_dead_ends() {
    let a = standalone(&chain(4));
    let space = a.space();
    let (mut completed, mut rolled_back) = (0, 0);
    let mut terminals = 0;
    for st in space.graph.states_list() {
        if !st.is_terminal {
            continue;
        }
        terminals += 1;
        if get(&st.marking, SAGA_COMPLETED) == 1 {
            completed += 1;
        } else if get(&st.marking, SAGA_ROLLED_BACK) == 1 {
            rolled_back += 1;
        } else {
            panic!("wedged in {:?}", st.marking);
        }
    }
    // go-pflow first asserts `HasDeadlock` ("a saga has terminal markings;
    // the analyser reported none"); pflow-reachability reserves
    // `has_deadlock` for terminals that are not accepted end states, so the
    // claim being made there — terminal markings exist — is asserted
    // directly.
    assert!(
        terminals > 0,
        "a saga has terminal markings; the analyser reported none"
    );
    assert_eq!(completed, 1);
    // One per failure point: step 1..n failing, plus the failed commit.
    assert_eq!(rolled_back, 5);
}

/// `TestTerminalsAreMutuallyExclusive` (metasubnet_test.go:327): checked
/// structurally through verify, by walking the state space, and by deriving
/// the single-control-token invariant with Farkas.
#[test]
fn terminals_are_mutually_exclusive() {
    let a = standalone(&chain(4));

    let vd = a.verify(Property::mutual_exclusion(
        vec![SAGA_COMPLETED.into(), SAGA_ROLLED_BACK.into()],
        1,
    ));
    assert_eq!(
        (vd.status, vd.method),
        (Status::Proved, Method::Structural),
        "{}",
        vd.detail
    );

    for st in a.space().graph.states_list() {
        assert!(
            get(&st.marking, SAGA_COMPLETED) + get(&st.marking, SAGA_ROLLED_BACK) <= 1,
            "{:?}",
            st.marking
        );
    }

    assert!(
        has_unit_invariant(&a.model, &chain(4).control_places()),
        "the single-control-token P-invariant is not derivable from the emitted net"
    );
}

/// `TestConservationAcrossAccounts` (metasubnet_test.go:371).
#[test]
fn conservation_across_accounts() {
    const N: usize = 4;
    let a = standalone(&chain(N));

    let accounts: Vec<String> = (0..=N).map(|i| format!("acct{i}")).collect();
    let expr = format!("{} == 1", accounts.join(" + "));
    let vd = a.verify(Property::invariant(expr.clone()));
    assert_eq!(vd.status, Status::Proved, "{expr}: {}", vd.detail);

    // The emitted constraint says the same thing.
    let model = chain(N).to_meta_model();
    let c = model
        .constraints
        .iter()
        .find(|c| c.id == "saga_conservation")
        .expect("no saga_conservation constraint");
    for acct in &accounts {
        assert!(
            c.expr.contains(&format!("tokens({acct:?})")),
            "saga_conservation {:?} omits {acct}",
            c.expr
        );
    }
    assert!(
        c.expr.ends_with("== 1"),
        "saga_conservation {:?} should total the initial 1",
        c.expr
    );

    // completed has moved the unit to the end of the line; rolled_back has
    // put every unit back where it began.
    for st in a.space().graph.states_list() {
        if get(&st.marking, SAGA_COMPLETED) == 1 {
            assert_eq!(get(&st.marking, &format!("acct{N}")), 1, "{:?}", st.marking);
        } else if get(&st.marking, SAGA_ROLLED_BACK) == 1 {
            assert_eq!(get(&st.marking, "acct0"), 1, "{:?}", st.marking);
        }
    }
}

/// `TestEveryStepIsInExactlyOneState` (metasubnet_test.go:425).
#[test]
fn every_step_is_in_exactly_one_state() {
    let a = standalone(&chain(4));
    for i in 1..=4 {
        let id = step_id(i);
        let expr = format!(
            "{} + {} + {} + {} == 1",
            pending_place(&id),
            done_place(&id),
            failed_place(&id),
            compensated_place(&id)
        );
        let vd = a.verify(Property::invariant(expr.clone()));
        assert_eq!(
            (vd.status, vd.method),
            (Status::Proved, Method::Structural),
            "{expr}"
        );
    }
}

/// `TestNoDeadTransitions` (metasubnet_test.go:441).
#[test]
fn no_dead_transitions() {
    for n in 1..=5 {
        let a = standalone(&chain(n));
        assert!(
            a.space().dead_trans.is_empty(),
            "n={n}: dead transitions {:?}",
            a.space().dead_trans
        );
        let vd = a.verify(Property::new(Kind::Live));
        assert_eq!(vd.status, Status::Proved, "n={n}: live: {}", vd.detail);
    }
}

/// `TestFinalCommitIsWhatMakesTheLastCompensationReachable`
/// (metasubnet_test.go:460): remove fail_commit and undo_sn goes dead.
#[test]
fn final_commit_is_what_makes_the_last_compensation_reachable() {
    let s = chain(3);
    let full = standalone(&s);
    assert!(
        full.space().dead_trans.is_empty(),
        "baseline has dead transitions"
    );

    let mut m = s.to_meta_model();
    m.arcs
        .retain(|a| a.from != SAGA_FAIL_COMMIT && a.to != SAGA_FAIL_COMMIT);
    let space = Analyzer::new(&m).analyze();
    let dead: HashSet<&str> = space.dead_trans.iter().map(String::as_str).collect();
    assert!(
        dead.contains(undo_transition("s3").as_str()),
        "without fail_commit, undo_s3 should be dead; {dead:?}"
    );
    assert!(
        !dead.contains(undo_transition("s2").as_str())
            && !dead.contains(undo_transition("s1").as_str()),
        "undo_s2 and undo_s1 are still triggered by later steps failing; {dead:?}"
    );
}

/// `TestEveryEmittedConstraintIsATrueInvariant` (metasubnet_test.go:500):
/// every constraint the saga emits is provable of the net it emits.
#[test]
fn every_emitted_constraint_is_a_true_invariant() {
    let tokens = regex::Regex::new(r#"tokens\("([^"]+)"\)"#).unwrap();
    for s in [chain(1), chain(3), transfer()] {
        let a = standalone(&s);
        let cons = s.to_meta_model().constraints;
        assert!(
            cons.len() >= 2,
            "{}: only {} constraints emitted",
            s.id,
            cons.len()
        );
        for c in cons {
            let expr = tokens.replace_all(&c.expr, "$1").into_owned();
            let vd = a.verify(Property {
                name: c.id.clone(),
                ..Property::invariant(expr.clone())
            });
            assert_eq!(
                vd.status,
                Status::Proved,
                "{}: constraint {} ({expr}): {}",
                s.id,
                c.id,
                vd.detail
            );
        }
    }
}

/// `TestSharedAccountOmitsLocalConservation` (metasubnet_test.go:522).
#[test]
fn shared_account_omits_local_conservation() {
    let s = SagaBuilder::new("s")
        .account("own", 3)
        .shared_account("shared")
        .step("x")
        .move_tokens("own", "shared", 1)
        .done()
        .must_build();
    for c in s.to_meta_model().constraints {
        assert_ne!(
            c.id, "saga_conservation",
            "closed-saga conservation {:?} claimed with a shared account",
            c.expr
        );
    }
    s.validate().unwrap();
}

fn token_link(from_subnet: &str, from_port: &str, to_subnet: &str, to_place: &str) -> Link {
    Link {
        kind: LinkKind::Token,
        from: Endpoint {
            subnet: from_subnet.into(),
            port: from_port.into(),
            ..Default::default()
        },
        to: Endpoint {
            subnet: to_subnet.into(),
            place: to_place.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn exported_token(id: &str, initial: i64) -> Place {
    Place {
        id: id.into(),
        kind: Some(StateKind::Token),
        initial,
        exported: true,
        ..Default::default()
    }
}

/// `pairBundle` (metasubnet_test.go:537): two sagas on one shared pool
/// through TokenLinks. Each owns its escrow; both draw on `pool`.
fn pair_bundle(balance: i64, amount_a: i64, amount_b: i64) -> Bundle {
    let mk = |id: &str, amount| {
        SagaBuilder::new(id)
            .shared_account("pool")
            .account("escrow", 0)
            .account("out", 0)
            .step("hold")
            .move_tokens("pool", "escrow", amount)
            .done()
            .step("pay")
            .move_tokens("escrow", "out", amount)
            .done()
            .must_build()
    };
    let mut b = Bundle::new("pair");
    b.add_subnet(mk("a", amount_a).to_meta_subnet());
    b.add_subnet(mk("b", amount_b).to_meta_subnet());
    b.add_subnet(Subnet {
        id: "bank".into(),
        net_type: NetType::Resource,
        model: Model {
            name: "bank".into(),
            places: vec![exported_token("pool", balance)],
            ..Default::default()
        },
        ..Default::default()
    });
    for id in ["a", "b"] {
        b.add_link(token_link(id, &account_port("pool"), "bank", "pool"));
    }
    b
}

/// `TestTwoSagasShareAnAccount` (metasubnet_test.go:564): neither saga can
/// overdraw the pool, tokens are conserved across all of them, and a saga
/// that cannot get its funds rolls back rather than wedging.
#[test]
fn two_sagas_share_an_account() {
    // 100 in the pool, 60 asked by each: both cannot be served.
    let a = analyse(&pair_bundle(100, 60, 60));

    let pool = a.flat("bank", "pool");
    assert_eq!(
        a.flat("a", "pool"),
        pool,
        "the token link did not fuse a's pool with the bank's"
    );
    assert_eq!(
        a.flat("b", "pool"),
        pool,
        "the token link did not fuse b's pool with the bank's"
    );

    let held = vec![
        pool.clone(),
        a.flat("a", "escrow"),
        a.flat("a", "out"),
        a.flat("b", "escrow"),
        a.flat("b", "out"),
    ];
    assert!(
        has_unit_invariant(&a.model, &held),
        "pool + escrows + outs is not derivable as a P-invariant of the composed net"
    );
    let space = a.space();
    let g = &space.graph;
    for st in g.states_list() {
        let total: i64 = held.iter().map(|p| get(&st.marking, p)).sum();
        assert_eq!(
            total, 100,
            "tokens across the bank and both sagas in {:?}",
            st.marking
        );
    }

    // The contention is real: some run ends with a completed and b rolled
    // back.
    let split = g.states_list().iter().any(|st| {
        get(&st.marking, &a.flat("a", SAGA_COMPLETED)) == 1
            && get(&st.marking, &a.flat("b", SAGA_ROLLED_BACK)) == 1
    });
    assert!(
        split,
        "no marking has a completed and b rolled back; the sagas never contended"
    );
    for id in ["a", "b"] {
        // A saga mid-flight can always fail out, because failing consumes no
        // funds.
        let s = stranded(
            g,
            &[a.flat(id, SAGA_COMPLETED), a.flat(id, SAGA_ROLLED_BACK)],
        );
        assert!(s.is_empty(), "saga {id} stranded in {} states", s.len());
    }

    for st in g.states_list() {
        assert!(
            get(&st.marking, &pool) >= 0,
            "pool went negative in {:?}",
            st.marking
        );
    }
}

/// `TestCompensationStrandsWhenSomeoneElseSpendsTheCredit`
/// (metasubnet_test.go:643): the limit of the model, pinned. A compensation
/// needs the credited account to still hold the step's moves; if another
/// subnet may spend that credit, a rollback can be owed funds that are gone.
#[test]
fn compensation_strands_when_someone_else_spends_the_credit() {
    let build = |competitor: bool| {
        let s = SagaBuilder::new("s")
            .account("src", 1)
            .shared_account("dst")
            .step("send")
            .move_tokens("src", "dst", 1)
            .done()
            .step("later")
            .done()
            .must_build();
        let mut b = Bundle::new("x");
        b.add_subnet(s.to_meta_subnet());
        let mut other = Model {
            name: "other".into(),
            places: vec![exported_token("dst", 0)],
            ..Default::default()
        };
        if competitor {
            other.places.push(Place {
                id: "gone".into(),
                kind: Some(StateKind::Token),
                ..Default::default()
            });
            other.transitions = vec![Transition {
                id: "spend".into(),
                ..Default::default()
            }];
            other.arcs = vec![
                Arc {
                    from: "dst".into(),
                    to: "spend".into(),
                    weight: 1,
                    ..Default::default()
                },
                Arc {
                    from: "spend".into(),
                    to: "gone".into(),
                    weight: 1,
                    ..Default::default()
                },
            ];
        }
        b.add_subnet(Subnet {
            id: "other".into(),
            net_type: NetType::Resource,
            model: other,
            ..Default::default()
        });
        b.add_link(token_link("s", &account_port("dst"), "other", "dst"));
        analyse(&b)
    };

    let quiet = build(false);
    let space = quiet.space();
    let n = stranded(
        &space.graph,
        &[
            quiet.flat("s", SAGA_COMPLETED),
            quiet.flat("s", SAGA_ROLLED_BACK),
        ],
    )
    .len();
    assert_eq!(n, 0, "with nobody spending dst");

    let noisy = build(true);
    let space = noisy.space();
    let s = stranded(
        &space.graph,
        &[
            noisy.flat("s", SAGA_COMPLETED),
            noisy.flat("s", SAGA_ROLLED_BACK),
        ],
    );
    assert!(
        !s.is_empty(),
        "a competitor spending the credit should strand the compensation"
    );
    // Every stranded marking is one where send has run and its credit is
    // gone, so undo_send can never fire.
    for st in s {
        assert!(
            get(&st.marking, &noisy.flat("s", &done_place("send"))) == 1
                && get(&st.marking, "other/gone") == 1,
            "stranded for a reason other than the spent credit: {:?}",
            st.marking
        );
    }
}

// ---------------------------------------------------------------------------
// Not in go-pflow: the saga composed with this crate's other constructors.
// ---------------------------------------------------------------------------

/// A saga whose `do:reserve` is fused (EventLink) with a resource pool's
/// `acquire` and whose `undo:reserve` is fused with its `release`, and whose
/// `commit` is fused with a queue's `enqueue`. Flattening must keep each
/// part's laws: the bundle validates, the pool's conservation and the
/// saga's single control token both survive as P-invariants, both outcomes
/// stay reachable, a rollback returns the seat, and nothing strands.
#[test]
fn saga_composes_with_queue_and_resource_pool() {
    let saga = SagaBuilder::new("pay")
        .account("funds", 3)
        .account("held", 0)
        .step("reserve")
        .move_tokens("funds", "held", 1)
        .done()
        .step("charge")
        .done()
        .must_build();
    let queue = new_queue(QueueSpec {
        id: "outbox".into(),
        capacity: 2,
        ..Default::default()
    })
    .unwrap();
    let pool = new_resource_pool("seats", 1);

    let event = |from: (&str, &str), to: (&str, &str)| Link {
        kind: LinkKind::Event,
        from: Endpoint {
            subnet: from.0.into(),
            port: from.1.into(),
            ..Default::default()
        },
        to: Endpoint {
            subnet: to.0.into(),
            port: to.1.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = Bundle::new("composed");
    b.add_subnet(saga.to_meta_subnet());
    b.add_subnet(queue);
    b.add_subnet(pool);
    b.add_link(event(("pay", "do:reserve"), ("seats", "acquire")));
    b.add_link(event(("pay", "undo:reserve"), ("seats", "release")));
    b.add_link(event(("pay", "commit"), ("outbox", "enqueue")));

    let a = analyse(&b);
    let control: Vec<String> = saga
        .control_places()
        .iter()
        .map(|p| a.flat("pay", p))
        .collect();
    assert!(
        has_unit_invariant(&a.model, &control),
        "the saga's control token did not survive fusion"
    );
    let seats = [a.flat("seats", "available"), a.flat("seats", "in_use")];
    assert!(
        has_unit_invariant(&a.model, &seats),
        "the pool's conservation did not survive fusion"
    );

    let space = a.space();
    let g = &space.graph;
    let reach = |p: &str| g.states_list().iter().any(|st| get(&st.marking, p) == 1);
    assert!(
        reach(&a.flat("pay", SAGA_COMPLETED)),
        "completed is unreachable once composed"
    );
    assert!(
        reach(&a.flat("pay", SAGA_ROLLED_BACK)),
        "rolled_back is unreachable once composed"
    );
    // commit and enqueue are one transition: it consumes a queue slot and
    // produces an item. (The queue's own dequeue may drain the item later, so
    // the item is not asserted at every completed marking.)
    let commit = &a.map.transition["pay"]["commit"];
    assert_eq!(
        commit, &a.map.transition["outbox"]["enqueue"],
        "commit and enqueue were not fused"
    );
    let has_arc = |from: &str, to: &str| {
        a.model
            .arcs
            .iter()
            .any(|arc| arc.from == from && arc.to == to)
    };
    assert!(
        has_arc(&a.flat("outbox", "slots"), commit) && has_arc(commit, &a.flat("outbox", "items"))
    );
    assert!(
        g.states_list()
            .iter()
            .any(|st| get(&st.marking, &a.flat("pay", SAGA_COMPLETED)) == 1
                && get(&st.marking, &a.flat("outbox", "items")) == 1),
        "no completed marking holds the enqueued item"
    );
    for st in g.states_list() {
        if get(&st.marking, &a.flat("pay", SAGA_COMPLETED)) == 1 {
            assert_eq!(
                get(&st.marking, &a.flat("seats", "in_use")),
                1,
                "{:?}",
                st.marking
            );
        }
        if get(&st.marking, &a.flat("pay", SAGA_ROLLED_BACK)) == 1 {
            assert_eq!(
                get(&st.marking, &a.flat("seats", "available")),
                1,
                "rollback leaked a seat: {:?}",
                st.marking
            );
        }
    }
    let s = stranded(
        g,
        &[
            a.flat("pay", SAGA_COMPLETED),
            a.flat("pay", SAGA_ROLLED_BACK),
        ],
    );
    assert!(s.is_empty(), "{} stranded states once composed", s.len());
}
