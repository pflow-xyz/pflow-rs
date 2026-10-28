//! `lmr` and `futility`: pieces that can change a value. Their gate is the
//! roadmap's: play mode only (exact mode refuses it), opt-in (no default
//! plan names it), and honest about what they did: a search that cut a
//! move says so (`Solver::lossy`), and one that did not found the exact
//! value, even through a table earlier cuts passed through. Their effect on
//! play is reported by pflow-solver's `search_lossy` example.
mod common;

use pflow_metamodel::game::{Game, Solver};
use pflow_metamodel::{Estimate, Result, SearchPlan};
use std::collections::HashMap;

fn plan(json: &str) -> SearchPlan {
    SearchPlan::from_json(json).unwrap()
}

/// Exact values as the estimate: the best a futility test can have.
struct Exact(HashMap<Vec<i64>, f64>);

impl Estimate for Exact {
    fn name(&self) -> &'static str {
        "ode"
    }
    fn estimate(&mut self, g: &Game, mk: &[i64]) -> Result<f64> {
        if let Some(v) = self.0.get(mk) {
            return Ok(*v);
        }
        let v = Solver::new(g, usize::MAX).value(mk)?.unwrap();
        self.0.insert(mk.to_vec(), v);
        Ok(v)
    }
}

/// A wrong estimate: always the best bound for the first side.
struct Wrong;

impl Estimate for Wrong {
    fn name(&self) -> &'static str {
        "ode"
    }
    fn estimate(&mut self, _: &Game, _: &[i64]) -> Result<f64> {
        Ok(1.0)
    }
}

