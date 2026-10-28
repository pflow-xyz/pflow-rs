//! Exact values of two-sided games played on a net: alpha-beta over the
//! firing rule, one position at a time, with a node budget.
//!
//! A [`Game`] reads a net as a game. `auto` transitions fire as soon as
//! they are enabled (a win detector, a cleanup step), lowest index first,
//! until none is: the marking is then settled. Every other transition is a
//! move, and `sign` says whose: +1 for the side maximizing the reward, -1
//! for the other. At a settled marking the side to move is the one every
//! enabled move belongs to, and a marking with no move is final, worth the
//! reward read from it: `reward` weights per place, each place holding at
//! most one token at the end. Nothing here knows any particular game.
//!
//! [`Solver::value`] searches over one of two engines:
//!
//! - [`BitModel`], when the marking's counts are 0 or 1 and the net has at
//!   most 256 places and 256 transitions: a marking is a few words, and so
//!   is its enabled set, kept current by [`BitModel::fire_tracked`]. A
//!   firing that would leave a place outside 0/1 is refused, and the
//!   position is searched again by counts.
//! - [`IndexedModel`] with its [`Enabled`] sets otherwise
//!   ([`IndexedModel::fire_tracked`]).
//!
//! Rewards a final marking can have are few in most games (win, draw,
//! loss), so a value is found as a run of yes/no searches, one per gap
//! between adjacent rewards ("is it above this one?"), each with a window
//! that narrow. They share one transposition table ([`crate::memo`]),
//! bounded and persisting across calls, and a chain of [`crate::order`]
//! pieces (by default the table's best move, a winning one, then history)
//! orders moves. [`crate::filter`] pieces (by default, settling a position
//! at a winning move) drop moves that cannot change the value, and
//! [`crate::root`] pieces, off by default, run once at the position given:
//! an estimate that orders its moves and aims the run of yes/no searches.
//!
//! A [`crate::book`] seeds the table with exact values ahead of any search,
//! and [`Solver::choose`] gives a best move as well as the value.
//!
//! A value is exact. Past its budget (nodes, and time if given) a search
//! returns `None`, never a guess; only `choose` in play mode guesses. `tests` hold it to a plain minimax on tic-tac-toe (bits) and on
//! Nim (counts), at every reachable position.

use std::time::{Duration, Instant};

use crate::bits::BitModel;
use crate::book::Book;
use crate::error::{Error, Result};
use crate::filter::{Filter, Filters};
use crate::indexed::{Enabled, IndexedModel, TransitionSet};
use crate::memo::{self, Entry, Table};
use crate::order::{Chain, Ctx};
use crate::root::{Estimate, Ranked, Root};

/// A two-sided game on a net, over dense markings in `ix.places` order.
/// See the module docs.
#[derive(Debug, Clone)]
pub struct Game {
    pub ix: IndexedModel,
    /// `ix` over 0/1 markings.
    pub bits: BitModel,
    /// Per transition (in `ix` order): fires as soon as it is enabled.
    pub auto: Vec<bool>,
    /// Per transition: the side that chooses it (+1 maximizes the reward).
    pub sign: Vec<f64>,
    /// (dense place index, weight): the reward, read from a final marking.
    pub reward: Vec<(usize, f64)>,
    /// The least and greatest reward a final marking can have (each reward
    /// place holding 0 or 1 tokens).
    pub bounds: (f64, f64),
    /// `auto` as a set, and its complement (the moves).
    pub auto_set: TransitionSet,
    pub move_set: TransitionSet,
    /// The same as words, when the bit engine applies.
    auto_words: [u64; INLINE],
    move_words: [u64; INLINE],
    /// The automatic transitions that pay a reward: what a side is playing
    /// towards. Empty when the net declares none.
    pub goals: Vec<Goal>,
    /// Per transition: the goals (indices into `goals`) of its own side that
    /// its output places feed, a goal being fed by a place it takes or reads
    /// from. A place every goal of the side needs says nothing of where to
    /// play and does not count: a turn token each win detector consumes, for
    /// one.
    pub fed: Vec<Vec<u32>>,
    /// Per transition: how many goals `fed` holds.
    pub centrality: Vec<u32>,
    /// Per goal: the moves of its side that change a place it has a
    /// condition on, each with how far one firing can bring the goal
    /// closer to enabled (its deficit, [`Walk::deficit`], drops by at most
    /// that much), and the most of these. See [`touches`].
    pub(crate) feeders: Vec<Vec<(u32, u64)>>,
    pub(crate) reach: Vec<u64>,
    /// Every reward a final marking can have, ascending, when there are
    /// few: the values a search decides between.
    values: Vec<f64>,
}

/// An automatic transition that puts a token on a reward place (a win being
/// claimed, say): `side` is the sign of the reward it pays, the side it is
/// a goal of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Goal {
    pub t: usize,
    pub side: f64,
}

/// The most words of places, and of transitions, the bit engine keeps
/// inline (256 of each), so a child is a copy of a few words.
const INLINE: usize = 4;

/// The most reward places, and distinct rewards, worth searching value by
/// value; past either, a search takes the whole range at once.
const VALUES: usize = 16;

/// How long settling may run before the automatic transitions count as
/// never stopping.
const SETTLE: usize = 10_000;

fn endless() -> Error {
    Error::Game("automatic transitions never stop firing".into())
}

/// The distinct sums of some of `weights`, ascending, or none past
/// [`VALUES`].
fn rewards(weights: &[f64]) -> Vec<f64> {
    if weights.len() > VALUES {
        return vec![];
    }
    let mut sums = vec![0.0];
    for &w in weights {
        let more: Vec<f64> = sums.iter().map(|x| x + w).collect();
        sums.extend(more);
        sums.sort_by(f64::total_cmp);
        sums.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        if sums.len() > VALUES {
            return vec![];
        }
    }
    sums
}

/// The goals of a game: the automatic transitions whose outputs pay a net
/// reward.
fn goals_of(ix: &IndexedModel, auto: &[bool], reward: &[(usize, f64)]) -> Vec<Goal> {
    (0..auto.len())
        .filter(|&t| auto[t])
        .filter_map(|t| {
            let paid: f64 = ix.arcs[t]
                .outputs
                .iter()
                .map(|&(p, w)| {
                    let r: f64 = reward.iter().filter(|x| x.0 == p).map(|x| x.1).sum();
                    r * w as f64
                })
                .sum();
            (paid != 0.0).then(|| Goal {
                t,
                side: paid.signum(),
            })
        })
        .collect()
}

/// [`Game::fed`] for the goals of a game.
fn fed_goals(ix: &IndexedModel, goals: &[Goal], sign: &[f64]) -> Vec<Vec<u32>> {
    let fed = |g: &Goal, p: usize| {
        let a = &ix.arcs[g.t];
        a.inputs.iter().chain(&a.reads).any(|x| x.0 == p)
    };
    (0..sign.len())
        .map(|t| {
            let mine: Vec<(usize, &Goal)> = goals
                .iter()
                .enumerate()
                .filter(|g| g.1.side == sign[t])
                .collect();
            let mut hit = vec![false; mine.len()];
            for &(p, _) in &ix.arcs[t].outputs {
                if mine.iter().all(|g| fed(g.1, p)) {
                    continue;
                }
                for (h, g) in hit.iter_mut().zip(&mine) {
                    *h |= fed(g.1, p);
                }
            }
            hit.iter()
                .zip(&mine)
                .filter(|x| *x.0)
                .map(|x| x.1 .0 as u32)
                .collect()
        })
        .collect()
}

