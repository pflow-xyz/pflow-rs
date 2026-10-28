//! An opening book: exact values of positions, kept in a solver's table
//! ahead of any search ([`crate::game::Solver::with_book`]), so a search
//! that reaches one of them stops there.
//!
//! A book's entries are pinned ([`crate::memo`]): a search never pushes
//! them out, and they take their slots only for as long as they are not
//! all the slots a new position could take. They are keyed as the search
//! keys a position, so one is found whichever way the search reaches it.
//!
//! [`Book::from_labels`] reads pflow-native's label cache (`rnn.imitate`
//! and `rnn.verify`'s `labels`): a JSONL file whose first line names what
//! the labels depend on (the places, transitions, automatic transitions,
//! sides and reward) and whose other lines each hold a settled position
//! and the exact value of every move there (`{"mk": [...], "vals": [...]}`,
//! the moves in transition order, the values in the mover's view), or no
//! values (`null`) where its search ran past the budget. Only exact lines
//! are read: each gives the position's own value and best move, and the
//! value of the position after each move. A book holds what it is given:
//! a wrong value in it is a wrong value out.

use serde_json::Value;

use crate::error::{Error, Result};
use crate::game::Game;

/// Exact values of positions. See the module docs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Book {
    /// (settled marking, value in the +1 side's view, best move if known).
    pub(crate) entries: Vec<(Vec<i64>, f64, Option<usize>)>,
}

fn book_error(msg: String) -> Error {
    Error::Game(format!("book: {msg}"))
}

impl Book {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that settled position `mk` is worth exactly `value` (+1
    /// side's view), `best` doing best when known.
    pub fn add(&mut self, mk: Vec<i64>, value: f64, best: Option<usize>) {
        self.entries.push((mk, value, best));
    }

    /// The positions recorded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The book a label cache holds for `g` (see the module docs), refused
    /// when its header names another net or other rules, or a line does
    /// not fit `g`. A last line cut off mid-write is skipped.
    pub fn from_labels(g: &Game, text: &str) -> Result<Self> {
        let mut lines = text.lines();
        let head: Value = serde_json::from_str(lines.next().unwrap_or(""))
            .map_err(|e| book_error(format!("header: {e}")))?;
        let reward: Vec<Value> = g
            .reward
            .iter()
            .map(|&(p, w)| serde_json::json!([p, w]))
            .collect();
        let want = [
            ("labels", serde_json::json!(1)),
            ("places", serde_json::json!(g.ix.places)),
            ("transitions", serde_json::json!(g.ix.transitions)),
            ("auto", serde_json::json!(g.auto)),
            ("sign", serde_json::json!(g.sign)),
            ("reward", Value::Array(reward)),
        ];
        for (field, v) in want {
            if head[field] != v {
                return Err(book_error(format!(
                    "the labels' {field:?} is not this game's: they are for another net or \
                     other rules"
                )));
            }
        }
        let mut book = Self::new();
        let lines: Vec<&str> = lines.collect();
        for (n, line) in lines.iter().enumerate() {
            let at = |msg: String| book_error(format!("line {}: {msg}", n + 2));
            let x: Value = match serde_json::from_str(line) {
                Ok(x) => x,
                Err(_) if n + 1 == lines.len() => break,
                Err(e) => return Err(at(e.to_string())),
            };
            let mk: Vec<i64> =
                serde_json::from_value(x["mk"].clone()).map_err(|e| at(e.to_string()))?;
            let vals: Option<Vec<f64>> =
                serde_json::from_value(x["vals"].clone()).map_err(|e| at(e.to_string()))?;
            let Some(vals) = vals else {
                continue;
            };
            if mk.len() != g.ix.places.len() {
                return Err(at(format!(
                    "a marking of {} places, for a net of {}",
                    mk.len(),
                    g.ix.places.len()
                )));
            }
            let moves = g.moves(&mk);
            if moves.is_empty() {
                return Err(at("a final position: there is no move to label".into()));
            }
            if moves.len() != vals.len() {
                return Err(at(format!(
                    "{} values for the {} moves there",
                    vals.len(),
                    moves.len()
                )));
            }
            let s = g.mover(&moves)?;
            let mut best = 0;
            for (i, (&t, &v)) in moves.iter().zip(&vals).enumerate() {
                if v > vals[best] {
                    best = i;
                }
                book.add(g.play(&mk, t)?, s * v, None);
            }
            book.add(mk, s * vals[best], Some(moves[best]));
        }
        Ok(book)
    }
}
