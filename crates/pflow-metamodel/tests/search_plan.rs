//! Search plans: a legal plan finds the default search's values, an illegal
//! one is refused by the name of the piece at fault.
mod common;

use pflow_metamodel::SearchPlan;

fn plan(json: &str) -> SearchPlan {
    SearchPlan::from_json(json).unwrap()
}

/// The values the default search finds on `name`'s positions.
fn defaults(name: &str) -> (pflow_metamodel::game::Game, Vec<Vec<i64>>, Vec<f64>) {
    let (g, m0) = common::game(name);
    let mut all = common::positions(name, &g, &m0);
    all.truncate(400);
    let want = common::fresh(&g, &all).iter().map(|x| x.0).collect();
    (g, all, want)
}

fn values(p: &SearchPlan, g: &pflow_metamodel::game::Game, all: &[Vec<i64>]) -> Vec<f64> {
    all.iter()
        .map(|mk| p.solver(g).unwrap().value(mk).unwrap().unwrap())
        .collect()
}

#[test]
fn legal_plans_find_the_default_values() {
    let plans = [
        "{}",
        r#"{"mode": "exact"}"#,
        r#"{"mode": "play", "order": ["history", "tt_best"]}"#,
        r#"{"order": ["tt_best", "immediate_win", "threats", "centrality", "history"]}"#,
        r#"{"order": []}"#,
        r#"{"drivers": []}"#,
        r#"{"drivers": ["null_window", {"budget": {"nodes": 3000000}}], "memo": {"entries": 50}}"#,
        r#"{"memo": {"entries": 1000}, "order": ["centrality"]}"#,
    ];
    for name in ["tic-tac-toe", "connect-3"] {
        let (g, all, want) = defaults(name);
        for json in plans {
            assert_eq!(values(&plan(json), &g, &all), want, "{name} by {json}");
        }
    }
}

#[test]
fn a_budget_driver_gives_a_value_up() {
    let (g, m0) = common::game("connect-3");
    let p = plan(r#"{"drivers": ["null_window", {"budget": {"nodes": 3}}]}"#);
    assert_eq!(p.solver(&g).unwrap().value(&m0).unwrap(), None);
}

#[test]
fn a_memo_bounds_the_table_not_the_value() {
    let (g, all, want) = defaults("connect-3");
    let p = plan(r#"{"memo": {"entries": 1}}"#);
    assert_eq!(values(&p, &g, &all), want);
}

fn refused(json: &str) -> String {
    let (g, _) = common::game("connect-3");
    SearchPlan::from_json(json)
        .and_then(|p| p.solver(&g).map(|_| ()))
        .expect_err(json)
        .to_string()
}

#[test]
fn unknown_pieces_are_refused_by_name() {
    for json in [
        r#"{"order": ["tt_best", "zigzag"]}"#,
        r#"{"drivers": ["zigzag"]}"#,
        r#"{"drivers": [{"zigzag": {}}]}"#,
        r#"{"filters": ["zigzag"]}"#,
        r#"{"heuristics": ["zigzag"], "mode": "play"}"#,
    ] {
        let e = refused(json);
        assert!(e.contains("zigzag") && e.contains("unknown"), "{json}: {e}");
    }
}

#[test]
fn pieces_in_the_wrong_place_are_refused() {
    let e = refused(r#"{"order": ["null_window"]}"#);
    assert!(e.contains("null_window") && e.contains("driver"), "{e}");
    let e = refused(r#"{"drivers": ["history"]}"#);
    assert!(e.contains("history") && e.contains("order piece"), "{e}");
    let e = refused(r#"{"order": ["history", "history"]}"#);
    assert!(e.contains("history") && e.contains("twice"), "{e}");
    let e = refused(r#"{"drivers": ["budget"]}"#);
    assert!(e.contains("budget"), "{e}");
    let e = refused(r#"{"drivers": [{"budget": {"nodes": 0}}]}"#);
    assert!(e.contains("budget"), "{e}");
    let e = refused(r#"{"memo": {"entries": 0}}"#);
    assert!(e.contains("memo"), "{e}");
    let e = refused(r#"{"mode": "fast"}"#);
    assert!(e.contains("fast"), "{e}");
    let e = refused(r#"{"ordr": []}"#);
    assert!(e.contains("ordr"), "{e}");
}

#[test]
fn exact_mode_refuses_pieces_that_can_change_the_value() {
    let e = refused(r#"{"filters": ["beam"]}"#);
    assert!(e.contains("beam") && e.contains("exact"), "{e}");
    let e = refused(r#"{"mode": "exact", "heuristics": ["static_eval"]}"#);
    assert!(e.contains("static_eval") && e.contains("exact"), "{e}");
    // Play mode lets them in the plan, but none is built.
    let e = refused(r#"{"mode": "play", "filters": ["beam"]}"#);
    assert!(e.contains("beam") && e.contains("not built"), "{e}");
}

#[test]
fn goal_orders_are_refused_on_a_net_without_goals() {
    let g = common::goalless();
    for name in ["centrality", "threats"] {
        let p = plan(&format!(r#"{{"order": ["tt_best", "{name}"]}}"#));
        p.check().unwrap();
        let e = p.solver(&g).err().expect("refused").to_string();
        assert!(e.contains(name) && e.contains("goals"), "{e}");
    }
    plan("{}").solver(&g).unwrap();
}