/// [`Game::feeders`]: for each goal, the moves `u` of its side with
/// `touch(u) > 0`, where `touch(u)` sums `|Δu(p)|` over the goal's
/// conditions (each input, read, inhibitor and capacity on a place `p`).
/// A condition's share of a deficit, in tokens or in bits, moves by at
/// most `|Δu(p)|` when `u` fires, so `touch(u)` bounds how much one firing
/// of `u` can lower the deficit.
fn touches(ix: &IndexedModel, auto: &[bool], sign: &[f64], goals: &[Goal]) -> Vec<Vec<(u32, u64)>> {
    let delta = |u: usize, p: usize| -> i64 {
        let at = |xs: &[(usize, i64)]| -> i64 { xs.iter().filter(|x| x.0 == p).map(|x| x.1).sum() };
        at(&ix.arcs[u].outputs) - at(&ix.arcs[u].inputs)
    };
    goals
        .iter()
        .map(|g| {
            let a = &ix.arcs[g.t];
            let places: Vec<usize> = a
                .inputs
                .iter()
                .chain(&a.reads)
                .chain(&a.inhibitors)
                .map(|x| x.0)
                .chain(a.capacities.iter().map(|x| x.0))
                .collect();
            (0..auto.len())
                .filter(|&u| !auto[u] && sign[u] == g.side)
                .filter_map(|u| {
                    let touch: u64 = places.iter().map(|&p| delta(u, p).unsigned_abs()).sum();
                    (touch > 0).then_some((u as u32, touch))
                })
                .collect()
        })
        .collect()
}

/// A set's words, inline, when it fits.
fn inline(s: &TransitionSet) -> [u64; INLINE] {
    let mut w = [0; INLINE];
    if s.words.len() <= INLINE {
        w[..s.words.len()].copy_from_slice(&s.words);
    }
    w
}

impl Game {
    /// A game over `ix`; the arguments as the fields of the same names.
    pub fn new(
        ix: IndexedModel,
        auto: Vec<bool>,
        sign: Vec<f64>,
        reward: Vec<(usize, f64)>,
    ) -> Self {
        let n = auto.len();
        let auto_set = ix.set_of((0..n).filter(|&t| auto[t]));
        let move_set = ix.set_of((0..n).filter(|&t| !auto[t]));
        let bounds = (
            reward.iter().map(|x| x.1.min(0.0)).sum(),
            reward.iter().map(|x| x.1.max(0.0)).sum(),
        );
        let goals = goals_of(&ix, &auto, &reward);
        let fed = fed_goals(&ix, &goals, &sign);
        let feeders = touches(&ix, &auto, &sign, &goals);
        Self {
            reach: feeders
                .iter()
                .map(|f| f.iter().map(|x| x.1).max().unwrap_or(0))
                .collect(),
            feeders,
            centrality: fed.iter().map(|f| f.len() as u32).collect(),
            fed,
            goals,
            values: rewards(&reward.iter().map(|x| x.1).collect::<Vec<_>>()),
            bits: ix.bits(),
            auto_words: inline(&auto_set),
            move_words: inline(&move_set),
            auto_set,
            move_set,
            ix,
            auto,
            sign,
            reward,
            bounds,
        }
    }

    /// Fires automatic transitions, lowest index first, until none is
    /// enabled.
    pub fn settle(&self, mk: &mut [i64]) -> Result<()> {
        for _ in 0..SETTLE {
            let Some(t) = (0..self.auto.len()).find(|&t| self.auto[t] && self.ix.enabled(t, mk))
            else {
                return Ok(());
            };
            self.ix.fire_in_place(t, mk);
        }
        Err(endless())
    }

    /// The moves at a settled marking.
    pub fn moves(&self, mk: &[i64]) -> Vec<usize> {
        self.ix.enabled_transitions(mk)
    }

    /// The reward of a final marking.
    pub fn score(&self, mk: &[i64]) -> Result<f64> {
        self.check(self.reward.iter().map(|&(p, w)| w * mk[p] as f64).sum())
    }

    /// A reward, refused outside the bounds.
    fn check(&self, x: f64) -> Result<f64> {
        if x < self.bounds.0 || x > self.bounds.1 {
            return Err(Error::Game(format!(
                "a final marking scores {x}, outside the reward's assumed range {:?} (each reward place ending \
                 with at most one token)",
                self.bounds
            )));
        }
        Ok(x)
    }

    /// The side to move at a settled marking with moves `moves`.
    pub fn mover(&self, moves: &[usize]) -> Result<f64> {
        let s = self.sign[moves[0]];
        if s == 0.0 || moves.iter().any(|&t| self.sign[t] != s) {
            let names: Vec<&str> = moves
                .iter()
                .map(|&t| self.ix.transitions[t].as_str())
                .collect();
            return Err(Error::Game(format!(
                "the enabled moves {names:?} do not all belong to one side"
            )));
        }
        Ok(s)
    }

    /// The settled marking after move `t`.
    pub fn play(&self, mk: &[i64], t: usize) -> Result<Vec<i64>> {
        let mut next = self.ix.fire(t, mk);
        self.settle(&mut next)?;
        Ok(next)
    }
}

/// A settled marking the search plays moves from.
pub(crate) trait Walk: Clone {
    /// The moves here, into `out`.
    fn moves(&self, g: &Game, out: &mut Vec<usize>);
    /// Whether no move is enabled.
    fn over(&self, g: &Game) -> bool;
    /// Whether transition `t` is enabled.
    fn has(&self, t: usize) -> bool;
    /// Plays move `t` and settles. False when this engine cannot represent
    /// the result, which leaves `self` meaning nothing.
    fn play(&mut self, g: &Game, t: usize) -> Result<bool>;
    fn score(&self, g: &Game) -> Result<f64>;
    /// How many tokens (or, over bits, conditions) transition `t` is short
    /// of enabled here: [`IndexedModel::deficit`], [`BitModel::deficit`].
    fn deficit(&self, g: &Game, t: usize) -> u64;
    /// Whether this is the counts engine, whose keys are hashes.
    const COUNTS: bool;
    /// The transposition table's key: the marking's words over bits, a
    /// 128-bit hash of it over counts.
    fn key(&self) -> memo::Key;
}

