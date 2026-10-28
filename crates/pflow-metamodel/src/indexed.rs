//! The firing rule of [`crate::firing`], resolved to indices once.
//!
//! [`Model::enabled`] and [`Model::fire`] look a transition's arcs up by
//! scanning every arc, and keep the marking in a map keyed by place id.
//! That is the reference, and the right call for one firing. A search that
//! fires millions of times (a game solver, a reachability walk) pays for
//! the scans on every step. [`IndexedModel`] does the lookups once: each
//! transition keeps its arcs as (place index, weight), and a marking is a
//! dense `[i64]` in [`IndexedModel::places`] order.
//!
//! It applies the same four rules, arc by arc, as `firing.rs`: a consuming
//! arc needs at least `weight` tokens (each arc on its own, as there); a
//! read arc needs at least `weight` and consumes none; an inhibitor arc
//! blocks at `weight` or more; a capacity bounds the place after firing,
//! net of what the same firing consumes from it. A negative weight makes a
//! transition never enabled. `tests::agrees_with_the_reference_rule` checks
//! all of it against [`Model::enabled`] and [`Model::fire`] on random nets.
//!
//! A search that walks from marking to marking need not recheck every
//! transition after each firing. [`Enabled`] keeps, for each transition,
//! how many of its conditions (an arc's threshold, an inhibitor, a
//! capacity) the marking fails, and the set of those failing none. Firing
//! `t` changes only the places on its arcs, so
//! [`IndexedModel::fire_tracked`] re-evaluates just the conditions on those
//! places. A place every move touches (a turn token) costs one comparison
//! per arc on it rather than a recheck of every transition with an arc
//! there. `tests::tracked_sets_match_a_full_recheck` compares it with
//! [`IndexedModel::enabled_set`], counts included, after every firing of
//! long random walks, and [`IndexedModel::unfire_tracked`] with the state
//! before it.
//!
//! For nets whose markings stay 0/1, [`crate::BitModel`] is the same rule
//! over bits.

use std::collections::HashMap;

use crate::firing::Marking;
use crate::schema::{ArcType, Model};

/// One transition's arcs, by place index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Arcs {
    pub(crate) inputs: Vec<(usize, i64)>,
    pub(crate) outputs: Vec<(usize, i64)>,
    pub(crate) reads: Vec<(usize, i64)>,
    pub(crate) inhibitors: Vec<(usize, i64)>,
    /// (place, net increase, capacity) for each capacity a firing raises.
    pub(crate) capacities: Vec<(usize, i64, i64)>,
    /// A negative weight on any of its arcs: never enabled.
    pub(crate) invalid: bool,
}

/// A [`Model`]'s firing rule over dense markings. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedModel {
    /// Token place ids, in declaration order (the first of any repeated
    /// id): the order of a dense marking.
    pub places: Vec<String>,
    /// Transition ids, in declaration order: what a transition index means.
    pub transitions: Vec<String>,
    pub(crate) arcs: Vec<Arcs>,
    /// Per place: (transition, condition) for every condition on it.
    conds: Vec<Vec<(usize, Cond)>>,
    /// Per transition: the places its firing changes (its arcs' places).
    touched: Vec<Vec<usize>>,
}

/// One condition a transition's enablement puts on one place's count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cond {
    /// A consuming or read arc: at least this many.
    AtLeast(i64),
    /// An inhibitor arc: fewer than this many.
    Below(i64),
    /// A capacity: the count plus this increase stays within this bound.
    Capped(i64, i64),
}

impl Cond {
    fn holds(self, v: i64) -> bool {
        match self {
            Cond::AtLeast(w) => v >= w,
            Cond::Below(w) => v < w,
            Cond::Capped(d, cap) => v.saturating_add(d) <= cap,
        }
    }
}

/// A set of transitions, by index, as a bitset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransitionSet {
    pub(crate) words: Vec<u64>,
}

impl TransitionSet {
    pub(crate) fn empty(n: usize) -> Self {
        Self {
            words: vec![0; n.div_ceil(64)],
        }
    }

    pub(crate) fn set(&mut self, t: usize, on: bool) {
        let (w, b) = (t / 64, 1u64 << (t % 64));
        if on {
            self.words[w] |= b;
        } else {
            self.words[w] &= !b;
        }
    }

    /// Whether `t` is in the set.
    pub fn contains(&self, t: usize) -> bool {
        self.words[t / 64] >> (t % 64) & 1 == 1
    }

