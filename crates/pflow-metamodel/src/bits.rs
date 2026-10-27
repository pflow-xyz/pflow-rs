//! The firing rule of [`IndexedModel`] over markings whose every count is
//! 0 or 1, held as bits.
//!
//! Many nets never put two tokens on a place: a board's cells, a turn
//! token, a workflow's states. A marking of such a net is a bitset, one
//! bit per place, and the firing rule becomes mask arithmetic. Each
//! transition is resolved, per 64-place word, to the bits its conditions
//! need set (`one`) out of those they look at (`care`), and to the bits a
//! firing flips (`flip`) with the values they must have beforehand
//! (`from`). Enabled is `mk & care == one`; firing is `mk ^= flip`, and
//! firing again undoes it, so a search walks back without copying.
//!
//! The conditions translate as follows. At 0 or 1 tokens, a consuming or
//! read arc of weight 1 needs the bit set, and one of weight 2 or more
//! never holds; an inhibitor of weight 1 needs it clear, and one of 2 or
//! more always holds; a capacity holds at both counts, at 0 only, or at
//! neither. A transition no 0/1 marking enables is never enabled here.
//!
//! Nothing here assumes the net keeps its markings 0/1. [`BitModel::fire`]
//! refuses a firing whose result would leave a place outside 0 and 1 (a
//! second token on a cell, a repeated input arc taking a place below
//! zero), leaving the marking as it was, so the caller can carry on with
//! [`IndexedModel`]. `tests::agrees_with_the_indexed_rule` holds it to
//! [`IndexedModel`] on random nets, at every 0/1 marking a random walk
//! reaches.

use crate::indexed::{IndexedModel, TransitionSet};

/// One transition's masks over one word of a bit marking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Word {
    at: usize,
    /// The bits its conditions look at, and the values they need there.
    care: u64,
    one: u64,
    /// The bits a firing flips, and the values they must have before it
    /// for the result to stay 0/1.
    flip: u64,
    from: u64,
}

/// An [`IndexedModel`]'s firing rule over 0/1 markings. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitModel {
    places: usize,
    words: usize,
    /// Per transition: its range in `masks`.
    spans: Vec<(usize, usize)>,
    masks: Vec<Word>,
    /// Per transition: no 0/1 marking enables it.
    never: Vec<bool>,
    /// Per transition: no firing of it leaves a 0/1 marking.
    spoils: Vec<bool>,
    /// Per transition: those it can enable, in index order.
    wakes: Vec<Vec<usize>>,
}