/// Mixes words into a 128-bit hash.
fn key_of(xs: impl Iterator<Item = u64>) -> u128 {
    let (mut a, mut b) = (0x9E37_79B9_7F4A_7C15u64, 0xC2B2_AE3D_27D4_EB4Fu64);
    for (i, v) in xs.enumerate() {
        let x = v.wrapping_add((i as u64) << 40);
        a = (a ^ x).wrapping_mul(0xFF51_AFD7_ED55_8CCD).rotate_left(29);
        b = (b ^ x.rotate_left(17))
            .wrapping_mul(0xC4CE_B9FE_1A85_EC53)
            .rotate_left(31);
    }
    (a as u128) << 64 | b as u128
}

/// Counts, with the enabled set kept current.
#[derive(Clone)]
pub(crate) struct Counts {
    mk: Vec<i64>,
    en: Enabled,
}

impl Walk for Counts {
    fn moves(&self, _: &Game, out: &mut Vec<usize>) {
        out.extend(self.en.iter());
    }

    fn over(&self, _: &Game) -> bool {
        self.en.is_empty()
    }

    fn has(&self, t: usize) -> bool {
        self.en.contains(t)
    }

    fn play(&mut self, g: &Game, t: usize) -> Result<bool> {
        g.ix.fire_tracked(t, &mut self.mk, &mut self.en);
        for _ in 0..SETTLE {
            let Some(u) = self.en.first_in(&g.auto_set) else {
                return Ok(true);
            };
            g.ix.fire_tracked(u, &mut self.mk, &mut self.en);
        }
        Err(endless())
    }

    fn score(&self, g: &Game) -> Result<f64> {
        g.score(&self.mk)
    }

    fn deficit(&self, g: &Game, t: usize) -> u64 {
        g.ix.deficit(t, &self.mk)
    }

    const COUNTS: bool = true;

    fn key(&self) -> memo::Key {
        let h = key_of(self.mk.iter().map(|&v| v as u64));
        [(h >> 64) as u64, h as u64, 0, 0]
    }
}

/// A 0/1 marking as bits, with its enabled set.
#[derive(Clone, Copy)]
struct Bits {
    mk: [u64; INLINE],
    en: [u64; INLINE],
}

/// The set bits of `a & b`, in index order.
fn both(a: [u64; INLINE], b: [u64; INLINE]) -> impl Iterator<Item = usize> {
    (0..INLINE).flat_map(move |i| {
        let mut w = a[i] & b[i];
        std::iter::from_fn(move || {
            (w != 0).then(|| {
                let t = w.trailing_zeros() as usize;
                w &= w - 1;
                i * 64 + t
            })
        })
    })
}

impl Walk for Bits {
    fn moves(&self, g: &Game, out: &mut Vec<usize>) {
        out.extend(both(self.en, g.move_words));
    }

    fn over(&self, g: &Game) -> bool {
        (0..INLINE).all(|i| self.en[i] & g.move_words[i] == 0)
    }

    fn has(&self, t: usize) -> bool {
        self.en[t / 64] >> (t % 64) & 1 == 1
    }

    fn play(&mut self, g: &Game, t: usize) -> Result<bool> {
        if !g.bits.fire_tracked(t, &mut self.mk, &mut self.en) {
            return Ok(false);
        }
        for _ in 0..SETTLE {
            let Some(u) = both(self.en, g.auto_words).next() else {
                return Ok(true);
            };
            if !g.bits.fire_tracked(u, &mut self.mk, &mut self.en) {
                return Ok(false);
            }
        }
        Err(endless())
    }

    fn score(&self, g: &Game) -> Result<f64> {
        let has = |p: usize| self.mk[p / 64] >> (p % 64) & 1 == 1;
        g.check(
            g.reward
                .iter()
                .map(|&(p, w)| if has(p) { w } else { 0.0 })
                .sum(),
        )
    }

    fn deficit(&self, g: &Game, t: usize) -> u64 {
        match g.bits.deficit(t, &self.mk) {
            u32::MAX => u64::MAX,
            d => d as u64,
        }
    }

    const COUNTS: bool = false;

    fn key(&self) -> memo::Key {
        // The marking itself (the enabled set follows from it); the table
        // keeps it apart from the counts' hashes.
        self.mk
    }
}

const NO_MOVE: u32 = u32::MAX;

/// The move [`Solver::choose`] picks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Choice {
    /// The move.
    pub t: usize,
    /// The position's exact value (+1 side's view), when the search found
    /// it.
    pub value: Option<f64>,
    /// Whether `t` is a best move: always, outside play mode.
    pub exact: bool,
}

/// Alpha-beta with a transposition table that persists across calls (so
/// solving the positions of one game reuses work) and a [`Chain`] of pieces
/// for move order. One per thread.
pub struct Solver<'a> {
    g: &'a Game,
    chain: Chain,
    filters: Filters,
    tt: Table,
    history: Vec<u32>,
    nodes: usize,
    budget: usize,
    /// The bit engine met a marking it cannot hold.
    spilled: bool,
    /// Whether a value is found by yes/no searches (see `solve`).
    null_window: bool,
    root: Root<'a>,
    /// What `root` makes of the position [`Solver::value`] is searching.
    ranked: Option<Ranked>,
    /// A search's time, and when the one running stops.
    time: Option<Duration>,
    deadline: Option<Instant>,
    /// The clock ran out: every search returns `None` until the next call.
    stop: bool,
    /// Whether [`Solver::choose`] guesses when the budget runs out.
    play: bool,
}

impl<'a> Solver<'a> {
    /// The entries the table holds at most, by default: 2^23, 264 MiB at the
    /// 33 bytes an entry of a net of up to 192 places takes (about 400 MiB
    /// while the last doubling moves it). On a Connect-4 position of 29
    /// million nodes, searched without filters, it cost 3.6% more nodes than
    /// an unbounded table; 2^21 entries cost 46%.
    pub const MEMO: usize = 1 << 23;

    /// The table for `g`: keys as wide as a bit marking when the bit engine
    /// applies, else a 128-bit hash; bounds as indices when the rewards are
    /// few.
    fn table(g: &Game, entries: usize) -> Table {
        let fits = g.bits.set_words() <= INLINE && g.bits.words() <= INLINE;
        let kw = if fits { g.bits.words().max(2) } else { 2 };
        Table::new(entries, kw, !g.values.is_empty())
    }

    /// A solver giving up on a value past `budget` nodes.
    pub fn new(g: &'a Game, budget: usize) -> Self {
        Self::with_chain(g, budget, Chain::default()).expect("the default chain needs no goals")
    }

    /// A solver ordering moves by `chain`, refused when a piece of it
    /// needs what the net does not declare (see [`Chain::check`]).
    pub fn with_chain(g: &'a Game, budget: usize, chain: Chain) -> Result<Self> {
        chain.check(g)?;
        Ok(Self {
            g,
            chain,
            filters: Filters::of(&Filter::DEFAULT),
            tt: Self::table(g, Self::MEMO),
            history: vec![0; g.ix.transitions.len()],
            nodes: 0,
            budget,
            spilled: false,
            null_window: true,
            root: Root::default(),
            ranked: None,
            time: None,
            deadline: None,
            stop: false,
            play: false,
        })
    }

