//! Move filters for the game search: pieces that drop moves, or settle a
//! position at once, only where the value is already known.
//!
//! Both read what the net declares and nothing else: whose move a
//! transition is, and the reward's bounds ([`Game::bounds`], the most and
//! least a final marking pays). A bound is the end of the range, so a
//! position that reaches it is worth it whatever else the side could do:
//!
//! - `must_win`: a move that ends the game at the mover's best bound is the
//!   value. The position is settled as soon as one is found, before the
//!   other moves are played.
//! - `non_losing`: a move after which the other side has a reply that ends
//!   the game at the mover's worst bound is worth that bound, so no other
//!   move does worse: it is dropped. If every move is dropped, the position
//!   is worth the worst bound at once.
//!
//! - `forced_moves`: `non_losing`, asked only where it can matter. A goal
//!   of the other side wins on a reply only if that reply (and the goal
//!   firing after it) makes every one of its conditions hold. Conditions on
//!   places that no automatic transition and no move of the mover's side
//!   changes ([`Game::stable`]: a board's stones of the other colour) are
//!   the same at the position the reply is played from as now, so the
//!   reply must set the ones that fail now, and one move sets only so many
//!   ([`Game::stable_reach`]). Where every goal of the other side fails
//!   more of them than that, no move leaves it a win, and the check is
//!   skipped for all the moves at once; elsewhere it is `non_losing`'s, so
//!   the moves dropped, the position settled and the nodes searched are
//!   the same, only the replies played to find them are fewer. It can only
//!   keep a move `non_losing` would drop, never drop one it would keep, so
//!   values hold whatever the arcs. Bit markings only: over counts the
//!   check is `non_losing`'s everywhere. It reads the arcs and the goals,
//!   nothing else.
//!
//! None of them changes a value, so all are allowed in exact mode. A game whose
//! decisive finals pay less than the bound (two wins that never come
//! together, say) gives them nothing to act on, which costs only their
//! checks. The nodes a search counts do not include the replies
//! `non_losing` plays to check a move.

use crate::error::{Error, Result};
use crate::game::{Game, Walk};

/// A filter, by name. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    MustWin,
    NonLosing,
    ForcedMoves,
}

impl Filter {
    /// The names [`Filter::parse`] takes.
    pub const NAMES: [&'static str; 3] = ["must_win", "non_losing", "forced_moves"];

    /// The filters a solver runs unless told otherwise: `must_win`, which
    /// on the Connect-4 sample saves a quarter of the nodes and of the
    /// time. `non_losing` saves about as many nodes but, checking replies
    /// at every node, takes two and a half times as long there: it is for
    /// a few positions (a root's moves), not a whole search.
    pub const DEFAULT: [Filter; 1] = [Filter::MustWin];

    /// The filter called `name`.
    pub fn parse(name: &str) -> Result<Self> {
        Ok(match name {
            "must_win" => Self::MustWin,
            "non_losing" => Self::NonLosing,
            "forced_moves" => Self::ForcedMoves,
            _ => {
                return Err(Error::Game(format!(
                    "unknown filter {name:?} (known: {:?})",
                    Self::NAMES
                )))
            }
        })
    }

    /// The name [`Filter::parse`] takes for this filter.
    pub fn name(&self) -> &'static str {
        match self {
            Self::MustWin => "must_win",
            Self::NonLosing => "non_losing",
            Self::ForcedMoves => "forced_moves",
        }
    }
}

/// The filters a search runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Filters {
    pub must_win: bool,
    pub non_losing: bool,
    pub forced_moves: bool,
}

impl Filters {
    pub fn of(filters: &[Filter]) -> Self {
        Self {
            must_win: filters.contains(&Filter::MustWin),
            non_losing: filters.contains(&Filter::NonLosing),
            forced_moves: filters.contains(&Filter::ForcedMoves),
        }
    }

    /// Whether a move of side `s` from `w` could leave a goal of the other
    /// side in reach: false means no move can, so no reply need be played
    /// (see the module docs, `forced_moves`).
    pub fn threat<W: Walk>(&self, g: &Game, s: f64, w: &W) -> bool {
        if W::COUNTS {
            return true;
        }
        g.goals.iter().enumerate().any(|(i, goal)| {
            goal.side == -s && w.deficit_among(g, goal.t, &g.stable[i]) <= g.stable_reach[i]
        })
    }

    /// The best bound for side `s`.
    pub fn bound(&self, g: &Game, s: f64) -> f64 {
        if s > 0.0 {
            g.bounds.1
        } else {
            g.bounds.0
        }
    }

    /// Whether `next` is final at side `s`'s best bound.
    pub fn best<W: Walk>(&self, g: &Game, s: f64, next: &W) -> Result<bool> {
        Ok(next.over(g) && next.score(g)? == self.bound(g, s))
    }

    /// Whether, after a move of side `s`, the other side is to move at
    /// `next` and has a reply ending the game at `s`'s worst bound; `None`
    /// when the engine cannot hold a reply's result.
    ///
    /// With goals declared, only replies that can bring a goal of the other
    /// side from its deficit at `next` to enabled in one firing are played
    /// ([`Game::feeders`]), and none when no goal is that close. A game
    /// that reaches the bound some other way is then missed, which only
    /// keeps a move the filter could have dropped.
    pub fn lost<W: Walk>(&self, g: &Game, s: f64, next: &W) -> Result<Option<bool>> {
        let mut replies = Vec::with_capacity(8);
        next.moves(g, &mut replies);
        if replies.is_empty() || g.mover(&replies)? != -s {
            return Ok(Some(false));
        }
        let wins = |u: usize| -> Result<Option<bool>> {
            let mut after = next.clone();
            if !after.play(g, u)? {
                return Ok(None);
            }
            Ok(Some(self.best(g, -s, &after)?))
        };
        if g.goals.is_empty() {
            for u in replies {
                match wins(u)? {
                    Some(false) => {}
                    other => return Ok(other),
                }
            }
            return Ok(Some(false));
        }
        let mut tried: Vec<u32> = vec![];
        for (i, goal) in g.goals.iter().enumerate() {
            if goal.side != -s {
                continue;
            }
            let d = next.deficit(g, goal.t);
            if d > g.reach[i] {
                continue;
            }
            for &(u, touch) in &g.feeders[i] {
                if touch < d || !next.has(u as usize) || tried.contains(&u) {
                    continue;
                }
                tried.push(u);
                match wins(u as usize)? {
                    Some(false) => {}
                    other => return Ok(other),
                }
            }
        }
        Ok(Some(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for n in Filter::NAMES {
            assert_eq!(Filter::parse(n).unwrap().name(), n);
        }
        let e = Filter::parse("beam").unwrap_err().to_string();
        assert!(e.contains("beam") && e.contains("must_win"), "{e}");
    }
}