impl IndexedModel {
    /// The firing rule over 0/1 markings; see [`BitModel`].
    pub fn bits(&self) -> BitModel {
        let words = self.places.len().div_ceil(64);
        let (mut spans, mut masks, mut never, mut spoils) = (vec![], vec![], vec![], vec![]);
        for a in &self.arcs {
            let (mut one, mut zero) = (vec![0u64; words], vec![0u64; words]);
            let mut no = a.invalid;
            let mut need = |p: usize, holds_at: [bool; 2]| {
                let (w, b) = (p / 64, 1u64 << (p % 64));
                match holds_at {
                    [true, true] => {}
                    [false, true] => one[w] |= b,
                    [true, false] => zero[w] |= b,
                    [false, false] => no = true,
                }
            };
            for &(p, w) in a.inputs.iter().chain(&a.reads) {
                need(p, [0 >= w, 1 >= w]);
            }
            for &(p, w) in &a.inhibitors {
                need(p, [0 < w, 1 < w]);
            }
            for &(p, d, cap) in &a.capacities {
                need(p, [d <= cap, d.saturating_add(1) <= cap]);
            }
            // Net change per place, in i128 so no weight overflows it.
            let mut delta: Vec<(usize, i128)> = vec![];
            let mut add = |p: usize, d: i128| match delta.iter_mut().find(|x| x.0 == p) {
                Some(x) => x.1 += d,
                None => delta.push((p, d)),
            };
            a.inputs.iter().for_each(|&(p, w)| add(p, -(w as i128)));
            a.outputs.iter().for_each(|&(p, w)| add(p, w as i128));
            let (mut flip, mut from) = (vec![0u64; words], vec![0u64; words]);
            let mut spoil = false;
            for &(p, d) in &delta {
                let (w, b) = (p / 64, 1u64 << (p % 64));
                match d {
                    0 => {}
                    1 => flip[w] |= b,
                    -1 => {
                        flip[w] |= b;
                        from[w] |= b;
                    }
                    _ => spoil = true,
                }
            }
            no |= one.iter().zip(&zero).any(|(o, z)| o & z != 0);
            let start = masks.len();
            for w in 0..words {
                let care = one[w] | zero[w];
                if care | flip[w] != 0 {
                    masks.push(Word {
                        at: w,
                        care,
                        one: one[w],
                        flip: flip[w],
                        from: from[w],
                    });
                }
            }
            spans.push((start, masks.len()));
            never.push(no);
            spoils.push(spoil);
        }
        // u can go from disabled to enabled through t only if t sets a bit
        // u needs set, or clears one u needs clear.
        let n = spans.len();
        let sets = |t: usize, at: usize| -> (u64, u64) {
            let (a, b) = spans[t];
            masks[a..b]
                .iter()
                .find(|m| m.at == at)
                .map_or((0, 0), |m| (m.flip & !m.from, m.from))
        };
        let wakes = (0..n)
            .map(|t| {
                (0..n)
                    .filter(|&u| {
                        let (a, b) = spans[u];
                        !never[u]
                            && masks[a..b].iter().any(|m| {
                                let (on, off) = sets(t, m.at);
                                on & m.one != 0 || off & m.care & !m.one != 0
                            })
                    })
                    .collect()
            })
            .collect();
        BitModel {
            places: self.places.len(),
            words,
            spans,
            masks,
            never,
            spoils,
            wakes,
        }
    }
}

impl BitModel {
    /// `mk` (dense, in [`IndexedModel::places`] order) as bits, or `None`
    /// if some count is not 0 or 1.
    pub fn marking(&self, mk: &[i64]) -> Option<Vec<u64>> {
        let mut bits = vec![0u64; self.words];
        for (p, &v) in mk.iter().enumerate() {
            match v {
                0 => {}
                1 => bits[p / 64] |= 1 << (p % 64),
                _ => return None,
            }
        }
        Some(bits)
    }

    /// A bit marking as a dense one.
    pub fn counts(&self, bits: &[u64]) -> Vec<i64> {
        (0..self.places)
            .map(|p| (bits[p / 64] >> (p % 64) & 1) as i64)
            .collect()
    }

    fn masks(&self, t: usize) -> &[Word] {
        let (a, b) = self.spans[t];
        &self.masks[a..b]
    }

    /// Whether transition `t` can fire at `mk`.
    pub fn enabled(&self, t: usize, mk: &[u64]) -> bool {
        !self.never[t] && self.masks(t).iter().all(|m| mk[m.at] & m.care == m.one)
    }

    /// The transitions that can fire at `mk`.
    pub fn enabled_set(&self, mk: &[u64]) -> TransitionSet {
        let mut s = TransitionSet::empty(self.spans.len());
        for t in 0..self.spans.len() {
            if self.enabled(t, mk) {
                s.set(t, true);
            }
        }
        s
    }

    /// The transitions of `among` that can fire at `mk`, in index order.
    pub fn enabled_in<'a>(
        &'a self,
        mk: &'a [u64],
        among: &'a TransitionSet,
    ) -> impl Iterator<Item = usize> + 'a {
        among.iter().filter(move |&t| self.enabled(t, mk))
    }

    /// Fires enabled transition `t` in place, if the result is a 0/1
    /// marking; otherwise returns false and leaves `mk` as it was. Like
    /// [`IndexedModel::fire_in_place`], it does not check enablement, and
    /// at a marking that does not enable `t` the result means nothing.
    pub fn fire(&self, t: usize, mk: &mut [u64]) -> bool {
        if self.spoils[t] || self.masks(t).iter().any(|m| mk[m.at] & m.flip != m.from) {
            return false;
        }
        self.masks(t).iter().for_each(|m| mk[m.at] ^= m.flip);
        true
    }

    /// The transitions, in index order, that firing `t` can enable: any
    /// transition disabled before a firing of `t` and enabled after it is
    /// among them. A search that keeps some transitions disabled (fires
    /// automatic ones until none is enabled, say) need recheck only these.
    pub fn wakes(&self, t: usize) -> &[usize] {
        &self.wakes[t]
    }

    /// Undoes a [`BitModel::fire`] of `t` that returned true.
    pub fn unfire(&self, t: usize, mk: &mut [u64]) {
        self.masks(t).iter().for_each(|m| mk[m.at] ^= m.flip);
    }
}