    /// Keeps the table to at most `entries` (by default [`Solver::MEMO`]),
    /// inside a search and across calls: past that, a new position takes
    /// the place of a weaker one ([`crate::memo`]), and a position
    /// forgotten is searched again if met again.
    pub fn with_memo(mut self, entries: usize) -> Self {
        self.tt = Self::table(self.g, entries);
        self
    }

    /// Drops moves by `filters` (see [`crate::filter`]) instead of
    /// [`Filter::DEFAULT`].
    pub fn with_filters(mut self, filters: &[Filter]) -> Self {
        self.filters = Filters::of(filters);
        self
    }

    /// Entries the table holds now.
    pub fn memo_len(&self) -> usize {
        self.tt.len()
    }

    /// Bytes one table entry takes, and the most bytes the table grows to.
    pub fn memo_bytes(&self) -> (usize, usize) {
        (self.tt.entry_bytes(), self.tt.most_bytes())
    }

    /// Finds a value by one search over the whole reward range, not a run
    /// of yes/no searches, when `on` is false.
    pub fn with_null_window(mut self, on: bool) -> Self {
        self.null_window = on;
        self
    }

    /// Breaks the order chain's ties among the root's moves by `estimate`,
    /// best guess first (see [`crate::root`]).
    pub fn with_estimate(mut self, estimate: Box<dyn Estimate + 'a>) -> Self {
        self.root.estimate = Some(estimate);
        self
    }

    /// Drops the root's moves that leave the other side a win at once
    /// (`non_losing`, at the root only), when `on`.
    pub fn with_root_non_losing(mut self, on: bool) -> Self {
        self.root.non_losing = on;
        self
    }

    /// Starts from the estimate when `on` (it needs one, see
    /// [`Solver::with_estimate`]): the yes/no search first run is the one
    /// whose threshold is nearest the best guess at the root, and the run
    /// goes on from there; with a single search over the range, a window
    /// an eighth of the range wide around the guess, and the whole range if
    /// the value falls outside it.
    pub fn with_aspiration(mut self, on: bool) -> Self {
        self.root.aspiration = on;
        self
    }

    /// Gives a value up past `time` as well as past the node budget, each
    /// call to [`Solver::value`] or [`Solver::choose`] timed on its own. The
    /// clock is read every 1,024 nodes, so a search can run over by that
    /// much. It needs `std::time::Instant` (not on `wasm32-unknown-unknown`).
    pub fn with_time(mut self, time: Duration) -> Self {
        self.time = Some(time);
        self
    }

    /// Lets [`Solver::choose`] return a move it has not proven best when
    /// the budget runs out, when `on` (play mode).
    pub fn with_play(mut self, on: bool) -> Self {
        self.play = on;
        self
    }

    /// Pins `book`'s positions in the table, exact (see [`crate::book`]).
    /// A book larger than the table keeps what fits.
    pub fn with_book(mut self, book: &Book) -> Self {
        for (mk, v, best) in &book.entries {
            let (k, counts) = self.key_at(mk);
            let best = best.map_or(NO_MOVE, |t| t as u32);
            self.tt.pin(&k, counts, *v, best);
        }
        self
    }

    /// The table's key for settled marking `mk`, and whether it is a
    /// counts key: as the search keys it, by bits when it fits.
    fn key_at(&self, mk: &[i64]) -> (memo::Key, bool) {
        let g = self.g;
        let fits = g.bits.set_words() <= INLINE;
        match g.bits.marking(mk).filter(|b| fits && b.len() <= INLINE) {
            Some(b) => {
                let mut k = [0; INLINE];
                k[..b.len()].copy_from_slice(&b);
                (k, false)
            }
            None => {
                let h = key_of(mk.iter().map(|&v| v as u64));
                ([(h >> 64) as u64, h as u64, 0, 0], true)
            }
        }
    }

    /// The moves at settled position `mk` that the root's pieces keep, best
    /// first (all of them in the net's order, when none runs). An estimate
    /// is solved once per move.
    pub fn root_order(&mut self, mk: &[i64]) -> Result<Vec<usize>> {
        Ok(match self.rank(mk)? {
            Some(r) => r.order,
            None => self.g.moves(mk),
        })
    }

    /// The root's pieces at `mk`, or `None` when none runs or there is no
    /// move.
    fn rank(&mut self, mk: &[i64]) -> Result<Option<Ranked>> {
        let g = self.g;
        let moves = g.moves(mk);
        if !self.root.any() || moves.is_empty() {
            return Ok(None);
        }
        let s = g.mover(&moves)?;
        let mut kept: Vec<(f64, usize)> = vec![];
        for &t in &moves {
            let next = g.play(mk, t)?;
            if self.root.non_losing {
                let w = Counts {
                    en: g.ix.enabled_set(&next),
                    mk: next.clone(),
                };
                if self.filters.lost(g, s, &w)? == Some(true) {
                    continue;
                }
            }
            let guess = match &mut self.root.estimate {
                Some(_) if g.moves(&next).is_empty() => g.score(&next)?,
                Some(e) => e.estimate(g, &next)?.clamp(g.bounds.0, g.bounds.1),
                None => 0.0,
            };
            kept.push((s * guess, t));
        }
        // Stable: ties keep the net's order.
        kept.sort_by(|x, y| y.0.total_cmp(&x.0));
        let mut rank = vec![0; g.ix.transitions.len()];
        for (i, &(_, t)) in kept.iter().enumerate() {
            rank[t] = (kept.len() - i) as u64;
        }
        Ok(Some(Ranked {
            guess: kept
                .first()
                .filter(|_| self.root.estimate.is_some())
                .map(|x| s * x.0),
            order: kept.iter().map(|x| x.1).collect(),
            rank,
        }))
    }

    /// The nodes the last [`Solver::value`] or [`Solver::choose`] searched.
    pub fn nodes(&self) -> usize {
        self.nodes
    }

    /// The exact value (+1 side's view) of a settled position with both
    /// sides playing perfectly, or `None` past `budget` nodes.
    pub fn value(&mut self, mk: &[i64]) -> Result<Option<f64>> {
        self.start();
        let ranked = self.rank(mk)?;
        self.run(mk, ranked)
    }

