//! The composition laws of the search pieces, one test per law (named in
//! docs/search-composition-laws.md, which says which hold and why the others
//! do not). Laws that need the crate's private types are unit tests: O5 in
//! game.rs, B1 and B2 in memo.rs.
//!
//! Positions are tic-tac-toe's (the first 400 reachable) and the Connect-3
//! sample. Node counts are compared between fresh solvers only: a solver's
//! history carries over from one call to the next.
mod common;

use pflow_metamodel::game::{Choice, Game, Solver};
use pflow_metamodel::order::{Chain, Piece};
use pflow_metamodel::{Estimate, Filter, Result, SearchPlan};

/// The two nets and their positions.
fn nets() -> Vec<(&'static str, Game, Vec<Vec<i64>>)> {
    ["tic-tac-toe", "connect-3"]
        .into_iter()
        .map(|name| {
            let (g, m0) = common::game(name);
            let mut all = common::positions(name, &g, &m0);
            all.truncate(400);
            (name, g, all)
        })
        .collect()
}

/// (value, nodes) at each position, each by a fresh solver from `make`.
fn by<'a>(all: &[Vec<i64>], make: impl Fn() -> Solver<'a>) -> Vec<(Option<f64>, usize)> {
    all.iter()
        .map(|mk| {
            let mut s = make();
            let v = s.value(mk).unwrap();
            (v, s.nodes())
        })
        .collect()
}

fn values(runs: &[(Option<f64>, usize)]) -> Vec<Option<f64>> {
    runs.iter().map(|x| x.0).collect()
}

fn nodes(runs: &[(Option<f64>, usize)]) -> usize {
    runs.iter().map(|x| x.1).sum()
}

fn plan(json: &str) -> SearchPlan {
    SearchPlan::from_json(json).unwrap()
}

/// (value, nodes) by `json`'s solver, fresh per position, given the noise
/// estimate when the plan names one.
fn by_plan(g: &Game, all: &[Vec<i64>], json: &str) -> Vec<(Option<f64>, usize)> {
    let p = plan(json);
    by(all, || match p.estimate().unwrap() {
        Some(_) => p.solver_with(g, Some(Box::new(Noise))).unwrap(),
        None => p.solver(g).unwrap(),
    })
}

fn chain(names: &[&str]) -> Chain {
    let pieces: Vec<Piece> = names.iter().map(|n| Piece::parse(n).unwrap()).collect();
    Chain::new(&pieces).unwrap()
}

fn with_chain<'a>(g: &'a Game, c: &Chain, filters: &[Filter]) -> Solver<'a> {
    Solver::with_chain(g, common::BUDGET, c.clone())
        .unwrap()
        .with_filters(filters)
}

/// An estimate called `ode` that reads noise off the marking.
struct Noise;

impl Estimate for Noise {
    fn name(&self) -> &'static str {
        "ode"
    }

    fn estimate(&mut self, _: &Game, mk: &[i64]) -> Result<f64> {
        let h = mk
            .iter()
            .fold(7u64, |h, &v| h.wrapping_mul(31).wrapping_add(v as u64));
        Ok((h % 201) as f64 / 100.0 - 1.0)
    }
}

/// O1: chains under `then` are a monoid: associative, with unit
/// `Chain::new(&[])`, both groupings refused together past eight pieces.
/// `Chain::default()` is not the unit.
#[test]
fn o1_chains_are_a_monoid_under_then() {
    let n = Piece::NAMES;
    let mut chains = vec![chain(&[])];
    chains.extend(n.map(|a| chain(&[a])));
    chains.extend([
        chain(&[n[0], n[1]]),
        chain(&[n[3], n[2], n[4]]),
        chain(&[n[2]; 4]),
        chain(&[n[1]; 5]),
        chain(&[n[4]; 8]),
    ]);
    let unit = Chain::new(&[]).unwrap();
    let (mut refused, mut joined) = (0, 0);
    for a in &chains {
        assert_eq!(&unit.then(a).unwrap(), a);
        assert_eq!(&a.then(&unit).unwrap(), a);
        if let Ok(d) = Chain::default().then(a) {
            assert_ne!(&d, a);
        }
        for b in &chains {
            for c in &chains {
                let left = a.then(b).and_then(|ab| ab.then(c));
                let right = b.then(c).and_then(|bc| a.then(&bc));
                let len = a.pieces().len() + b.pieces().len() + c.pieces().len();
                match (left, right) {
                    (Ok(l), Ok(r)) => {
                        assert_eq!(l, r);
                        assert_eq!(l.pieces().len(), len);
                        joined += 1;
                    }
                    (Err(_), Err(_)) => {
                        assert!(len > 8);
                        refused += 1;
                    }
                    (l, r) => panic!("one grouping refused: {l:?} {r:?}"),
                }
            }
        }
    }
    assert!(joined > 0 && refused > 0);
}