    /// No transition is enabled.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    /// The transitions in the set, in index (declaration) order.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(i, &w)| {
            let mut w = w;
            std::iter::from_fn(move || {
                (w != 0).then(|| {
                    let b = w.trailing_zeros() as usize;
                    w &= w - 1;
                    i * 64 + b
                })
            })
        })
    }

    /// The first transition, in index order, that is in both sets.
    pub fn first_in(&self, other: &TransitionSet) -> Option<usize> {
        self.words
            .iter()
            .zip(&other.words)
            .enumerate()
            .find_map(|(i, (&a, &b))| {
                let w = a & b;
                (w != 0).then(|| i * 64 + w.trailing_zeros() as usize)
            })
    }
}

/// The transitions enabled at a marking, kept up to date as it fires
/// ([`IndexedModel::fire_tracked`]): a [`TransitionSet`] and, per
/// transition, how many of its conditions fail.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Enabled {
    set: TransitionSet,
    failing: Vec<u32>,
}

impl Enabled {
    /// The enabled transitions.
    pub fn set(&self) -> &TransitionSet {
        &self.set
    }

    /// Whether `t` is enabled.
    pub fn contains(&self, t: usize) -> bool {
        self.set.contains(t)
    }

    /// No transition is enabled.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// The enabled transitions, in index (declaration) order.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.set.iter()
    }

    /// The first enabled transition, in index order, that is in `among`.
    pub fn first_in(&self, among: &TransitionSet) -> Option<usize> {
        self.set.first_in(among)
    }
}

impl Model {
    /// The firing rule resolved to indices; see [`IndexedModel`].
    pub fn indexed(&self) -> IndexedModel {
        let mut index: HashMap<&str, usize> = HashMap::new();
        let mut places = vec![];
        for p in self.places.iter().filter(|p| p.is_token()) {
            if !index.contains_key(p.id.as_str()) {
                index.insert(p.id.as_str(), places.len());
                places.push(p.id.clone());
            }
        }
        let place = |id: &str| -> Option<usize> {
            // As `Model::token_place`: the first place of that id, if it
            // holds tokens.
            self.place_by_id(id)
                .filter(|p| p.is_token())
                .and_then(|_| index.get(id).copied())
        };
        let arcs: Vec<Arcs> = self
            .transitions
            .iter()
            .map(|t| {
                let mut a = Arcs::default();
                for arc in &self.arcs {
                    let w = arc.effective_weight();
                    if arc.to == t.id {
                        let Some(p) = place(&arc.from) else { continue };
                        a.invalid |= w < 0;
                        match arc.typ {
                            ArcType::Normal => a.inputs.push((p, w)),
                            ArcType::Inhibitor => a.inhibitors.push((p, w)),
                            ArcType::Read => a.reads.push((p, w)),
                        }
                    } else if arc.from == t.id && !arc.is_read_only() {
                        if let Some(p) = place(&arc.to) {
                            a.invalid |= w < 0;
                            a.outputs.push((p, w));
                        }
                    }
                }
                let mut net: HashMap<usize, i64> = HashMap::new();
                for &(p, w) in &a.inputs {
                    let e = net.entry(p).or_insert(0);
                    *e = e.saturating_sub(w);
                }
                for &(p, w) in &a.outputs {
                    let e = net.entry(p).or_insert(0);
                    *e = e.saturating_add(w);
                }
                for p in &self.places {
                    if p.capacity <= 0 || !p.is_token() {
                        continue;
                    }
                    let Some(&i) = index.get(p.id.as_str()) else {
                        continue;
                    };
                    match net.get(&i) {
                        Some(&d) if d > 0 => a.capacities.push((i, d, p.capacity)),
                        _ => {}
                    }
                }
                a
            })
            .collect();
        let mut conds: Vec<Vec<(usize, Cond)>> = vec![vec![]; places.len()];
        for (u, a) in arcs.iter().enumerate() {
            // An invalid transition is never enabled; its conditions never
            // matter.
            if a.invalid {
                continue;
            }
            for &(p, w) in a.inputs.iter().chain(&a.reads) {
                conds[p].push((u, Cond::AtLeast(w)));
            }
            for &(p, w) in &a.inhibitors {
                conds[p].push((u, Cond::Below(w)));
            }
            for &(p, d, cap) in &a.capacities {
                conds[p].push((u, Cond::Capped(d, cap)));
            }
        }
        let touched = arcs
            .iter()
            .map(|a| {
                let mut ps: Vec<usize> = a.inputs.iter().chain(&a.outputs).map(|x| x.0).collect();
                ps.sort_unstable();
                ps.dedup();
                ps
            })
            .collect();
        IndexedModel {
            places,
            transitions: self.transitions.iter().map(|t| t.id.clone()).collect(),
            arcs,
            conds,
            touched,
        }
    }
}