    /// A move at settled position `mk`: a best one and the position's
    /// value, found within the budget (nodes and time, for the whole
    /// call), or `None` when there is no move or the budget runs out first.
    /// In play mode ([`Solver::with_play`]) a budget that runs out gives a
    /// guess instead: the table's best move at `mk` if it has one, else the
    /// first the root's pieces rank ([`Solver::root_order`]).
    ///
    /// The value is found first, then a move worth it: the table's best,
    /// then the moves in the root's order, each valued with the table the
    /// first search filled.
    pub fn choose(&mut self, mk: &[i64]) -> Result<Option<Choice>> {
        let g = self.g;
        self.start();
        let moves = g.moves(mk);
        if moves.is_empty() {
            self.nodes = 0;
            return Ok(None);
        }
        let s = g.mover(&moves)?;
        let ranked = self.rank(mk)?;
        let order = ranked.as_ref().map_or(moves.clone(), |r| r.order.clone());
        if order.is_empty() {
            // Every move leaves the other side a win: any is as good.
            self.nodes = 1;
            return Ok(Some(Choice {
                t: moves[0],
                value: Some(self.filters.bound(g, -s)),
                exact: true,
            }));
        }
        let limit = self.budget;
        let v = self.run(mk, ranked)?;
        let mut total = self.nodes;
        let (k, counts) = self.key_at(mk);
        let known = self
            .tt
            .get(&k, counts)
            .map(|e| e.best as usize)
            .filter(|t| order.contains(t));
        let mut found = None;
        if let Some(v) = v {
            for t in known.into_iter().chain(order.iter().copied()) {
                let next = g.play(mk, t)?;
                let x = if g.moves(&next).is_empty() {
                    Some(g.score(&next)?)
                } else {
                    self.budget = limit.saturating_sub(total);
                    let x = self.run(&next, None)?;
                    total += self.nodes;
                    x
                };
                match x {
                    Some(x) if x == v => {
                        found = Some(t);
                        break;
                    }
                    Some(_) => {}
                    None => break,
                }
            }
        }
        self.budget = limit;
        self.nodes = total;
        Ok(match found {
            Some(t) => Some(Choice {
                t,
                value: v,
                exact: true,
            }),
            None if self.play => Some(Choice {
                t: known.unwrap_or(order[0]),
                value: v,
                exact: false,
            }),
            None => None,
        })
    }

    /// A call's start: the table ages, the clock starts.
    fn start(&mut self) {
        self.tt.age();
        self.stop = false;
        self.deadline = self.time.map(|d| Instant::now() + d);
    }

    /// The value at `mk`, with `ranked` the root's pieces there (or none).
    fn run(&mut self, mk: &[i64], ranked: Option<Ranked>) -> Result<Option<f64>> {
        let g = self.g;
        self.ranked = ranked;
        if self.ranked.as_ref().is_some_and(|r| r.order.is_empty()) {
            // `non_losing` dropped every move: each leaves the other side a
            // win.
            self.nodes = 1;
            let s = g.mover(&g.moves(mk))?;
            return Ok(Some(self.filters.bound(g, -s)));
        }
        let guess = self
            .ranked
            .as_ref()
            .and_then(|r| r.guess)
            .filter(|_| self.root.aspiration);
        let fits = g.bits.set_words() <= INLINE;
        if let Some(b) = g.bits.marking(mk).filter(|b| fits && b.len() <= INLINE) {
            let mut w = Bits {
                mk: [0; INLINE],
                en: [0; INLINE],
            };
            w.mk[..b.len()].copy_from_slice(&b);
            g.bits.enabled_words(&w.mk, &mut w.en[..g.bits.set_words()]);
            self.nodes = 0;
            self.spilled = false;
            let got = self.solve(&w, guess)?;
            if !self.spilled {
                return Ok(got);
            }
        }
        self.nodes = 0;
        let en = g.ix.enabled_set(mk);
        self.solve(
            &Counts {
                mk: mk.to_vec(),
                en,
            },
            guess,
        )
    }

    /// The value at `w` as a run of yes/no searches, one per gap between
    /// adjacent rewards, starting from the mover's worst ("does the mover
    /// avoid it?"; on Connect-4 that took a fifth fewer nodes than starting
    /// from the best), or from the gap nearest `guess`, and going on the
    /// way the answers point. With too many rewards to step through, one
    /// search over the whole range, or first over a window around `guess`.
    fn solve<W: Walk>(&mut self, w: &W, guess: Option<f64>) -> Result<Option<f64>> {
        let g = self.g;
        let vals = &g.values;
        let (lo, hi) = g.bounds;
        if vals.len() < 3 || !self.null_window {
            if let Some(e) = guess {
                let d = (hi - lo) / 8.0;
                let (a, b) = ((e - d).max(lo), (e + d).min(hi));
                let Some(v) = self.search(w, a, b, true)? else {
                    return Ok(None);
                };
                if a < v && v < b {
                    return Ok(Some(v));
                }
            }
            return self.search(w, lo, hi, true);
        }
        let mut moves = vec![];
        w.moves(g, &mut moves);
        if moves.is_empty() {
            return w.score(g).map(Some);
        }
        let s = g.mover(&moves)?;
        let gaps = vals.len() - 1;
        let mid = |k: usize| (vals[k] + vals[k + 1]) / 2.0;
        let mut k = match guess {
            Some(e) => (0..gaps)
                .min_by(|&x, &y| (mid(x) - e).abs().total_cmp(&(mid(y) - e).abs()))
                .unwrap(),
            None if s > 0.0 => 0,
            None => gaps - 1,
        };
        // The value is one of vals[i..=j].
        let (mut i, mut j) = (0, gaps);
        loop {
            let (a, b) = (vals[k], vals[k + 1]);
            let Some(v) = self.search(w, a, b, true)? else {
                return Ok(None);
            };
            let up = v > (a + b) / 2.0;
            if up {
                i = k + 1;
            } else {
                j = k;
            }
            if i >= j {
                return Ok(Some(vals[i]));
            }
            k = if up { i } else { j - 1 };
        }
    }

