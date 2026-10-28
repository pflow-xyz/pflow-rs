//! Shared by the search tests and the search bench: the example nets in
//! `tests/fixtures` (byte-identical copies of pflow-native's `examples/`) read
//! as two-sided games, and positions sampled from them.
//!
//! The fixtures name their parts by convention (`x_`/`o_` moves, `*win*`
//! detectors, `win_x`/`win_o` rewards). That is this file's declaration of
//! sides, automatic transitions and rewards for these three nets, and the only
//! place a name is read: the library takes the declaration, never the names.

#![allow(dead_code)]

use pflow_metamodel::game::{Game, Solver};
use pflow_metamodel::order::Chain;
use pflow_metamodel::Filter;

pub const NETS: [&str; 3] = ["tic-tac-toe", "connect-3", "connect-4"];

/// The game on a fixture net, and its settled initial marking.
pub fn game(name: &str) -> (Game, Vec<i64>) {
    // By the crate's sibling path, so pflow-solver's tests read it too.
    let path = format!(
        "{}/../pflow-metamodel/tests/fixtures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let (model, _) = pflow_parser::model_from_json(&std::fs::read(path).unwrap()).unwrap();
    let ix = model.indexed();
    let m0 = ix.dense(&model.initial_marking());
    let auto = ix.transitions.iter().map(|t| t.contains("win")).collect();
    let sign = ix
        .transitions
        .iter()
        .map(|t| if t.starts_with("x_") { 1.0 } else { -1.0 })
        .collect();
    let at = |id: &str| ix.places.iter().position(|p| p == id).unwrap();
    let reward = vec![(at("win_x"), 1.0), (at("win_o"), -1.0)];
    let g = Game::new(ix, auto, sign, reward);
    let mut m0 = m0;
    g.settle(&mut m0).unwrap();
    (g, m0)
}

pub struct Lcg(pub u64);

impl Lcg {
    pub fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize % n
    }
}

/// Every settled position reachable from `m0`, in a fixed order.
pub fn reachable(g: &Game, m0: &[i64]) -> Vec<Vec<i64>> {
    let (mut seen, mut todo) = (std::collections::HashSet::new(), vec![m0.to_vec()]);
    while let Some(mk) = todo.pop() {
        if seen.insert(mk.clone()) {
            todo.extend(g.moves(&mk).into_iter().map(|t| g.play(&mk, t).unwrap()));
        }
    }
    let mut all: Vec<_> = seen.into_iter().collect();
    all.sort();
    all
}

/// `count` positions from seeded random play: game `i` plays on until it has
/// made `plies[i % plies.len()]` moves (or ends), and its last position that
/// still has a move is kept.
pub fn sample(g: &Game, m0: &[i64], seed: u64, count: usize, plies: &[usize]) -> Vec<Vec<i64>> {
    let mut r = Lcg(seed);
    (0..count)
        .map(|i| {
            let mut mk = m0.to_vec();
            for _ in 0..plies[i % plies.len()] {
                let moves = g.moves(&mk);
                let next = g.play(&mk, moves[r.below(moves.len())]).unwrap();
                if g.moves(&next).is_empty() {
                    break;
                }
                mk = next;
            }
            mk
        })
        .collect()
}

/// The positions a net is held to: every reachable one on tic-tac-toe, a
/// fixed sample on the Connect games (small enough to solve each in turn).
pub fn positions(name: &str, g: &Game, m0: &[i64]) -> Vec<Vec<i64>> {
    match name {
        "tic-tac-toe" => reachable(g, m0),
        "connect-3" => sample(g, m0, 3, 40, &[0, 4, 8, 10, 12]),
        _ => {
            let mut all = sample(g, m0, 4, 40, &CONNECT4_PLIES);
            all.extend(sample(g, m0, 4, 10, &CONNECT4_HARD_PLIES));
            all
        }
    }
}

/// How deep the Connect-4 sample plays: late enough to solve in a moment.
pub const CONNECT4_PLIES: [usize; 4] = [20, 22, 24, 26];

/// Earlier positions, a few hundred thousand nodes each: where move order
/// shows.
pub const CONNECT4_HARD_PLIES: [usize; 2] = [16, 17];

/// The most nodes any position of the samples may take: they solve in a few
/// thousand, so reaching this is a bug, and it keeps the table small.
pub const BUDGET: usize = 3_000_000;

/// (value, nodes) of each position, each solved by a fresh solver.
pub fn fresh(g: &Game, all: &[Vec<i64>]) -> Vec<(f64, usize)> {
    fresh_with(g, all, &Chain::default())
}

