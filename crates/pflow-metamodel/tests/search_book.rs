//! Opening books from label caches in pflow-native's format: read back
//! exactly, refused when they are for another game, and a search seeded
//! with one finds the default values in fewer nodes.
mod common;

use pflow_metamodel::game::{Game, Solver};
use pflow_metamodel::Book;
use serde_json::{json, Value};

/// The header pflow-native writes for `g`'s labels.
fn header(g: &Game) -> Value {
    json!({
        "labels": 1,
        "places": g.ix.places,
        "transitions": g.ix.transitions,
        "auto": g.auto,
        "sign": g.sign,
        "reward": g.reward,
        "budget": common::BUDGET,
    })
}

/// A label cache for `all`, as pflow-native's `rnn.verify` writes one: the
/// value of each move in the mover's view, `null` where one ran past
/// `budget`.
fn labels(g: &Game, all: &[Vec<i64>], budget: usize) -> String {
    let mut s = Solver::new(g, budget);
    let mut out = format!("{}\n", header(g));
    for mk in all {
        let moves = g.moves(mk);
        let side = g.mover(&moves).unwrap();
        let vals: Option<Vec<f64>> = moves
            .iter()
            .map(|&t| {
                s.value(&g.play(mk, t).unwrap())
                    .unwrap()
                    .map(|x| side * x + 0.0)
            })
            .collect();
        out.push_str(&format!("{}\n", json!({"mk": mk, "vals": vals})));
    }
    out
}

#[test]
fn a_book_from_labels_keeps_the_values_and_saves_nodes() {
    let (g, m0) = common::game("connect-3");
    let all = common::positions("connect-3", &g, &m0);
    let want = common::fresh(&g, &all);
    let seen = common::sample(&g, &m0, 11, 60, &[4, 6, 8]);
    let mut text = labels(&g, &seen, common::BUDGET);
    // A line past its budget, and a last line cut off mid-write.
    text.push_str(&format!("{}\n", json!({"mk": m0, "vals": null})));
    text.push_str(r#"{"mk": [1, 0"#);
    let book = Book::from_labels(&g, &text).unwrap();
    let moves: usize = seen.iter().map(|mk| g.moves(mk).len()).sum();
    assert_eq!(book.len(), seen.len() + moves);
    let (mut plain, mut booked) = (0, 0);
    for (mk, &(v, n)) in all.iter().zip(&want) {
        let mut s = Solver::new(&g, common::BUDGET).with_book(&book);
        assert_eq!(s.value(mk).unwrap(), Some(v), "at {mk:?}");
        plain += n;
        booked += s.nodes();
    }
    assert!(
        booked < plain,
        "{booked} nodes with the book, {plain} without"
    );
}

#[test]
fn labels_for_another_game_are_refused() {
    let (g, m0) = common::game("connect-3");
    let (ttt, _) = common::game("tic-tac-toe");
    let text = labels(&g, std::slice::from_ref(&m0), 1000);
    let e = Book::from_labels(&ttt, &text).unwrap_err().to_string();
    assert!(e.contains("places"), "{e}");
    let mut head = header(&g);
    head["sign"][0] = json!(1.0 - g.sign[0]);
    let rest: String = text.lines().skip(1).collect();
    let e = Book::from_labels(&g, &format!("{head}\n{rest}"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("sign"), "{e}");
    // A line that does not fit the game, and one that is not JSON, before
    // the last.
    let bad = format!("{}\n{}\n", header(&g), json!({"mk": m0, "vals": [1.0]}));
    let e = Book::from_labels(&g, &bad).unwrap_err().to_string();
    assert!(e.contains("line 2") && e.contains("moves"), "{e}");
    // A line for a position with no move, which once panicked.
    let mut end = m0.clone();
    while let Some(&t) = g.moves(&end).first() {
        end = g.play(&end, t).unwrap();
    }
    let bad = format!("{}\n{}\n", header(&g), json!({"mk": end, "vals": []}));
    let e = Book::from_labels(&g, &bad).unwrap_err().to_string();
    assert!(e.contains("line 2") && e.contains("final"), "{e}");
    let bad = format!(
        "{}\nnot json\n{}\n",
        header(&g),
        json!({"mk": m0, "vals": null})
    );
    let e = Book::from_labels(&g, &bad).unwrap_err().to_string();
    assert!(e.contains("line 2"), "{e}");
}