    /// Fail-soft alpha-beta in the +1 side's view. Inside the bounds every
    /// result is a bound on the true value; with the window the whole
    /// range, it is the value. `root` when `w` is the position
    /// [`Solver::value`] was given, where the root's pieces run.
    fn search<W: Walk>(
        &mut self,
        w: &W,
        mut a: f64,
        mut b: f64,
        root: bool,
    ) -> Result<Option<f64>> {
        self.nodes += 1;
        if self.nodes > self.budget || self.stop {
            return Ok(None);
        }
        if self.nodes & 1023 == 0 && self.deadline.is_some_and(|d| Instant::now() >= d) {
            self.stop = true;
            return Ok(None);
        }
        let g = self.g;
        let n0 = self.nodes;
        let k = w.key();
        let mut first = NO_MOVE;
        if let Some(e) = self.tt.get(&k, W::COUNTS) {
            if e.lo == e.hi || e.lo >= b {
                return Ok(Some(e.lo));
            }
            if e.hi <= a {
                return Ok(Some(e.hi));
            }
            a = a.max(e.lo);
            b = b.min(e.hi);
            first = e.best;
        }
        let mut moves = Vec::with_capacity(8);
        w.moves(g, &mut moves);
        if moves.is_empty() {
            let x = w.score(g)?;
            self.store::<W>(&k, x, x, NO_MOVE, 1);
            return Ok(Some(x));
        }
        let s = g.mover(&moves)?;
        let f = self.filters;
        // Children, best first, as the order chain ranks them.
        let cx = Ctx {
            g,
            mover: s,
            first,
            history: &self.history,
        };
        let mut kids = Vec::with_capacity(moves.len());
        for &t in &moves {
            let mut next = w.clone();
            if !next.play(g, t)? {
                self.spilled = true;
                return Ok(None);
            }
            if f.must_win && f.best(g, s, &next)? {
                // The cutoff the winning move would have made.
                self.chain.cutoff(&mut self.history, t);
                let x = f.bound(g, s);
                self.store::<W>(&k, x, x, t as u32, self.nodes - n0);
                return Ok(Some(x));
            }
            kids.push((self.chain.key(&cx, t, &next)?, t, next));
        }
        if f.non_losing {
            let mut keep = Vec::with_capacity(kids.len());
            for kid in kids {
                match f.lost(g, s, &kid.2)? {
                    None => {
                        self.spilled = true;
                        return Ok(None);
                    }
                    Some(true) => {}
                    Some(false) => keep.push(kid),
                }
            }
            if keep.is_empty() {
                let x = f.bound(g, -s);
                self.store::<W>(&k, x, x, NO_MOVE, self.nodes - n0);
                return Ok(Some(x));
            }
            kids = keep;
        }
        match self.ranked.as_ref().filter(|_| root) {
            Some(r) => {
                kids.retain(|x| r.rank[x.1] > 0);
                kids.sort_by(|x, y| {
                    self.chain
                        .before(&x.0, &y.0)
                        .then(r.rank[y.1].cmp(&r.rank[x.1]))
                });
            }
            None => kids.sort_by(|x, y| self.chain.before(&x.0, &y.0)),
        }
        let (a0, b0) = (a, b);
        let mut best = (
            if s > 0.0 {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            },
            NO_MOVE,
        );
        for (_, t, next) in &kids {
            let Some(v) = self.search(next, a, b, false)? else {
                return Ok(None);
            };
            if (s > 0.0 && v > best.0) || (s < 0.0 && v < best.0) {
                best = (v, *t as u32);
            }
            if s > 0.0 {
                a = a.max(v);
            } else {
                b = b.min(v);
            }
            if a >= b {
                self.chain.cutoff(&mut self.history, *t);
                break;
            }
        }
        let v = best.0;
        let (lo, hi) = if v <= a0 {
            (g.bounds.0, v)
        } else if v >= b0 {
            (v, g.bounds.1)
        } else {
            (v, v)
        };
        // Merged with what was known before this search narrowed the window.
        self.store::<W>(&k, lo, hi, best.1, self.nodes - n0);
        Ok(Some(v))
    }

