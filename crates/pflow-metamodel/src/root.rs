//! The root of a game search: pieces that run once per position searched,
//! not at every node under it, so they can afford what a node cannot.
//!
//! - An [`Estimate`] guesses a position's value: an ODE solve, a cache, a
//!   learned model. This crate defines none (`pflow_solver::game` has the
//!   mass-action ODE, named `ode`). The guess breaks the order chain's ties
//!   among the root's moves (in a fresh solver the history is empty, so
//!   past `tt_best` and `immediate_win` it decides), and the aspiration
//!   driver starts where the best guess points ([`Solver::with_aspiration`]).
//!   Leading the chain instead was tried: within a few percent either way
//!   over two Connect-4 samples, and this way never overrides the chain.
//! - `non_losing` at the root: [`crate::filter`]'s `non_losing`, checked
//!   for the root's moves only, so it costs a few replies per position
//!   rather than per node. It catches what a guess misses: a move that
//!   leaves the other side a win at once (a must-block the guess ranks
//!   low).
//!
//! Neither changes a value. A guess only orders the root's moves and picks
//! which yes/no search runs first, and `non_losing` drops only moves whose
//! value is already known. `tests` hold both to that with guesses that are
//! wrong on purpose.
//!
//! [`Solver::with_aspiration`]: crate::game::Solver::with_aspiration

use crate::error::Result;
use crate::game::Game;

/// A guess at the value of a position. See the module docs.
pub trait Estimate {
    /// The name a [`crate::SearchPlan`] calls it by.
    fn name(&self) -> &'static str;

    /// A guess at the value (+1 side's view) of settled position `mk` of
    /// `g`, which has a move. Any number will do for the search's sake; one
    /// outside [`Game::bounds`] is read as the nearer bound.
    fn estimate(&mut self, g: &Game, mk: &[i64]) -> Result<f64>;
}

/// What a solver does at the root.
#[derive(Default)]
pub(crate) struct Root<'a> {
    pub estimate: Option<Box<dyn Estimate + 'a>>,
    pub non_losing: bool,
    pub aspiration: bool,
}

impl Root<'_> {
    /// Whether any piece runs.
    pub fn any(&self) -> bool {
        self.estimate.is_some() || self.non_losing
    }
}

/// The root's moves at one position, as its pieces see them.
pub(crate) struct Ranked {
    /// The moves kept, best first.
    pub order: Vec<usize>,
    /// Per transition: larger is tried sooner; 0 for a move dropped.
    pub rank: Vec<u64>,
    /// The best guess among the moves kept, within the bounds.
    pub guess: Option<f64>,
}
