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

use std::collections::HashMap;

use crate::firing::Marking;
use crate::schema::{ArcType, Model};

/// One transition's arcs, by place index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Arcs {
    inputs: Vec<(usize, i64)>,
    outputs: Vec<(usize, i64)>,
    reads: Vec<(usize, i64)>,
    inhibitors: Vec<(usize, i64)>,
    /// (place, net increase, capacity) for each capacity a firing raises.
    capacities: Vec<(usize, i64, i64)>,
    /// A negative weight on any of its arcs: never enabled.
    invalid: bool,
}

/// A [`Model`]'s firing rule over dense markings. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedModel {
    /// Token place ids, in declaration order (the first of any repeated
    /// id): the order of a dense marking.
    pub places: Vec<String>,
    /// Transition ids, in declaration order: what a transition index means.
    pub transitions: Vec<String>,
    arcs: Vec<Arcs>,
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
        let arcs = self
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
        IndexedModel {
            places,
            transitions: self.transitions.iter().map(|t| t.id.clone()).collect(),
            arcs,
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

    /// The transitions that can fire at `mk`, in declaration order.
    pub fn enabled_transitions(&self, mk: &[i64]) -> Vec<usize> {
        (0..self.arcs.len())
            .filter(|&t| self.enabled(t, mk))
            .collect()
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

    /// The marking after `t` fires, leaving `mk` untouched.
    pub fn fire(&self, t: usize, mk: &[i64]) -> Vec<i64> {
        let mut next = mk.to_vec();
        self.fire_in_place(t, &mut next);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Arc, Place, StateKind, Transition};

    /// A tiny deterministic generator, so the test needs no dependency.
    struct Lcg(u64);
    impl Lcg {
        fn below(&mut self, n: u64) -> u64 {
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
    fn random_model(r: &mut Lcg) -> Model {
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
