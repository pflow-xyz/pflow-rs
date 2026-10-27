//! Move order for the game search: a chain of pieces, each a sort key per
//! move, compared one after the other (the first to differ decides).
//!
//! Order never changes a value, only how soon a cutoff comes: alpha-beta
//! returns the same number whatever order it tries the moves in. So a piece
//! is always sound, and only a bad one costs nodes. Pieces are an enum, not
//! boxed trait objects, so ranking a move costs a match and no allocation.

use crate::error::{Error, Result};
use crate::game::{Game, Walk};

/// The most pieces a chain holds.
const MAX_PIECES: usize = 8;

/// What a piece may read when it ranks move `t`.
pub(crate) struct Ctx<'a> {
    pub g: &'a Game,
    /// The side to move (+1 maximizes the reward).
    pub mover: f64,
    /// The move the transposition table remembers as best here, if any.
    pub first: u32,
    /// Per transition: how often it caused a cutoff.
    pub history: &'a [u32],
}

/// One sort key per move: larger is tried sooner.
pub(crate) trait Orderer {
    /// Whether a nonzero key settles the move's rank: the pieces after this
    /// one do not sort it further (it sits in a top tier of its own, as when
    /// the table names it, or when it wins).
    const DECISIVE: bool = false;

    /// The key of move `t`, which leads to `next`.
    fn key<W: Walk>(&self, cx: &Ctx, t: usize, next: &W) -> Result<u64>;

    /// Move `t` just caused a cutoff.
    fn cutoff(&self, _history: &mut [u32], _t: usize) {}
}

/// The move the transposition table remembers as best.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtBest;

/// A move that ends the game in the mover's favour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImmediateWin;

/// Moves that caused cutoffs before, per transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct History;

impl Orderer for TtBest {
    const DECISIVE: bool = true;

    fn key<W: Walk>(&self, cx: &Ctx, t: usize, _: &W) -> Result<u64> {
        Ok((t as u32 == cx.first) as u64)
    }
}

impl Orderer for ImmediateWin {
    const DECISIVE: bool = true;

    fn key<W: Walk>(&self, cx: &Ctx, _: usize, next: &W) -> Result<u64> {
        Ok((next.over(cx.g) && cx.mover * next.score(cx.g)? > 0.0) as u64)
    }
}

impl Orderer for History {
    fn key<W: Walk>(&self, cx: &Ctx, t: usize, _: &W) -> Result<u64> {
        Ok(cx.history[t] as u64)
    }

    fn cutoff(&self, history: &mut [u32], t: usize) {
        history[t] += 1;
    }
}

/// A piece of a chain, by name. See [`Piece::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    TtBest(TtBest),
    ImmediateWin(ImmediateWin),
    History(History),
}

impl Piece {
    /// The names [`Piece::parse`] takes.
    pub const NAMES: [&'static str; 3] = ["tt_best", "immediate_win", "history"];

    /// The piece called `name`.
    pub fn parse(name: &str) -> Result<Self> {
        Ok(match name {
            "tt_best" => Self::TtBest(TtBest),
            "immediate_win" => Self::ImmediateWin(ImmediateWin),
            "history" => Self::History(History),
            _ => {
                return Err(Error::Game(format!(
                    "unknown order piece {name:?} (known: {:?})",
                    Self::NAMES
                )))
            }
        })
    }

    /// The name [`Piece::parse`] takes for this piece.
    pub fn name(&self) -> &'static str {
        match self {
            Self::TtBest(_) => "tt_best",
            Self::ImmediateWin(_) => "immediate_win",
            Self::History(_) => "history",
        }
    }
}

impl Orderer for Piece {
    fn key<W: Walk>(&self, cx: &Ctx, t: usize, next: &W) -> Result<u64> {
        match self {
            Self::TtBest(p) => p.key(cx, t, next),
            Self::ImmediateWin(p) => p.key(cx, t, next),
            Self::History(p) => p.key(cx, t, next),
        }
    }

    fn cutoff(&self, history: &mut [u32], t: usize) {
        match self {
            Self::TtBest(p) => p.cutoff(history, t),
            Self::ImmediateWin(p) => p.cutoff(history, t),
            Self::History(p) => p.cutoff(history, t),
        }
    }
}

impl Piece {
    /// [`Orderer::DECISIVE`] of the piece inside.
    fn decisive(&self) -> bool {
        match self {
            Self::TtBest(_) => TtBest::DECISIVE,
            Self::ImmediateWin(_) => ImmediateWin::DECISIVE,
            Self::History(_) => History::DECISIVE,
        }
    }
}

/// The keys of one move, one per piece of the chain.
pub(crate) type Key = [u64; MAX_PIECES];

/// Pieces in order of priority: a move's keys are compared piece by piece
/// (up to a decisive piece's nonzero key), and ties keep the net's own order
/// of the moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pieces: Vec<Piece>,
}

impl Default for Chain {
    /// The remembered best move, then a winning one, then by history.
    fn default() -> Self {
        Self {
            pieces: vec![
                Piece::TtBest(TtBest),
                Piece::ImmediateWin(ImmediateWin),
                Piece::History(History),
            ],
        }
    }
}

impl Chain {
    /// A chain of `pieces`, highest priority first.
    pub fn new(pieces: &[Piece]) -> Result<Self> {
        if pieces.len() > MAX_PIECES {
            return Err(Error::Game(format!(
                "an order chain holds at most {MAX_PIECES} pieces, not {}",
                pieces.len()
            )));
        }
        Ok(Self {
            pieces: pieces.to_vec(),
        })
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// The keys of move `t`.
    pub(crate) fn key<W: Walk>(&self, cx: &Ctx, t: usize, next: &W) -> Result<Key> {
        let mut k = [0; MAX_PIECES];
        for (slot, p) in k.iter_mut().zip(&self.pieces) {
            *slot = p.key(cx, t, next)?;
            if *slot != 0 && p.decisive() {
                break;
            }
        }
        Ok(k)
    }

    /// How two moves' keys compare: `Less` when `a` is tried sooner.
    pub(crate) fn before(&self, a: &Key, b: &Key) -> std::cmp::Ordering {
        let n = self.pieces.len();
        b[..n].cmp(&a[..n])
    }

    pub(crate) fn cutoff(&self, history: &mut [u32], t: usize) {
        for p in &self.pieces {
            p.cutoff(history, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for n in Piece::NAMES {
            assert_eq!(Piece::parse(n).unwrap().name(), n);
        }
        let e = Piece::parse("nope").unwrap_err().to_string();
        assert!(e.contains("nope") && e.contains("tt_best"), "{e}");
    }

    #[test]
    fn a_chain_has_a_limit() {
        let p = Piece::parse("history").unwrap();
        assert!(Chain::new(&[p; MAX_PIECES]).is_ok());
        assert!(Chain::new(&[p; MAX_PIECES + 1]).is_err());
    }
}