#[test]
fn exact_mode_refuses_it_and_play_mode_admits_it() {
    let refused = |json: &str| plan(json).check().unwrap_err().to_string();
    for json in [
        r#"{"heuristics": ["lmr"]}"#,
        r#"{"mode": "exact", "heuristics": ["lmr"]}"#,
        r#"{"filters": ["futility"], "root": {"order": ["ode"]}}"#,
    ] {
        let e = refused(json);
        assert!(e.contains("exact mode"), "{json}: {e}");
    }
    plan(r#"{"mode": "play", "heuristics": ["lmr"]}"#)
        .check()
        .unwrap();
    plan(r#"{"mode": "play", "filters": ["futility"], "root": {"order": ["ode"]}}"#)
        .check()
        .unwrap();
    // The reserved ones stay unbuilt in either mode.
    let e = refused(r#"{"mode": "play", "heuristics": ["static_eval"]}"#);
    assert!(e.contains("not built"), "{e}");
    // futility asks the estimate; tuning needs its piece and sense.
    for json in [
        r#"{"mode": "play", "filters": ["futility"]}"#,
        r#"{"mode": "play", "heuristics": ["lmr"], "tuning": {"futility": {"margin": 0.1}}}"#,
        r#"{"mode": "play", "filters": ["futility"], "root": {"order": ["ode"]},
            "tuning": {"futility": {"margin": -1}}}"#,
        r#"{"mode": "play", "tuning": {"lmr": {"after": 1, "nodes": 5}}}"#,
        r#"{"mode": "play", "heuristics": ["lmr"], "tuning": {"lmr": {"after": 0, "nodes": 5}}}"#,
        r#"{"mode": "play", "heuristics": ["lmr", "lmr"]}"#,
    ] {
        refused(json);
    }
}

#[test]
fn it_is_not_in_a_default_plan_and_the_golden_gate_is_untouched() {
    let n = plan("{}").normalized().unwrap();
    assert!(n.heuristics.is_empty() && n.tuning.is_none());
    assert!(!n.filters.unwrap().iter().any(|f| f == "futility"));
    // A plan that names them states them, tuned, and draws them.
    let p = plan(r#"{"mode": "play", "heuristics": ["lmr"]}"#);
    let n = p.normalized().unwrap();
    assert_eq!(n.tuning.unwrap().lmr.unwrap().after, 3);
    assert!(p.net().unwrap().contains("heuristic.lmr(after=3,nodes=64)"));
}

#[test]
fn a_search_that_cut_nothing_found_the_exact_value() {
    let (g, m0) = common::game("tic-tac-toe");
    let all = common::reachable(&g, &m0);
    let want: Vec<f64> = common::fresh(&g, &all).iter().map(|x| x.0).collect();
    for json in [
        r#"{"mode": "play", "heuristics": ["lmr"], "tuning": {"lmr": {"after": 1, "nodes": 3}}}"#,
        r#"{"mode": "play", "heuristics": ["lmr"], "tuning": {"lmr": {"after": 2, "nodes": 12}}}"#,
        r#"{"mode": "play", "heuristics": ["lmr"], "tuning": {"lmr": {"after": 1, "nodes": 1000000}}}"#,
    ] {
        let p = plan(json);
        // One solver for every position: its table holds what earlier cuts
        // passed through, and must hold nothing false.
        let mut s = p.solver(&g).unwrap();
        let (mut cut, mut wrong) = (0, 0);
        for (mk, want) in all.iter().zip(&want) {
            let v = s.value(mk).unwrap().unwrap();
            if s.lossy() {
                cut += 1;
                wrong += (v != *want) as usize;
            } else {
                assert_eq!(v, *want, "{json}: a search with no cut found a wrong value");
            }
        }
        println!("{json}: {cut} of {} searches cut, {wrong} wrong", all.len());
        if json.contains("1000000") {
            assert_eq!(cut, 0);
        } else {
            assert!(cut > 0, "{json}");
        }
    }
    // And it can change a value, which is why exact mode refuses it.
    let p = plan(
        r#"{"mode": "play", "heuristics": ["lmr"], "tuning": {"lmr": {"after": 1, "nodes": 3}}}"#,
    );
    let changed = all
        .iter()
        .zip(&want)
        .any(|(mk, want)| p.solver(&g).unwrap().value(mk).unwrap().unwrap() != *want);
    assert!(changed, "lmr with 3 nodes changed no value on tic-tac-toe");
}

#[test]
fn futility_with_an_exact_estimate_and_no_margin_changes_nothing_and_a_wrong_one_does() {
    let (g, m0) = common::game("tic-tac-toe");
    let all = common::reachable(&g, &m0);
    let want: Vec<f64> = common::fresh(&g, &all).iter().map(|x| x.0).collect();
    let json = |margin: f64| {
        format!(
            r#"{{"mode": "play", "filters": ["futility"], "root": {{"order": ["ode"]}},
                "tuning": {{"futility": {{"margin": {margin}}}}}}}"#
        )
    };
    let p = plan(&json(0.0));
    let mut cut = 0;
    for (mk, want) in all.iter().zip(&want) {
        let mut s = p
            .solver_with(&g, Some(Box::new(Exact(HashMap::new()))))
            .unwrap();
        assert_eq!(s.value(mk).unwrap().unwrap(), *want);
        cut += s.lossy() as usize;
    }
    assert!(cut > 0, "the exact estimate cut nothing");
    // A wrong estimate: values change, and every changed one says so.
    let (mut changed, mut said) = (0, 0);
    for (mk, want) in all.iter().zip(&want) {
        let mut s = p.solver_with(&g, Some(Box::new(Wrong))).unwrap();
        let v = s.value(mk).unwrap().unwrap();
        if v != *want {
            changed += 1;
            said += s.lossy() as usize;
        }
    }
    assert!(
        changed > 0 && changed == said,
        "{changed} changed, {said} said so"
    );
}

#[test]
fn choose_does_not_call_a_cut_search_exact() {
    let (g, m0) = common::game("tic-tac-toe");
    let p = plan(
        r#"{"mode": "play", "heuristics": ["lmr"], "tuning": {"lmr": {"after": 1, "nodes": 2}}}"#,
    );
    let mut any = false;
    for mk in common::reachable(&g, &m0) {
        if g.moves(&mk).is_empty() {
            continue;
        }
        let mut s = p.solver(&g).unwrap();
        let c = s.choose(&mk).unwrap().unwrap();
        if s.lossy() {
            assert!(!c.exact);
            any = true;
        }
    }
    assert!(any);
}
