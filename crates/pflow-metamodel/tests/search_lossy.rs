//! `lmr`, a piece that can change a value. Their gate is the
//! roadmap's: play mode only (exact mode refuses it), opt-in (no default
//! plan names it), and honest about what they did: a search that cut a
//! move says so (`Solver::lossy`), and one that did not found the exact
//! value, even through a table earlier cuts passed through. Their effect on
//! play is reported by pflow-solver's `search_lossy` example.
mod common;

use pflow_metamodel::SearchPlan;

fn plan(json: &str) -> SearchPlan {
    SearchPlan::from_json(json).unwrap()
}

#[test]
fn exact_mode_refuses_it_and_play_mode_admits_it() {
    let refused = |json: &str| plan(json).check().unwrap_err().to_string();
    for json in [
        r#"{"heuristics": ["lmr"]}"#,
        r#"{"mode": "exact", "heuristics": ["lmr"]}"#,
    ] {
        let e = refused(json);
        assert!(e.contains("exact mode"), "{json}: {e}");
    }
    plan(r#"{"mode": "play", "heuristics": ["lmr"]}"#)
        .check()
        .unwrap();
    // The reserved ones stay unbuilt in either mode.
    let e = refused(r#"{"mode": "play", "heuristics": ["static_eval"]}"#);
    assert!(e.contains("not built"), "{e}");
    // Tuning needs its piece and sense.
    for json in [
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
