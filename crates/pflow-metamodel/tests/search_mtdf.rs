//! The `mtd_f` driver: it finds the values `null_window` finds, on the golden
//! nets and on a net with many distinct rewards, and shares the table with
//! every other search. Its node counts against `null_window` are reported by
//! the `search_mtdf` example and recorded in `docs/search-pieces-roadmap.md`.
mod common;

use pflow_metamodel::game::{Game, Solver};
use pflow_metamodel::{Estimate, Result, SearchPlan};

/// Five prizes: the rewards are the integers from -6 to 6, thirteen of them,
/// which `Game` steps through value by value (it lists at most 16).
const PRIZES: &[i64] = &[1, 1, 1, 1, 2];

fn plan(json: &str) -> SearchPlan {
    SearchPlan::from_json(json).unwrap()
}

/// Every position of the prize game with `weights`.
fn prize_positions(weights: &[i64]) -> (Game, Vec<Vec<i64>>) {
    let (g, m0) = common::prizes(weights);
    let all = common::reachable(&g, &m0);
    (g, all)
}

#[test]
fn mtd_f_finds_the_default_values_on_the_golden_nets() {
    for name in ["tic-tac-toe", "connect-3"] {
        let (g, m0) = common::game(name);
        let all = common::positions(name, &g, &m0);
        let want: Vec<f64> = common::fresh(&g, &all).iter().map(|x| x.0).collect();
        let p = plan(r#"{"drivers": ["mtd_f"]}"#);
        let got: Vec<f64> = all
            .iter()
            .map(|mk| p.solver(&g).unwrap().value(mk).unwrap().unwrap())
            .collect();
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn mtd_f_finds_the_values_of_many_rewards() {
    let (g, all) = prize_positions(PRIZES);
    assert!(all.len() > 40);
    let plans = [
        r#"{"drivers": ["null_window"]}"#,
        r#"{"drivers": []}"#,
        r#"{"drivers": ["mtd_f"]}"#,
        r#"{"drivers": ["mtd_f"], "filters": [], "order": []}"#,
        r#"{"drivers": ["mtd_f", {"budget": {"nodes": 100000}}], "memo": {"entries": 16}}"#,
    ];
    let want: Vec<f64> = all
        .iter()
        .map(|mk| {
            plan(plans[1])
                .solver(&g)
                .unwrap()
                .value(mk)
                .unwrap()
                .unwrap()
        })
        .collect();
    for json in plans {
        let p = plan(json);
        let got: Vec<f64> = all
            .iter()
            .map(|mk| p.solver(&g).unwrap().value(mk).unwrap().unwrap())
            .collect();
        assert_eq!(got, want, "by {json}");
    }
}

/// An estimate called `ode` that is wrong on purpose: a fixed reward.
struct Guess(f64);

impl Estimate for Guess {
    fn name(&self) -> &'static str {
        "ode"
    }

    fn estimate(&mut self, _: &Game, _: &[i64]) -> Result<f64> {
        Ok(self.0)
    }
}

#[test]
fn mtd_f_starts_from_a_guess_and_still_finds_the_value() {
    let (g, all) = prize_positions(PRIZES);
    let want: Vec<f64> = all
        .iter()
        .map(|mk| Solver::new(&g, usize::MAX).value(mk).unwrap().unwrap())
        .collect();
    let p = plan(
        r#"{"root": {"order": ["ode"]}, "drivers": ["mtd_f", "aspiration"], "memo": {"entries": 4096}}"#,
    );
    for guess in [-11.0, -3.0, 0.0, 2.5, 11.0, 50.0] {
        let got: Vec<f64> = all
            .iter()
            .map(|mk| {
                let mut s = p.solver_with(&g, Some(Box::new(Guess(guess)))).unwrap();
                s.value(mk).unwrap().unwrap()
            })
            .collect();
        assert_eq!(got, want, "guess {guess}");
    }
}

#[test]
fn mtd_f_runs_its_own_searches() {
    // Not null_window under another name: on a net with many rewards it
    // visits a different number of nodes.
    let (g, all) = prize_positions(PRIZES);
    let nodes = |json: &str| -> usize {
        let p = plan(json);
        all.iter()
            .map(|mk| {
                let mut s = p.solver(&g).unwrap();
                s.value(mk).unwrap().unwrap();
                s.nodes()
            })
            .sum()
    };
    assert_ne!(
        nodes(r#"{"drivers": ["mtd_f"]}"#),
        nodes(r#"{"drivers": ["null_window"]}"#)
    );
}

#[test]
fn mtd_f_shares_one_table_across_positions() {
    // One solver, every position in turn: what an earlier search proved
    // narrows a later one, and a later one is never wrong for it.
    let (g, all) = prize_positions(PRIZES);
    let want: Vec<f64> = all
        .iter()
        .map(|mk| Solver::new(&g, usize::MAX).value(mk).unwrap().unwrap())
        .collect();
    let mut s = plan(r#"{"drivers": ["mtd_f"]}"#).solver(&g).unwrap();
    let got: Vec<f64> = all.iter().map(|mk| s.value(mk).unwrap().unwrap()).collect();
    assert_eq!(got, want);
    // And again, with the table full of bounds.
    let again: Vec<f64> = all.iter().map(|mk| s.value(mk).unwrap().unwrap()).collect();
    assert_eq!(again, want);
}

#[test]
fn mtd_f_and_null_window_are_one_choice() {
    let e = plan(r#"{"drivers": ["null_window", "mtd_f"]}"#)
        .check()
        .unwrap_err()
        .to_string();
    assert!(e.contains("mtd_f") && e.contains("null_window"), "{e}");
    let e = plan(r#"{"drivers": ["mtd_f", "mtd_f"]}"#)
        .check()
        .unwrap_err()
        .to_string();
    assert!(e.contains("listed twice"), "{e}");
    assert!(plan(r#"{"drivers": ["mtd_f", {"budget": {"nodes": 5}}]}"#)
        .check()
        .is_ok());
}

#[test]
fn mtd_f_is_a_driver_in_the_normal_form() {
    let n = plan(r#"{"drivers": ["mtd_f"]}"#).normalized().unwrap();
    assert_eq!(
        n.drivers,
        Some(vec![serde_json::Value::String("mtd_f".into())])
    );
    assert!(plan(r#"{"drivers": ["mtd_f"]}"#)
        .net()
        .unwrap()
        .contains("driver.mtd_f"));
}

#[test]
fn a_budget_stops_mtd_f() {
    let (g, m0) = common::prizes(PRIZES);
    let p = plan(r#"{"drivers": ["mtd_f", {"budget": {"nodes": 3}}]}"#);
    assert_eq!(p.solver(&g).unwrap().value(&m0).unwrap(), None);
}