/// O2: leaving `order` out is the default chain, not the unit: the same
/// values, other node counts.
#[test]
fn o2_the_default_chain_is_not_the_unit() {
    for (name, g, all) in nets() {
        let default = by_plan(&g, &all, "{}");
        let unit = by_plan(&g, &all, r#"{"order": []}"#);
        assert_eq!(values(&default), values(&unit), "{name}");
        assert_ne!(nodes(&default), nodes(&unit), "{name}");
        println!(
            "o2 {name}: default {} nodes, order [] {}",
            nodes(&default),
            nodes(&unit)
        );
    }
}

/// O3: a piece listed again changes nothing, next to itself or past
/// another: the same values and nodes (history counted twice keeps its
/// order). A plan refuses the repeat; `Chain::new` takes it.
#[test]
fn o3_a_repeated_piece_changes_nothing() {
    for (name, g, all) in nets() {
        for a in Piece::NAMES {
            let once = by(&all, || with_chain(&g, &chain(&[a]), &Filter::DEFAULT));
            let twice = by(&all, || with_chain(&g, &chain(&[a, a]), &Filter::DEFAULT));
            assert_eq!(once, twice, "{name} [{a}, {a}]");
            for b in Piece::NAMES.into_iter().filter(|b| *b != a) {
                let ab = by(&all, || with_chain(&g, &chain(&[a, b]), &Filter::DEFAULT));
                let aba = by(&all, || {
                    with_chain(&g, &chain(&[a, b, a]), &Filter::DEFAULT)
                });
                assert_eq!(ab, aba, "{name} [{a}, {b}, {a}]");
            }
        }
    }
}

/// O4: a chain's order matters: swapped, the same values, and node counts
/// that differ for at least one of the pairs.
#[test]
fn o4_chains_do_not_commute() {
    let mut differ = 0;
    for (name, g, all) in nets() {
        for (a, b) in [
            ("tt_best", "history"),
            ("centrality", "history"),
            ("immediate_win", "centrality"),
        ] {
            let ab = by(&all, || with_chain(&g, &chain(&[a, b]), &Filter::DEFAULT));
            let ba = by(&all, || with_chain(&g, &chain(&[b, a]), &Filter::DEFAULT));
            assert_eq!(values(&ab), values(&ba), "{name} {a} {b}");
            println!(
                "o4 {name}: [{a}, {b}] {} nodes, [{b}, {a}] {}",
                nodes(&ab),
                nodes(&ba)
            );
            if ab != ba {
                differ += 1;
            }
        }
    }
    assert!(differ > 0);
}

/// O6: on games paying ±1, `must_win` leaves `immediate_win` nothing to
/// order (a winning move settles the node before any key is compared):
/// with it, the piece changes no node count; without it, only the values
/// are the same.
#[test]
fn o6_must_win_absorbs_immediate_win() {
    for (name, g, all) in nets() {
        let with = chain(&["tt_best", "immediate_win", "history"]);
        let without = chain(&["tt_best", "history"]);
        let a = by(&all, || with_chain(&g, &with, &[Filter::MustWin]));
        let b = by(&all, || with_chain(&g, &without, &[Filter::MustWin]));
        assert_eq!(a, b, "{name}");
        let a = by(&all, || with_chain(&g, &with, &[]));
        let b = by(&all, || with_chain(&g, &without, &[]));
        assert_eq!(values(&a), values(&b), "{name}");
        println!(
            "o6 {name}, no filters: with immediate_win {} nodes, without {}",
            nodes(&a),
            nodes(&b)
        );
    }
}

/// F1, F2: filters are flags. Their order is ignored, in a solver and in a
/// plan, and one given twice to a solver is one (a plan refuses it).
#[test]
fn f1_f2_filters_are_a_set() {
    use Filter::{MustWin, NonLosing};
    let c = Chain::default();
    for (name, g, all) in nets() {
        let mn = by(&all, || with_chain(&g, &c, &[MustWin, NonLosing]));
        let nm = by(&all, || with_chain(&g, &c, &[NonLosing, MustWin]));
        assert_eq!(mn, nm, "{name}");
        let pm = by_plan(&g, &all, r#"{"filters": ["must_win", "non_losing"]}"#);
        let pn = by_plan(&g, &all, r#"{"filters": ["non_losing", "must_win"]}"#);
        assert_eq!(pm, pn, "{name}");
        assert_eq!(pm, mn, "{name}");
        let once = by(&all, || with_chain(&g, &c, &[MustWin]));
        let twice = by(&all, || with_chain(&g, &c, &[MustWin, MustWin]));
        assert_eq!(once, twice, "{name}");
    }
}

/// F3: no filters is the unit, and not the default (`must_win`): the same
/// values, fewer nodes with the default.
#[test]
fn f3_no_filters_is_the_unit_not_the_default() {
    for (name, g, all) in nets() {
        let default = by_plan(&g, &all, "{}");
        let none = by_plan(&g, &all, r#"{"filters": []}"#);
        assert_eq!(values(&default), values(&none), "{name}");
        assert!(nodes(&default) < nodes(&none), "{name}");
        println!(
            "f3 {name}: default {} nodes, filters [] {} ({:+.1}%)",
            nodes(&default),
            nodes(&none),
            100.0 * (nodes(&default) as f64 / nodes(&none) as f64 - 1.0)
        );
    }
}

/// F4: every set of filters, in either order, finds the default values.
#[test]
fn f4_every_set_of_filters_keeps_the_values() {
    use Filter::{MustWin, NonLosing};
    let c = Chain::default();
    for (name, g, all) in nets() {
        let want = values(&by_plan(&g, &all, "{}"));
        for set in [
            vec![],
            vec![MustWin],
            vec![NonLosing],
            vec![MustWin, NonLosing],
            vec![NonLosing, MustWin],
        ] {
            let got = by(&all, || with_chain(&g, &c, &set));
            assert_eq!(values(&got), want, "{name} {set:?}");
        }
    }
}

/// F5: the moves `non_losing` drops at the root are lost: after each, a
/// fresh search finds the mover's worst bound. It keeps only moves there
/// are.
#[test]
fn f5_non_losing_drops_only_lost_moves() {
    let mut dropped = 0;
    for (name, g, all) in nets() {
        for mk in &all {
            let moves = g.moves(mk);
            if moves.is_empty() {
                continue;
            }
            let kept = Solver::new(&g, common::BUDGET)
                .with_root_non_losing(true)
                .root_order(mk)
                .unwrap();
            assert!(kept.iter().all(|t| moves.contains(t)), "{name}");
            let s = g.mover(&moves).unwrap();
            let worst = if s > 0.0 { g.bounds.0 } else { g.bounds.1 };
            for &t in moves.iter().filter(|t| !kept.contains(t)) {
                let next = g.play(mk, t).unwrap();
                let v = Solver::new(&g, common::BUDGET).value(&next).unwrap();
                assert_eq!(v, Some(worst), "{name}: move {t} at {mk:?}");
                dropped += 1;
            }
        }
    }
    println!("f5: {dropped} moves dropped, each lost");
    assert!(dropped > 0);
}

/// F6: `non_losing` at the root adds nothing to `non_losing` as a filter:
/// the same predicate, already run at the root.
#[test]
fn f6_root_non_losing_is_absorbed_by_the_filter() {
    let c = Chain::default();
    for (name, g, all) in nets() {
        let filter = by(&all, || with_chain(&g, &c, &[Filter::NonLosing]));
        let both = by(&all, || {
            with_chain(&g, &c, &[Filter::NonLosing]).with_root_non_losing(true)
        });
        assert_eq!(filter, both, "{name}");
    }
}

/// D1: the order drivers are listed in is ignored: budget(null_window(core))
/// either way, within the budget and past it.
#[test]
fn d1_driver_order_is_ignored() {
    for (name, g, all) in nets() {
        for n in [3, 3_000_000] {
            let a = by_plan(
                &g,
                &all,
                &format!(r#"{{"drivers": ["null_window", {{"budget": {{"nodes": {n}}}}}]}}"#),
            );
            let b = by_plan(
                &g,
                &all,
                &format!(r#"{{"drivers": [{{"budget": {{"nodes": {n}}}}}, "null_window"]}}"#),
            );
            assert_eq!(a, b, "{name} {n}");
        }
    }
}

/// D2: a node budget truncates: a search that needs n nodes finds the same
/// value in n under any budget of at least n, and gives up at node N + 1
/// under a budget N below it. With null_window on and off.
#[test]
fn d2_a_budget_truncates() {
    for (name, g, all) in nets() {
        for null_window in [true, false] {
            let solver = |n: usize| {
                Solver::with_chain(&g, n, Chain::default())
                    .unwrap()
                    .with_null_window(null_window)
            };
            for mk in &all {
                let mut s = solver(usize::MAX);
                let v = s.value(mk).unwrap();
                let need = s.nodes();
                for n in [need.saturating_sub(1), need, need + 1, need / 2] {
                    let mut s = solver(n);
                    let got = s.value(mk).unwrap();
                    if n >= need {
                        assert_eq!((got, s.nodes()), (v, need), "{name} {mk:?} budget {n}");
                    } else {
                        assert_eq!((got, s.nodes()), (None, n + 1), "{name} {mk:?} budget {n}");
                    }
                }
            }
        }
    }
}

/// D3: two budgets are refused, as is any driver listed twice: the plan
/// does not compose a budget with a budget.
#[test]
fn d3_two_budgets_are_refused() {
    for json in [
        r#"{"drivers": [{"budget": {"nodes": 3}}, {"budget": {"nodes": 3000000}}]}"#,
        r#"{"drivers": [{"budget": {"nodes": 5}}, {"budget": {"nodes": 5}}]}"#,
        r#"{"drivers": [{"budget": {"ms": 5}}, "null_window", {"budget": {"nodes": 5}}]}"#,
    ] {
        let e = plan(json).check().expect_err(json).to_string();
        assert!(e.contains("\"budget\" is listed twice"), "{e}");
    }
}

/// D4: no drivers is the identity (one search over the range): the same
/// values as null_window, other node counts. null_window twice is refused.
#[test]
fn d4_no_drivers_is_the_identity() {
    for (name, g, all) in nets() {
        let one = by_plan(&g, &all, r#"{"drivers": []}"#);
        let nw = by_plan(&g, &all, r#"{"drivers": ["null_window"]}"#);
        assert_eq!(values(&one), values(&nw), "{name}");
        assert_ne!(nodes(&one), nodes(&nw), "{name}");
        println!(
            "d4 {name}: drivers [] {} nodes, [null_window] {}",
            nodes(&one),
            nodes(&nw)
        );
    }
    let e = plan(r#"{"drivers": ["null_window", "null_window"]}"#)
        .check()
        .unwrap_err()
        .to_string();
    assert!(e.contains("listed twice"), "{e}");
}

/// D5: with fewer than three rewards there is no gap to step through, and
/// null_window is the identity.
#[test]
fn d5_null_window_is_the_identity_below_three_rewards() {
    let g = common::goalless();
    let mut m0 = vec![1, 0, 0];
    g.settle(&mut m0).unwrap();
    let all = common::reachable(&g, &m0);
    assert!(all.len() > 1);
    let one = by_plan(&g, &all, r#"{"drivers": []}"#);
    let nw = by_plan(&g, &all, r#"{"drivers": ["null_window"]}"#);
    assert_eq!(one, nw);
}

/// R1: the root's pieces are a set: listed either way, the same values,
/// nodes and root order.
#[test]
fn r1_root_pieces_are_a_set() {
    for (name, g, all) in nets() {
        for drivers in ["", r#", "drivers": ["null_window", "aspiration"]"#] {
            let a = format!(r#"{{"root": {{"order": ["ode", "non_losing"]}}{drivers}}}"#);
            let b = format!(r#"{{"root": {{"order": ["non_losing", "ode"]}}{drivers}}}"#);
            assert_eq!(by_plan(&g, &all, &a), by_plan(&g, &all, &b), "{name} {a}");
            let (pa, pb) = (plan(&a), plan(&b));
            for mk in &all {
                let order = |p: &SearchPlan| {
                    p.solver_with(&g, Some(Box::new(Noise)))
                        .unwrap()
                        .root_order(mk)
                        .unwrap()
                };
                assert_eq!(order(&pa), order(&pb), "{name} {mk:?}");
            }
        }
    }
}

/// P1: plans with the same normal form build the same solver (the same
/// values and nodes) and draw the same net; the normal form is its own.
#[test]
fn p1_plans_with_one_normal_form_are_one_plan() {
    let groups = [
        vec![
            "{}",
            r#"{"mode": "exact", "order": ["tt_best", "immediate_win", "history"],
                "drivers": ["null_window"], "filters": ["must_win"], "root": {"order": []},
                "memo": {"entries": 8388608}}"#,
        ],
        vec![
            r#"{"filters": ["must_win", "non_losing"],
                "drivers": ["null_window", {"budget": {"nodes": 3000000}}]}"#,
            r#"{"drivers": [{"budget": {"nodes": 3000000}}, "null_window"],
                "filters": ["non_losing", "must_win"], "memo": {"entries": 8388608}}"#,
        ],
        vec![
            r#"{"root": {"order": ["ode", "non_losing"]}, "drivers": ["null_window", "aspiration"],
                "order": ["centrality", "tt_best"]}"#,
            r#"{"root": {"order": ["non_losing", "ode"]}, "drivers": ["aspiration", "null_window"],
                "order": ["centrality", "tt_best"], "filters": ["must_win"]}"#,
        ],
    ];
    for (name, g, all) in nets() {
        for group in &groups {
            let first = plan(group[0]).normalized().unwrap();
            assert_eq!(first.normalized().unwrap(), first, "idempotent");
            let runs = by_plan(&g, &all, group[0]);
            for json in &group[1..] {
                let p = plan(json);
                assert_eq!(p.normalized().unwrap(), first, "{json}");
                assert_eq!(p.net().unwrap(), plan(group[0]).net().unwrap(), "{json}");
                assert_eq!(by_plan(&g, &all, json), runs, "{name} {json}");
            }
        }
    }
}

/// C1: `choose` is deterministic: two fresh solvers of one plan (no time
/// budget) choose the same at every position.
#[test]
fn c1_choose_is_deterministic() {
    for (name, g, all) in nets() {
        for json in [
            "{}",
            r#"{"order": ["centrality", "history"], "filters": ["must_win", "non_losing"]}"#,
            r#"{"root": {"order": ["ode", "non_losing"]}, "drivers": ["null_window", "aspiration"]}"#,
            r#"{"mode": "play", "drivers": ["null_window", {"budget": {"nodes": 20}}],
                "root": {"order": ["ode"]}}"#,
        ] {
            let p = plan(json);
            let choose = |mk: &[i64]| -> (Option<Choice>, usize) {
                let mut s = match p.estimate().unwrap() {
                    Some(_) => p.solver_with(&g, Some(Box::new(Noise))).unwrap(),
                    None => p.solver(&g).unwrap(),
                };
                let c = s.choose(mk).unwrap();
                (c, s.nodes())
            };
            for mk in &all {
                assert_eq!(choose(mk), choose(mk), "{name} {json} at {mk:?}");
            }
        }
    }
}