impl IndexedModel {
    /// A map marking as a dense one (absent places read as zero).
    pub fn dense(&self, mk: &Marking) -> Vec<i64> {
        self.places
            .iter()
            .map(|p| mk.get(p).copied().unwrap_or(0))
            .collect()
    }

    /// A dense marking as a map.
    pub fn sparse(&self, mk: &[i64]) -> Marking {
        self.places
            .iter()
            .cloned()
            .zip(mk.iter().copied())
            .collect()
    }

    /// Whether transition `t` can fire at `mk`.
    pub fn enabled(&self, t: usize, mk: &[i64]) -> bool {
        let a = &self.arcs[t];
        !a.invalid
            && a.inputs.iter().all(|&(p, w)| mk[p] >= w)
            && a.reads.iter().all(|&(p, w)| mk[p] >= w)
            && a.inhibitors.iter().all(|&(p, w)| mk[p] < w)
            && a.capacities
                .iter()
                .all(|&(p, d, cap)| mk[p].saturating_add(d) <= cap)
    }

    /// How many tokens `mk` is short of enabling `t`: what its input and
    /// read arcs lack, what its inhibitors and capacities have too much of.
    /// Zero exactly when `t` is enabled; `u64::MAX` for a transition with a
    /// negative weight, which never is.
    pub fn deficit(&self, t: usize, mk: &[i64]) -> u64 {
        let a = &self.arcs[t];
        if a.invalid {
            return u64::MAX;
        }
        let short = |x: i64| x.max(0) as u64;
        let mut d = 0u64;
        for &(p, w) in a.inputs.iter().chain(&a.reads) {
            d = d.saturating_add(short(w.saturating_sub(mk[p])));
        }
        for &(p, w) in &a.inhibitors {
            d = d.saturating_add(short(mk[p].saturating_sub(w).saturating_add(1)));
        }
        for &(p, add, cap) in &a.capacities {
            d = d.saturating_add(short(mk[p].saturating_add(add).saturating_sub(cap)));
        }
        d
    }

    /// The transitions that can fire at `mk`, in declaration order.
    pub fn enabled_transitions(&self, mk: &[i64]) -> Vec<usize> {
        (0..self.arcs.len())
            .filter(|&t| self.enabled(t, mk))
            .collect()
    }

    /// The transitions that can fire at `mk`, with the counts
    /// [`IndexedModel::fire_tracked`] keeps current.
    pub fn enabled_set(&self, mk: &[i64]) -> Enabled {
        let n = self.arcs.len();
        let mut failing: Vec<u32> = self.arcs.iter().map(|a| a.invalid as u32).collect();
        for (p, cs) in self.conds.iter().enumerate() {
            for &(u, c) in cs {
                failing[u] += !c.holds(mk[p]) as u32;
            }
        }
        let set = self.set_of((0..n).filter(|&t| failing[t] == 0));
        Enabled { set, failing }
    }

    /// A set of transitions, from their indices.
    pub fn set_of(&self, ts: impl IntoIterator<Item = usize>) -> TransitionSet {
        let mut s = TransitionSet::empty(self.arcs.len());
        for t in ts {
            s.set(t, true);
        }
        s
    }

    /// Fires `t` in place and brings `en`, the enabled set at `mk`, up to
    /// date, re-evaluating only the conditions on the places `t` changes.
    /// Like [`Model::fire`], it does not check enablement.
    pub fn fire_tracked(&self, t: usize, mk: &mut [i64], en: &mut Enabled) {
        self.retrack(t, mk, en, |mk| self.fire_in_place(t, mk));
    }

    /// Undoes [`IndexedModel::fire_tracked`] of `t`, restoring `mk` and
    /// `en` as they were, so a search can walk back instead of copying.
    /// Exact unless that firing saturated a count at the `i64` bounds.
    pub fn unfire_tracked(&self, t: usize, mk: &mut [i64], en: &mut Enabled) {
        self.retrack(t, mk, en, |mk| self.unfire_in_place(t, mk));
    }