/// [`fresh`], ordering moves by `chain`.
pub fn fresh_with(g: &Game, all: &[Vec<i64>], chain: &Chain) -> Vec<(f64, usize)> {
    fresh_filtered(g, all, chain, &Filter::DEFAULT)
}

/// [`fresh_with`], dropping moves by `filters` rather than the default's.
pub fn fresh_filtered(
    g: &Game,
    all: &[Vec<i64>],
    chain: &Chain,
    filters: &[Filter],
) -> Vec<(f64, usize)> {
    all.iter()
        .map(|mk| {
            let mut s = Solver::with_chain(g, BUDGET, chain.clone())
                .unwrap()
                .with_filters(filters);
            let v = s.value(mk).unwrap().expect("within the node budget");
            (v, s.nodes())
        })
        .collect()
}

/// One line per position, `net index value nodes`: what the order gate holds
/// every solver variant to.
pub fn gate_lines() -> Vec<String> {
    let mut lines = vec![];
    for name in NETS {
        let (g, m0) = game(name);
        let all = positions(name, &g, &m0);
        for (i, (v, n)) in fresh(&g, &all).iter().enumerate() {
            lines.push(format!("{name} {i} {v} {n}"));
        }
    }
    lines
}

/// (value, nodes) of each position, in turn, by one solver: its table and
/// history carry over.
pub fn shared(g: &Game, all: &[Vec<i64>]) -> Vec<(f64, usize)> {
    let mut s = Solver::new(g, BUDGET);
    all.iter()
        .map(|mk| {
            let v = s.value(mk).unwrap().unwrap();
            (v, s.nodes())
        })
        .collect()
}

/// A net whose automatic transitions pay nothing: two moves, a reward place
/// no transition fills. It declares no goal.
pub fn goalless() -> Game {
    use pflow_metamodel::{Arc, Model, Place, Transition};
    let place = |id: &str, initial| Place {
        id: id.into(),
        initial,
        ..Default::default()
    };
    let arc = |from: &str, to: &str| Arc {
        from: from.into(),
        to: to.into(),
        ..Default::default()
    };
    let m = Model {
        places: vec![place("a", 1), place("b", 0), place("won", 0)],
        transitions: ["x_go", "o_go"]
            .map(|t| Transition {
                id: t.into(),
                ..Default::default()
            })
            .to_vec(),
        arcs: vec![arc("a", "x_go"), arc("x_go", "b"), arc("b", "o_go")],
        ..Default::default()
    };
    let ix = m.indexed();
    let won = ix.places.iter().position(|p| p == "won").unwrap();
    Game::new(ix, vec![false; 2], vec![1.0, -1.0], vec![(won, 1.0)])
}

/// A net with many distinct rewards: `weights.len()` prizes, taken in turn
/// (x first), each worth its weight to whoever takes it, so the final
/// reward is the difference of what the sides took. The game ends when no
/// prize is left; the rewards are the sums of some of `weights` and of
/// their negatives.
pub fn prizes(weights: &[i64]) -> (Game, Vec<i64>) {
    use pflow_metamodel::{Arc, Model, Place, Transition};
    let place = |id: &str, initial| Place {
        id: id.into(),
        initial,
        ..Default::default()
    };
    let arc = |from: &str, to: &str| Arc {
        from: from.into(),
        to: to.into(),
        ..Default::default()
    };
    let mut places = vec![place("x_turn", 1), place("o_turn", 0)];
    let (mut ts, mut arcs) = (vec![], vec![]);
    for (i, _) in weights.iter().enumerate() {
        places.push(place(&format!("prize_{i}"), 1));
        for (s, mine, theirs) in [("x", "x_turn", "o_turn"), ("o", "o_turn", "x_turn")] {
            let (t, got) = (format!("{s}_take_{i}"), format!("{s}_got_{i}"));
            places.push(place(&got, 0));
            arcs.extend([
                arc(&format!("prize_{i}"), &t),
                arc(mine, &t),
                arc(&t, theirs),
                arc(&t, &got),
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
    let ix = m.indexed();
    let m0 = ix.dense(&m.initial_marking());
    let at = |id: &str| ix.places.iter().position(|p| p == id).unwrap();
    let mut reward = vec![];
    for (i, &w) in weights.iter().enumerate() {
        reward.push((at(&format!("x_got_{i}")), w as f64));
        reward.push((at(&format!("o_got_{i}")), -(w as f64)));
    }
    let sign = ix
        .transitions
        .iter()
        .map(|t| if t.starts_with("x_") { 1.0 } else { -1.0 })
        .collect();
    let g = Game::new(ix, vec![false; ts.len()], sign, reward);
    (g, m0)
}