    /// Remembers that the value at `k` lies in `[lo, hi]` (and in what the
    /// table knew of it), `best` doing best, found by `nodes` nodes.
    fn store<W: Walk>(&mut self, k: &memo::Key, lo: f64, hi: f64, best: u32, nodes: usize) {
        self.tt.store(k, W::COUNTS, Entry { lo, hi, best }, nodes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Arc, Model, Place, Transition};
    use std::collections::HashMap;

    fn place(id: &str, initial: i64) -> Place {
        Place {
            id: id.into(),
            initial,
            ..Default::default()
        }
    }

    fn arc(from: &str, to: &str) -> Arc {
        Arc {
            from: from.into(),
            to: to.into(),
            ..Default::default()
        }
    }

    /// Tic-tac-toe in the style of the Connect-4 example: a move takes a
    /// cell and passes the turn; a line's detector reads its three cells,
    /// takes the turn and the game, and marks the winner.
    fn tic_tac_toe() -> (Game, Vec<i64>) {
        let mut places = vec![place("game", 1), place("x_turn", 1), place("o_turn", 0)];
        places.extend([place("win_x", 0), place("win_o", 0)]);
        let (mut ts, mut arcs) = (vec![], vec![]);
        for i in 0..9 {
            places.push(place(&format!("open{i}"), 1));
            for s in ["x", "o"] {
                places.push(place(&format!("{s}{i}"), 0));
            }
        }
        for (s, mine, theirs) in [("x", "x_turn", "o_turn"), ("o", "o_turn", "x_turn")] {
            for i in 0..9 {
                let t = format!("{s}_play{i}");
                arcs.extend([
                    arc(&format!("open{i}"), &t),
                    arc(mine, &t),
                    arc(&t, &format!("{s}{i}")),
                    arc(&t, theirs),
                ]);
                ts.push(t);
            }
        }
        let lines = [
            [0, 1, 2],
            [3, 4, 5],
            [6, 7, 8],
            [0, 3, 6],
            [1, 4, 7],
            [2, 5, 8],
            [0, 4, 8],
            [2, 4, 6],
        ];
        for (s, after) in [("x", "o_turn"), ("o", "x_turn")] {
            for (l, cells) in lines.iter().enumerate() {
                let t = format!("{s}_win{l}");
                for c in cells {
                    let p = format!("{s}{c}");
                    arcs.extend([arc(&p, &t), arc(&t, &p)]);
                }
                arcs.extend([
                    arc(after, &t),
                    arc("game", &t),
                    arc(&t, &format!("win_{s}")),
                ]);
                ts.push(t);
            }
        }
        let m = Model {
            places,
            transitions: ts
                .iter()
                .map(|t| Transition {
                    id: t.clone(),
                    ..Default::default()
                })
                .collect(),
            arcs,
            ..Default::default()
        };
        game_of(&m)
    }

    /// Nim on one heap of 7: take one or two; whoever cannot move loses
    /// (an automatic transition at an empty heap hands the win over). The
    /// heap's counts run past 1, so it is searched by counts.
    fn nim() -> (Game, Vec<i64>) {
        let places = vec![
            place("heap", 7),
            place("x_turn", 1),
            place("o_turn", 0),
            place("win_x", 0),
            place("win_o", 0),
        ];
        let (mut ts, mut arcs) = (vec![], vec![]);
        for (s, mine, theirs) in [("x", "x_turn", "o_turn"), ("o", "o_turn", "x_turn")] {
            for k in [1, 2] {
                let t = format!("{s}_take{k}");
                arcs.extend([
                    Arc {
                        weight: k,
                        ..arc("heap", &t)
                    },
                    arc(mine, &t),
                    arc(&t, theirs),
                ]);
                ts.push(t);
            }
            // The side to move at an empty heap loses.
            let t = format!("{s}_stuck");
            let other = if s == "x" { "win_o" } else { "win_x" };
            arcs.extend([
                Arc {
                    typ: crate::schema::ArcType::Inhibitor,
                    ..arc("heap", &t)
                },
                arc(mine, &t),
                arc(&t, other),
            ]);
            ts.push(t);
        }
        let m = Model {
            places,
            transitions: ts
                .iter()
                .map(|t| Transition {
                    id: t.clone(),
                    ..Default::default()
                })
                .collect(),
            arcs,
            ..Default::default()
        };
        game_of(&m)
    }

    /// The game on `m` (moves by their `x_`/`o_` prefix, detectors
    /// automatic) and its initial marking.
    fn game_of(m: &Model) -> (Game, Vec<i64>) {
        let ix = m.indexed();
        let m0 = ix.dense(&m.initial_marking());
        let auto: Vec<bool> = ix
            .transitions
            .iter()
            .map(|t| t.contains("win") || t.contains("stuck"))
            .collect();
        let sign = ix
            .transitions
            .iter()
            .map(|t| if t.starts_with("x_") { 1.0 } else { -1.0 })
            .collect();
        let at = |id: &str| ix.places.iter().position(|p| p == id).unwrap();
        let reward = vec![(at("win_x"), 1.0), (at("win_o"), -1.0)];
        (Game::new(ix, auto, sign, reward), m0)
    }

    /// Plain minimax, memoized: the reference.
    fn minimax(g: &Game, mk: &[i64], memo: &mut HashMap<Vec<i64>, f64>) -> f64 {
        if let Some(&v) = memo.get(mk) {
            return v;
        }
        let moves = g.moves(mk);
        let v = if moves.is_empty() {
            g.score(mk).unwrap()
        } else {
            let s = g.mover(&moves).unwrap();
            let vs = moves
                .iter()
                .map(|&t| minimax(g, &g.play(mk, t).unwrap(), memo));
            if s > 0.0 {
                vs.fold(f64::NEG_INFINITY, f64::max)
            } else {
                vs.fold(f64::INFINITY, f64::min)
            }
        };
        memo.insert(mk.to_vec(), v);
        v
    }

    /// Every settled position reachable from the initial marking.
    fn positions(g: &Game, m0: Vec<i64>) -> Vec<Vec<i64>> {
        let mut start = m0;
        g.settle(&mut start).unwrap();
        let (mut seen, mut todo) = (std::collections::HashSet::new(), vec![start]);
        while let Some(mk) = todo.pop() {
            if seen.insert(mk.clone()) {
                todo.extend(g.moves(&mk).into_iter().map(|t| g.play(&mk, t).unwrap()));
            }
        }
        seen.into_iter().collect()
    }

    fn check(g: &Game, m0: Vec<i64>, want_bits: bool) {
        let mut memo = HashMap::new();
        let all = positions(g, m0);
        assert_eq!(
            all.iter().all(|mk| g.bits.marking(mk).is_some()),
            want_bits,
            "the engine this game exercises"
        );
        let mut exact = Solver::new(g, usize::MAX);
        let mut cut = 0;
        for mk in &all {
            let want = minimax(g, mk, &mut memo);
            assert_eq!(exact.value(mk).unwrap(), Some(want), "at {mk:?}");
            // Fresh, so the table holds nothing from other positions.
            match Solver::new(g, 3).value(mk).unwrap() {
                Some(v) => assert_eq!(v, want, "a budgeted value is exact"),
                None => cut += 1,
            }
        }
        assert!(cut > 0, "the small budget cuts some searches off");
    }

    /// Every ordering of the goal-reading pieces finds Nim's values by
    /// counts, whose goals are the "stuck" detectors.
    #[test]
    fn goal_orders_find_the_same_values_by_counts() {
        use crate::order::Piece;
        let (g, m0) = nim();
        assert_eq!(g.goals.len(), 2);
        let mut memo = HashMap::new();
        let all = positions(&g, m0);
        let [tt, win, hist, cen, thr] = Piece::NAMES.map(|n| Piece::parse(n).unwrap());
        for pieces in [
            vec![cen],
            vec![thr],
            vec![thr, cen],
            vec![tt, win, thr, cen, hist],
            vec![hist, cen, thr, win, tt],
        ] {
            let chain = Chain::new(&pieces).unwrap();
            for mk in &all {
                let mut s = Solver::with_chain(&g, usize::MAX, chain.clone()).unwrap();
                assert_eq!(s.value(mk).unwrap(), Some(minimax(&g, mk, &mut memo)));
            }
        }
    }

    /// The threats of the mover's moves, by bits and by counts.
    #[test]
    fn threats_count_goals_one_token_short_on_both_engines() {
        use crate::order::{Orderer, Threats};
        let (g, m0) = tic_tac_toe();
        let at = |name: &str| g.ix.transitions.iter().position(|t| t == name).unwrap();
        let mut mk = m0;
        for t in ["x_play0", "o_play3", "x_play1", "o_play4"] {
            mk = g.play(&mk, at(t)).unwrap();
        }
        let cx = Ctx {
            g: &g,
            mover: 1.0,
            first: NO_MOVE,
            history: &[],
        };
        let b = g.bits.marking(&mk).unwrap();
        let mut w = Bits {
            mk: [0; INLINE],
            en: [0; INLINE],
        };
        w.mk[..b.len()].copy_from_slice(&b);
        g.bits.enabled_words(&w.mk, &mut w.en[..g.bits.set_words()]);
        let c = Counts {
            en: g.ix.enabled_set(&mk),
            mk: mk.clone(),
        };
        let key = |name: &str| {
            let t = at(name);
            let (mut wb, mut wc) = (w, c.clone());
            assert!(wb.play(&g, t).unwrap() && wc.play(&g, t).unwrap());
            let by_bits = Threats.key(&cx, t, &wb).unwrap();
            assert_eq!(by_bits, Threats.key(&cx, t, &wc).unwrap());
            by_bits
        };
        // x holds 0 and 1, o holds 3 and 4. x5 touches no line of x that is
        // close. x6 leaves the column 0-3-6 one token short, though o's
        // piece sits on it: a goal's shortfall counts tokens, not which
        // cell is still free.
        assert_eq!(key("x_play5"), 0);
        assert_eq!(key("x_play6"), 1);
    }

    /// A memo keeps the table to its entries inside one search, and values
    /// stay exact.
    #[test]
    fn a_memo_bounds_the_table() {
        let (g, m0) = tic_tac_toe();
        let mut start = m0.clone();
        g.settle(&mut start).unwrap();
        let mut s = Solver::new(&g, usize::MAX).with_memo(50);
        assert_eq!(s.value(&start).unwrap(), Some(0.0));
        assert!(s.memo_len() <= 50, "{} entries", s.memo_len());
        let mut full = Solver::new(&g, usize::MAX);
        full.value(&start).unwrap();
        assert!(full.memo_len() > 50, "the default table is larger");
        assert!(s.nodes() > full.nodes(), "and remembering less costs nodes");
    }

    /// Every set of filters finds minimax's value at every position, by
    /// bits and by counts, and settles some positions in fewer nodes.
    #[test]
    fn filters_find_the_same_values_on_both_engines() {
        for (g, m0) in [tic_tac_toe(), nim()] {
            let mut memo = HashMap::new();
            let all = positions(&g, m0);
            let mut plain = 0;
            for mk in &all {
                let mut s = Solver::new(&g, usize::MAX).with_filters(&[]);
                s.value(mk).unwrap();
                plain += s.nodes();
            }
            for filters in [
                vec![Filter::MustWin],
                vec![Filter::NonLosing],
                vec![Filter::NonLosing, Filter::MustWin],
            ] {
                let mut nodes = 0;
                for mk in &all {
                    let mut s = Solver::new(&g, usize::MAX).with_filters(&filters);
                    let want = minimax(&g, mk, &mut memo);
                    assert_eq!(s.value(mk).unwrap(), Some(want), "{filters:?} at {mk:?}");
                    nodes += s.nodes();
                }
                assert!(nodes < plain, "{filters:?}: {nodes} nodes, {plain} without");
            }
        }
    }

    /// A guess that ignores the position, or reads noise from it.
    struct Guess(Option<f64>);

    impl Estimate for Guess {
        fn name(&self) -> &'static str {
            "guess"
        }

        fn estimate(&mut self, _: &Game, mk: &[i64]) -> Result<f64> {
            Ok(self.0.unwrap_or_else(|| {
                let h = key_of(mk.iter().map(|&v| v as u64));
                (h % 1000) as f64 / 250.0 - 2.0
            }))
        }
    }