    /// Applies `change` (which alters only `t`'s places) to `mk`, keeping
    /// `en` current.
    fn retrack(&self, t: usize, mk: &mut [i64], en: &mut Enabled, change: impl FnOnce(&mut [i64])) {
        let touched = &self.touched[t];
        // Take out the conditions' verdicts at the old counts, change, and
        // put in those at the new ones. A transition's last update comes
        // after all of its conditions here are back in, so its bit ends
        // right.
        for &p in touched {
            for &(u, c) in &self.conds[p] {
                en.failing[u] -= !c.holds(mk[p]) as u32;
            }
        }
        change(mk);
        for &p in touched {
            for &(u, c) in &self.conds[p] {
                en.failing[u] += !c.holds(mk[p]) as u32;
                en.set.set(u, en.failing[u] == 0);
            }
        }
    }

    /// Fires `t` in place. Like [`Model::fire`], it does not check
    /// enablement.
    pub fn fire_in_place(&self, t: usize, mk: &mut [i64]) {
        let a = &self.arcs[t];
        for &(p, w) in &a.inputs {
            mk[p] = mk[p].saturating_sub(w);
        }
        for &(p, w) in &a.outputs {
            mk[p] = mk[p].saturating_add(w);
        }
    }

    /// Undoes [`IndexedModel::fire_in_place`] of `t`. Exact unless that
    /// firing saturated a count at the `i64` bounds.
    pub fn unfire_in_place(&self, t: usize, mk: &mut [i64]) {
        let a = &self.arcs[t];
        for &(p, w) in &a.outputs {
            mk[p] = mk[p].saturating_sub(w);
        }
        for &(p, w) in &a.inputs {
            mk[p] = mk[p].saturating_add(w);
        }
    }

    /// The marking after `t` fires, leaving `mk` untouched.
    pub fn fire(&self, t: usize, mk: &[i64]) -> Vec<i64> {
        let mut next = mk.to_vec();
        self.fire_in_place(t, &mut next);
        next
    }

    /// `t`'s consuming arcs, as (place index, weight).
    pub fn inputs(&self, t: usize) -> &[(usize, i64)] {
        &self.arcs[t].inputs
    }

    /// `t`'s output arcs, as (place index, weight).
    pub fn outputs(&self, t: usize) -> &[(usize, i64)] {
        &self.arcs[t].outputs
    }

    /// `t`'s read arcs, as (place index, weight).
    pub fn reads(&self, t: usize) -> &[(usize, i64)] {
        &self.arcs[t].reads
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::schema::{Arc, Place, StateKind, Transition};

    /// A tiny deterministic generator, so the test needs no dependency.
    pub(crate) struct Lcg(pub(crate) u64);
    impl Lcg {
        pub(crate) fn below(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) % n
        }
    }

    /// A random net using every feature the rule has: read and inhibitor
    /// arcs, self-loops, repeated arcs, capacities, a data place, a
    /// repeated place id, negative and zero (defaulted) weights.
    pub(crate) fn random_model(r: &mut Lcg) -> Model {
        let np = 2 + r.below(5) as usize;
        let mut places: Vec<Place> = (0..np)
            .map(|i| Place {
                id: format!("p{i}"),
                capacity: [0, 0, 0, 1, 2, 3][r.below(6) as usize],
                ..Default::default()
            })
            .collect();
        if r.below(3) == 0 {
            places.push(Place {
                id: "d".into(),
                kind: Some(StateKind::Data),
                ..Default::default()
            });
        }
        if r.below(4) == 0 {
            places.push(Place {
                id: "p0".into(),
                capacity: 1,
                ..Default::default()
            });
        }
        let nt = 1 + r.below(4) as usize;
        let transitions: Vec<Transition> = (0..nt)
            .map(|i| Transition {
                id: format!("t{i}"),
                ..Default::default()
            })
            .collect();
        let mut arcs = vec![];
        for _ in 0..r.below(12) {
            let p = if r.below(8) == 0 {
                "d".to_string()
            } else {
                format!("p{}", r.below(np as u64))
            };
            let t = format!("t{}", r.below(nt as u64));
            let weight = [0, 1, 1, 2, 3, -1][r.below(6) as usize];
            let (from, to, typ) = match r.below(5) {
                0 => (p, t, ArcType::Read),
                1 => (p, t, ArcType::Inhibitor),
                2 | 3 => (p, t, ArcType::Normal),
                _ => (t, p, ArcType::Normal),
            };
            arcs.push(Arc {
                from,
                to,
                weight,
                typ,
                ..Default::default()
            });
        }
        Model {
            name: "random".into(),
            places,
            transitions,
            arcs,
            ..Default::default()
        }
    }

