//! Search plans: a legal plan finds the default search's values, an illegal
//! one is refused by the name of the piece at fault.
mod common;

use pflow_metamodel::game::Game;
use pflow_metamodel::{Estimate, Result, SearchPlan};

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
        r#"{"filters": ["must_win"]}"#,
        r#"{"filters": ["non_losing"], "mode": "exact"}"#,
        r#"{"filters": ["non_losing", "must_win"], "order": ["threats"], "memo": {"entries": 64}}"#,
        r#"{"root": {"order": ["non_losing"]}}"#,
        r#"{"root": {"order": []}, "drivers": []}"#,
        r#"{"drivers": ["null_window", {"budget": {"ms": 600000}}]}"#,
        r#"{"mode": "play", "drivers": [{"budget": {"nodes": 3000000, "ms": 600000}}]}"#,
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
fn play_mode_guesses_past_the_budget_and_exact_mode_does_not() {
    let (g, m0) = common::game("connect-3");
    let budget = r#""drivers": ["null_window", {"budget": {"nodes": 50}}]"#;
    let exact = plan(&format!("{{{budget}}}"));
    assert_eq!(exact.solver(&g).unwrap().choose(&m0).unwrap(), None);
    let play = plan(&format!(r#"{{"mode": "play", {budget}}}"#));
    let c = play.solver(&g).unwrap().choose(&m0).unwrap().unwrap();
    assert!(!c.exact && g.moves(&m0).contains(&c.t), "{c:?}");
    // With room, play mode's move is a proven one.
    let roomy = plan(r#"{"mode": "play"}"#);
    let mk = &common::positions("connect-3", &g, &m0)[20];
    let c = roomy.solver(&g).unwrap().choose(mk).unwrap().unwrap();
    assert!(c.exact, "{c:?}");
}

#[test]
fn a_memo_bounds_the_table_not_the_value() {
    let (g, all, want) = defaults("connect-3");
    let p = plan(r#"{"memo": {"entries": 1}}"#);
    assert_eq!(values(&p, &g, &all), want);
}

/// An estimate called `name` that reads noise off the marking.
struct Noise(&'static str);

impl Estimate for Noise {
    fn name(&self) -> &'static str {
        self.0
    }

    fn estimate(&mut self, _: &Game, mk: &[i64]) -> Result<f64> {
        let h = mk
            .iter()
            .fold(7u64, |h, &v| h.wrapping_mul(31).wrapping_add(v as u64));
        Ok((h % 201) as f64 / 100.0 - 1.0)
    }
}

#[test]
fn root_estimates_and_aspiration_find_the_default_values() {
    let plans = [
        r#"{"root": {"order": ["ode"]}}"#,
        r#"{"root": {"order": ["ode", "non_losing"]}, "drivers": ["null_window", "aspiration"]}"#,
        r#"{"root": {"order": ["non_losing", "ode"]}, "drivers": ["aspiration"]}"#,
    ];
    for name in ["tic-tac-toe", "connect-3"] {
        let (g, all, want) = defaults(name);
        for json in plans {
            let p = plan(json);
            assert_eq!(p.estimate().unwrap(), Some("ode"));
            let got: Vec<f64> = all
                .iter()
                .map(|mk| {
                    let mut s = p.solver_with(&g, Some(Box::new(Noise("ode")))).unwrap();
                    s.value(mk).unwrap().unwrap()
                })
                .collect();
            assert_eq!(got, want, "{name} by {json}");
        }
    }
}

#[test]
fn root_pieces_are_refused_by_name() {
    let e = refused(r#"{"root": {"order": ["ode"]}}"#);
    assert!(e.contains("ode") && e.contains("not built"), "{e}");
    let e = refused(r#"{"root": {"order": ["zigzag"]}}"#);
    assert!(
        e.contains("zigzag") && e.contains("unknown root piece"),
        "{e}"
    );
    let e = refused(r#"{"root": {"order": ["history"]}}"#);
    assert!(
        e.contains("history") && e.contains("not a root piece"),
        "{e}"
    );
    let e = refused(r#"{"root": {"order": ["must_win"]}}"#);
    assert!(
        e.contains("must_win") && e.contains("not a root piece"),
        "{e}"
    );
    let e = refused(r#"{"root": {"order": ["non_losing", "non_losing"]}}"#);
    assert!(e.contains("non_losing") && e.contains("twice"), "{e}");
    let e = refused(r#"{"root": {"ordr": []}}"#);
    assert!(e.contains("ordr"), "{e}");
    let e = refused(r#"{"drivers": ["aspiration"]}"#);
    assert!(e.contains("aspiration") && e.contains("estimate"), "{e}");
    let e = refused(r#"{"order": ["ode"]}"#);
    assert!(e.contains("ode") && e.contains("estimate"), "{e}");
    // An estimate by another name is not the one the plan names.
    let (g, _) = common::game("connect-3");
    let p = plan(r#"{"root": {"order": ["ode"]}}"#);
    let e = p
        .solver_with(&g, Some(Box::new(Noise("cache"))))
        .err()
        .unwrap();
    assert!(e.to_string().contains("cache"), "{e}");
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
    let e = refused(r#"{"drivers": [{"budget": {}}]}"#);
    assert!(e.contains("budget") && e.contains("ms"), "{e}");
    let e = refused(r#"{"drivers": [{"budget": {"ms": 0}}]}"#);
    assert!(e.contains("budget"), "{e}");
    let e = refused(r#"{"drivers": [{"budget": {"seconds": 1}}]}"#);
    assert!(e.contains("seconds"), "{e}");
    let e = refused(r#"{"filters": ["must_win", "must_win"]}"#);
    assert!(e.contains("must_win") && e.contains("twice"), "{e}");
    let e = refused(r#"{"order": ["non_losing"]}"#);
    assert!(e.contains("non_losing") && e.contains("filter"), "{e}");
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
    // Alongside a sound filter, the unsound one is still the one refused.
    let e = refused(r#"{"filters": ["must_win", "beam"]}"#);
    assert!(e.contains("beam") && e.contains("exact"), "{e}");
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

#[test]
fn drivers_listed_twice_are_refused() {
    for (json, name) in [
        (
            r#"{"drivers": ["null_window", "null_window"]}"#,
            "null_window",
        ),
        (
            r#"{"root": {"order": ["ode"]}, "drivers": ["aspiration", "null_window", "aspiration"]}"#,
            "aspiration",
        ),
        (
            r#"{"drivers": [{"budget": {"nodes": 3}}, {"budget": {"nodes": 3000000}}]}"#,
            "budget",
        ),
        // A later budget no longer erases an earlier time.
        (
            r#"{"drivers": [{"budget": {"ms": 50}}, {"budget": {"nodes": 5}}]}"#,
            "budget",
        ),
    ] {
        let e = plan(json).check().expect_err(json).to_string();
        assert!(e.contains(&format!("{name:?} is listed twice")), "{e}");
        let e = refused(json);
        assert!(e.contains("listed twice"), "{e}");
    }
}

/// The plans whose nets are committed in tests/golden (and drawn in
/// docs/search-composition-laws.md).
const DRAWN: [(&str, &str); 4] = [
    ("plan-default", "{}"),
    (
        "plan-connect4-exact",
        r#"{"mode":"exact","order":["tt_best","immediate_win","centrality","history"],"drivers":["null_window",{"budget":{"nodes":1000000}}],"memo":{"entries":500000}}"#,
    ),
    (
        "plan-root-ode",
        r#"{"root": {"order": ["ode", "non_losing"]}, "drivers": ["null_window", "aspiration"]}"#,
    ),
    (
        "plan-doc-example",
        r#"{"mode": "exact",
            "order": ["tt_best", "immediate_win", "centrality", "history"],
            "filters": ["must_win", "non_losing"],
            "root": {"order": ["ode", "non_losing"]},
            "drivers": ["null_window", "aspiration", {"budget": {"nodes": 1000000}}],
            "memo": {"entries": 500000}}"#,
    ),
];

fn net(json: &str) -> String {
    plan(json).net().unwrap()
}

#[test]
fn a_plan_draws_as_a_net_the_parser_reads() {
    let golden = format!("{}/tests/golden", env!("CARGO_MANIFEST_DIR"));
    for (file, json) in DRAWN {
        let text = net(json);
        let want = std::fs::read_to_string(format!("{golden}/{file}.json")).unwrap();
        assert_eq!(text, want, "{file}: regenerate with the plan_net example");
        let (m, _) = pflow_parser::model_from_json(text.as_bytes()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(m.places.len(), v["places"].as_object().unwrap().len());
        assert_eq!(
            m.transitions.len(),
            v["transitions"].as_object().unwrap().len()
        );
        assert_eq!(m.arcs.len(), v["arcs"].as_array().unwrap().len());
        // One way in, one way out: the composite is Position → Value⊥.
        let into = |p: &str| m.arcs.iter().filter(|a| a.to == p).count();
        let from = |p: &str| m.arcs.iter().filter(|a| a.from == p).count();
        for p in &m.places {
            let ends = (into(&p.id), from(&p.id));
            match p.id.as_str() {
                "Position" => assert_eq!(ends, (0, 1)),
                "Value⊥" => assert_eq!(ends, (1, 0)),
                _ => assert!(ends.0 == 1 && ends.1 >= 1, "{file}: {} {ends:?}", p.id),
            }
        }
    }
}

#[test]
fn a_net_draws_the_order_the_search_runs_not_the_order_listed() {
    // Sets listed in any order draw one net.
    let a = net(
        r#"{"filters": ["must_win", "non_losing"], "root": {"order": ["ode", "non_losing"]},
            "drivers": ["null_window", "aspiration", {"budget": {"nodes": 9, "ms": 70}}]}"#,
    );
    let b = net(
        r#"{"drivers": [{"budget": {"ms": 70, "nodes": 9}}, "aspiration", "null_window"],
            "root": {"order": ["non_losing", "ode"]}, "filters": ["non_losing", "must_win"]}"#,
    );
    assert_eq!(a, b);
    assert!(a.contains(r#""source": "driver.budget(nodes=9,ms=70)", "target": "Value⊥""#));
    // A chain's order is its meaning: swapped, another net.
    assert_ne!(
        net(r#"{"order": ["tt_best", "history"]}"#),
        net(r#"{"order": ["history", "tt_best"]}"#)
    );
    // The unit chain is a chain box given the list alone.
    let unit = net(r#"{"order": []}"#);
    assert!(!unit.contains("\"order."), "{unit}");
    assert!(unit.contains(r#""source": "MoveList·1", "target": "chain""#));
    assert!(!unit.contains(r#""source": "Key·1""#));
    // Defaults drawn: a plan that states them draws the same.
    let d = net("{}");
    for id in [
        "order.tt_best*",
        "order.immediate_win*",
        "order.history\"",
        "filter.must_win",
        "driver.null_window",
        "memo(entries=8388608)",
        "core(mode=exact)",
    ] {
        assert!(d.contains(id), "{id} in {d}");
    }
    assert_eq!(
        d,
        net(
            r#"{"order": ["tt_best", "immediate_win", "history"], "filters": ["must_win"],
                "drivers": ["null_window"], "root": {"order": []}, "memo": {"entries": 8388608}}"#
        )
    );
    // An illegal plan draws nothing.
    assert!(plan(r#"{"drivers": ["aspiration"]}"#).net().is_err());
}