    /// The root's pieces, with guesses wrong on purpose (outside the
    /// bounds, fixed, noise), change no value, by yes/no searches or by one
    /// over the range.
    #[test]
    fn root_pieces_keep_every_value() {
        for (g, m0) in [tic_tac_toe(), nim()] {
            let mut memo = HashMap::new();
            let all = positions(&g, m0);
            for guess in [Some(-7.0), Some(0.37), Some(g.bounds.1), None] {
                for (null_window, non_losing) in [(true, false), (true, true), (false, true)] {
                    let mut s = Solver::new(&g, usize::MAX)
                        .with_null_window(null_window)
                        .with_estimate(Box::new(Guess(guess)))
                        .with_root_non_losing(non_losing)
                        .with_aspiration(true);
                    for mk in &all {
                        let want = minimax(&g, mk, &mut memo);
                        let got = s.value(mk).unwrap();
                        assert_eq!(got, Some(want), "{guess:?} {null_window} at {mk:?}");
                        let order = s.root_order(mk).unwrap();
                        let mut moves = g.moves(mk);
                        if !non_losing {
                            assert_eq!(order.len(), moves.len());
                        }
                        moves.retain(|t| order.contains(t));
                        assert_eq!(moves.len(), order.len(), "{order:?} not among the moves");
                    }
                }
            }
        }
    }

    /// `choose` gives a best move and the value at every position, with
    /// the root's pieces or without.
    #[test]
    fn choose_finds_a_best_move() {
        for (g, m0) in [tic_tac_toe(), nim()] {
            let mut memo = HashMap::new();
            for non_losing in [false, true] {
                let mut s = Solver::new(&g, usize::MAX).with_root_non_losing(non_losing);
                for mk in positions(&g, m0.clone()) {
                    let want = minimax(&g, &mk, &mut memo);
                    let Some(c) = s.choose(&mk).unwrap() else {
                        assert!(g.moves(&mk).is_empty(), "no move chosen at {mk:?}");
                        continue;
                    };
                    assert!(c.exact && c.value == Some(want), "{c:?} at {mk:?}");
                    let next = g.play(&mk, c.t).unwrap();
                    assert_eq!(minimax(&g, &next, &mut memo), want, "{c:?} at {mk:?}");
                }
            }
        }
    }

    /// Past its budget, `choose` gives up in exact mode and guesses in play
    /// mode; the clock stops a search as the node count does.
    #[test]
    fn a_budget_that_runs_out_gives_a_guess_only_in_play_mode() {
        let (g, m0) = tic_tac_toe();
        let mut exact = Solver::new(&g, 20);
        assert_eq!(exact.choose(&m0).unwrap(), None);
        let mut play = Solver::new(&g, 20).with_play(true);
        let c = play.choose(&m0).unwrap().expect("a guess");
        assert!(!c.exact && g.moves(&m0).contains(&c.t), "{c:?}");
        let mut all = Solver::new(&g, usize::MAX);
        all.value(&m0).unwrap();
        assert!(all.nodes() > 1024, "the start takes {} nodes", all.nodes());
        let mut timed = Solver::new(&g, usize::MAX).with_time(Duration::ZERO);
        assert_eq!(timed.value(&m0).unwrap(), None);
        assert_eq!(timed.nodes(), 1024);
        let mut timed = timed.with_play(true);
        assert!(!timed.choose(&m0).unwrap().unwrap().exact);
    }

    /// A book of exact values changes no value and saves the search what
    /// it holds, even when the table is too small to keep anything else.
    #[test]
    fn a_book_keeps_every_value() {
        let (g, m0) = tic_tac_toe();
        let mut memo = HashMap::new();
        let all = positions(&g, m0.clone());
        let mut book = Book::new();
        for mk in all.iter().step_by(3) {
            book.add(mk.clone(), minimax(&g, mk, &mut memo), None);
        }
        let (mut plain, mut booked) = (0, 0);
        for memo_entries in [Solver::MEMO, 64] {
            let mut s = Solver::new(&g, usize::MAX)
                .with_memo(memo_entries)
                .with_book(&book);
            for mk in &all {
                let want = minimax(&g, mk, &mut memo);
                assert_eq!(s.value(mk).unwrap(), Some(want), "at {mk:?}");
                let c = s.choose(mk).unwrap();
                assert_eq!(c.and_then(|c| c.value), g.moves(mk).first().map(|_| want));
            }
            let mut p = Solver::new(&g, usize::MAX);
            p.value(&m0).unwrap();
            plain += p.nodes();
            let mut b = Solver::new(&g, usize::MAX).with_book(&book);
            b.value(&m0).unwrap();
            booked += b.nodes();
        }
        assert!(
            booked < plain,
            "{booked} nodes with the book, {plain} without"
        );
    }

    /// A position where every move lets the other side win at once is
    /// worth the loss, found without searching a reply.
    #[test]
    fn non_losing_settles_a_lost_position_at_once() {
        let (g, m0) = tic_tac_toe();
        let at = |name: &str| g.ix.transitions.iter().position(|t| t == name).unwrap();
        let mut mk = m0;
        for t in ["x_play0", "o_play1", "x_play4", "o_play8", "x_play6"] {
            mk = g.play(&mk, at(t)).unwrap();
        }
        // x holds 0, 4 and 6 and threatens 0-3-6 at 3 and 2-4-6 at 2; o, to
        // move, holds 1 and 8, cannot win at once and cannot block both.
        let mut s = Solver::new(&g, usize::MAX).with_filters(&[Filter::NonLosing]);
        assert_eq!(s.value(&mk).unwrap(), Some(1.0));
        let mut plain = Solver::new(&g, usize::MAX).with_filters(&[]);
        assert_eq!(plain.value(&mk).unwrap(), Some(1.0));
        assert!(
            s.nodes() < plain.nodes(),
            "{} vs {}",
            s.nodes(),
            plain.nodes()
        );
        // At the root alone, too: no move is kept, and no search runs.
        let mut root = Solver::new(&g, usize::MAX).with_root_non_losing(true);
        assert!(root.root_order(&mk).unwrap().is_empty());
        assert_eq!(root.value(&mk).unwrap(), Some(1.0));
        assert_eq!(root.nodes(), 1);
    }

    #[test]
    fn tic_tac_toe_matches_minimax_by_bits() {
        let (g, m0) = tic_tac_toe();
        check(&g, m0, true);
    }

    #[test]
    fn nim_matches_minimax_by_counts() {
        let (g, m0) = nim();
        check(&g, m0, false);
    }
}