#[cfg(test)]
mod tests {
    use crate::indexed::tests::{random_model, Lcg};

    #[test]
    fn agrees_with_the_indexed_rule() {
        let mut r = Lcg(5);
        let (mut fired, mut refused) = (0, 0);
        for _ in 0..3000 {
            let m = random_model(&mut r);
            let ix = m.indexed();
            let bm = ix.bits();
            let n = ix.transitions.len();
            let mut mk: Vec<i64> = (0..ix.places.len()).map(|_| r.below(2) as i64).collect();
            for _ in 0..20 {
                let Some(mut bits) = bm.marking(&mk) else {
                    break;
                };
                assert_eq!(bm.counts(&bits), mk);
                let on = ix.enabled_transitions(&mk);
                assert_eq!(
                    bm.enabled_set(&bits).iter().collect::<Vec<_>>(),
                    on,
                    "at {mk:?} in {m:?}"
                );
                let all = ix.set_of(0..n);
                assert_eq!(bm.enabled_in(&bits, &all).collect::<Vec<_>>(), on);
                for &t in &on {
                    let want = ix.fire(t, &mk);
                    let fits = want.iter().all(|&v| v == 0 || v == 1);
                    let before = bits.clone();
                    assert_eq!(bm.fire(t, &mut bits), fits, "firing {t} at {mk:?} in {m:?}");
                    if fits {
                        assert_eq!(bm.counts(&bits), want);
                        for u in ix.enabled_transitions(&want) {
                            assert!(
                                on.contains(&u) || bm.wakes(t).contains(&u),
                                "{t} enables {u} at {mk:?} in {m:?}"
                            );
                        }
                        bm.unfire(t, &mut bits);
                        fired += 1;
                    } else {
                        refused += 1;
                    }
                    assert_eq!(bits, before);
                }
                // Walk on by the indexed rule, possibly out of 0/1.
                if on.is_empty() {
                    break;
                }
                mk = ix.fire(on[r.below(on.len() as u64) as usize], &mk);
            }
        }
        assert!(
            fired > 5_000 && refused > 500,
            "fired {fired}, refused {refused}"
        );
    }

    #[test]
    fn spans_words() {
        use crate::schema::{Arc, Model, Place, Transition};
        // t moves a token from place 3 to place 130, past two word edges,
        // and is blocked by a token on place 64.
        let m = Model {
            places: (0..131)
                .map(|i| Place {
                    id: format!("p{i}"),
                    ..Default::default()
                })
                .collect(),
            transitions: vec![Transition {
                id: "t".into(),
                ..Default::default()
            }],
            arcs: vec![
                Arc {
                    from: "p3".into(),
                    to: "t".into(),
                    ..Default::default()
                },
                Arc {
                    from: "t".into(),
                    to: "p130".into(),
                    ..Default::default()
                },
                Arc {
                    from: "p64".into(),
                    to: "t".into(),
                    typ: crate::schema::ArcType::Inhibitor,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let ix = m.indexed();
        let bm = ix.bits();
        let mut mk = vec![0i64; 131];
        mk[3] = 1;
        let mut bits = bm.marking(&mk).unwrap();
        assert!(bm.enabled(0, &bits));
        assert!(bm.fire(0, &mut bits));
        assert_eq!(bm.counts(&bits), ix.fire(0, &mk));
        mk[64] = 1;
        assert!(!bm.enabled(0, &bm.marking(&mk).unwrap()));
        mk[64] = 2;
        assert_eq!(bm.marking(&mk), None);
    }
}