    #[test]
    fn agrees_with_the_reference_rule() {
        let mut r = Lcg(7);
        let mut fired = 0;
        for _ in 0..3000 {
            let m = random_model(&mut r);
            let ix = m.indexed();
            for _ in 0..10 {
                let mk: Marking = ix
                    .places
                    .iter()
                    .map(|p| (p.clone(), r.below(4) as i64))
                    .collect();
                let dense = ix.dense(&mk);
                for (t, id) in ix.transitions.iter().enumerate() {
                    let on = m.enabled(id, &mk);
                    assert_eq!(ix.enabled(t, &dense), on, "{id} at {mk:?} in {m:?}");
                    let want = ix.dense(&m.fire(id, &mk));
                    assert_eq!(ix.fire(t, &dense), want, "firing {id} at {mk:?}");
                    fired += on as usize;
                }
                let want: Vec<String> = m.enabled_transitions(&mk);
                let got: Vec<String> = ix
                    .enabled_transitions(&dense)
                    .into_iter()
                    .map(|t| ix.transitions[t].clone())
                    .collect();
                assert_eq!(got, want);
            }
        }
        assert!(
            fired > 10_000,
            "the random nets exercise enabled firings ({fired})"
        );
    }

    #[test]
    fn tracked_sets_match_a_full_recheck_and_undo() {
        let mut r = Lcg(11);
        let mut steps = 0;
        for _ in 0..3000 {
            let m = random_model(&mut r);
            let ix = m.indexed();
            let mut mk: Vec<i64> = (0..ix.places.len()).map(|_| r.below(4) as i64).collect();
            let mut en = ix.enabled_set(&mk);
            let listed: Vec<usize> = en.iter().collect();
            assert_eq!(listed, ix.enabled_transitions(&mk));
            for _ in 0..30 {
                // Mostly enabled moves, sometimes any transition (fire does
                // not check), so the walk also leaves the reachable set.
                let on: Vec<usize> = en.iter().collect();
                let t = if !on.is_empty() && r.below(4) != 0 {
                    on[r.below(on.len() as u64) as usize]
                } else if ix.transitions.is_empty() {
                    break;
                } else {
                    r.below(ix.transitions.len() as u64) as usize
                };
                let (mk0, en0) = (mk.clone(), en.clone());
                ix.fire_tracked(t, &mut mk, &mut en);
                assert_eq!(en, ix.enabled_set(&mk), "after {t} at {mk:?} in {m:?}");
                let (mut back, mut back_en) = (mk.clone(), en.clone());
                ix.unfire_tracked(t, &mut back, &mut back_en);
                assert_eq!((back, back_en), (mk0, en0), "undoing {t} in {m:?}");
                steps += 1;
            }
        }
        assert!(steps > 50_000, "the walks fire enough ({steps})");
    }

    #[test]
    fn transition_sets_iterate_in_order_across_words() {
        let ix = Model {
            transitions: (0..130)
                .map(|i| Transition {
                    id: format!("t{i}"),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
        .indexed();
        let s: TransitionSet = ix.set_of([129, 0, 64, 63, 65]);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![0, 63, 64, 65, 129]);
        assert!(s.contains(64) && !s.contains(62));
        assert_eq!(s.first_in(&ix.set_of([65, 129])), Some(65));
        assert_eq!(s.first_in(&ix.set_of([1])), None);
        assert!(ix.set_of([]).is_empty() && !s.is_empty());
    }

    #[test]
    fn dense_and_sparse_round_trip() {
        let m = Model {
            places: vec![
                Place {
                    id: "a".into(),
                    initial: 2,
                    ..Default::default()
                },
                Place {
                    id: "b".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let ix = m.indexed();
        let mk = m.initial_marking();
        assert_eq!(ix.dense(&mk), vec![2, 0]);
        assert_eq!(ix.sparse(&ix.dense(&mk)), mk);
    }
}
