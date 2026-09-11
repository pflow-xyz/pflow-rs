//! The firing rule, in one place. Ported from go-pflow's `metamodel/firing.go`.
//!
//! A Petri net's semantics are four rules that have to hold together: a
//! consuming arc needs at least `weight` tokens; a read arc needs at least
//! `weight` tokens present but consumes none; an inhibitor arc blocks once
//! its place holds `weight` or more; a place's capacity is a POST-FIRING
//! bound, netting out what this same firing consumes from that place.
//! Everything that executes or projects a net should route through
//! [`Model::enabled`] and [`Model::fire`] rather than reimplementing any
//! one of the four — see the ground rules in `ROADMAP.md`.

use std::collections::HashMap;

use crate::error::Error;
use crate::schema::{ArcType, Model};

/// A token count per place. Absent places read as zero, so a caller may
/// pass a sparse map.
pub type Marking = HashMap<String, i64>;

/// One arc resolved against a transition: which token place it touches,
/// with what weight, in what role, and whether it is kinetic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcRef {
    pub place: String,
    pub weight: i64,
    pub typ: ArcType,
    /// Mirrors `Arc::is_kinetic`, already defaulted. Meaningful only on
    /// [`Model::inputs`] results — outputs and tests take no part in a rate
    /// law.
    pub kinetic: bool,
}

impl Model {
    /// The marking the model declares.
    pub fn initial_marking(&self) -> Marking {
        let mut mk = Marking::with_capacity(self.places.len());
        for p in &self.places {
            if p.is_token() {
                mk.insert(p.id.clone(), p.initial);
            }
        }
        mk
    }

    /// Arcs that consume from a token place when `transition` fires. Read
    /// and inhibitor arcs are excluded: they test the marking without
    /// moving it.
    pub fn inputs(&self, transition: &str) -> Vec<ArcRef> {
        self.arcs
            .iter()
            .filter(|a| {
                a.to == transition && !a.is_read_only() && self.token_place(&a.from).is_some()
            })
            .map(|a| ArcRef {
                place: a.from.clone(),
                weight: a.effective_weight(),
                typ: a.typ,
                kinetic: a.is_kinetic(),
            })
            .collect()
    }

    /// Arcs that produce into a token place when `transition` fires.
    pub fn outputs(&self, transition: &str) -> Vec<ArcRef> {
        self.arcs
            .iter()
            .filter(|a| {
                a.from == transition && !a.is_read_only() && self.token_place(&a.to).is_some()
            })
            .map(|a| ArcRef {
                place: a.to.clone(),
                weight: a.effective_weight(),
                typ: a.typ,
                kinetic: a.is_kinetic(),
            })
            .collect()
    }

    /// Arcs that gate a firing without moving tokens: read arcs (require at
    /// least `weight`) and inhibitor arcs (require fewer than `weight`).
    pub fn tests(&self, transition: &str) -> Vec<ArcRef> {
        self.arcs
            .iter()
            .filter(|a| {
                a.to == transition && a.is_read_only() && self.token_place(&a.from).is_some()
            })
            .map(|a| ArcRef {
                place: a.from.clone(),
                weight: a.effective_weight(),
                typ: a.typ,
                kinetic: a.is_kinetic(),
            })
            .collect()
    }

    /// Whether `transition` can fire at `mk`. An unknown transition is
    /// never enabled.
    pub fn enabled(&self, transition: &str, mk: &Marking) -> bool {
        self.enablement(transition, mk).is_ok()
    }

    /// [`Model::enabled`] with the reason attached, for callers that report
    /// refusals to a human rather than merely acting on them.
    pub fn enabled_why_not(&self, transition: &str, mk: &Marking) -> Result<(), Error> {
        self.enablement(transition, mk)
    }

    fn enablement(&self, transition: &str, mk: &Marking) -> Result<(), Error> {
        if self.transition_by_id(transition).is_none() {
            return Err(Error::UnknownTransition(
                transition.to_string(),
                self.name.clone(),
            ));
        }

        for input in self.inputs(transition) {
            if input.weight < 0 {
                return Err(Error::NegativeWeight {
                    transition: transition.to_string(),
                    place: input.place,
                    weight: input.weight,
                });
            }
            let have = *mk.get(&input.place).unwrap_or(&0);
            if have < input.weight {
                return Err(Error::InsufficientInput {
                    transition: transition.to_string(),
                    place: input.place,
                    weight: input.weight,
                    have,
                });
            }
        }

        for output in self.outputs(transition) {
            if output.weight < 0 {
                return Err(Error::NegativeWeight {
                    transition: transition.to_string(),
                    place: output.place,
                    weight: output.weight,
                });
            }
        }

        for test in self.tests(transition) {
            if test.weight < 0 {
                return Err(Error::NegativeWeight {
                    transition: transition.to_string(),
                    place: test.place,
                    weight: test.weight,
                });
            }
            let have = *mk.get(&test.place).unwrap_or(&0);
            match test.typ {
                ArcType::Inhibitor => {
                    if have >= test.weight {
                        return Err(Error::Inhibited {
                            transition: transition.to_string(),
                            place: test.place,
                            weight: test.weight,
                            have,
                        });
                    }
                }
                _ => {
                    // read arc
                    if have < test.weight {
                        return Err(Error::ReadUnsatisfied {
                            transition: transition.to_string(),
                            place: test.place,
                            weight: test.weight,
                            have,
                        });
                    }
                }
            }
        }

        // Capacity is checked on the net effect, which is why it cannot be
        // folded into the loops above: a self-loop consumes and produces on
        // one place. Weights are non-negative here (checked above), but a
        // model can still declare one near i64::MAX, so the net and the
        // post-firing marking are computed with saturating arithmetic
        // rather than `+`/`-`, which would panic (debug) or silently wrap
        // (release) and could turn an over-capacity firing into one that
        // looks allowed.
        let mut net: HashMap<String, i64> = HashMap::new();
        for input in self.inputs(transition) {
            let entry = net.entry(input.place).or_insert(0);
            *entry = entry.saturating_sub(input.weight);
        }
        for output in self.outputs(transition) {
            let entry = net.entry(output.place).or_insert(0);
            *entry = entry.saturating_add(output.weight);
        }
        for p in &self.places {
            if p.capacity <= 0 || !p.is_token() {
                continue;
            }
            let delta = match net.get(p.id.as_str()) {
                Some(d) if *d > 0 => *d,
                _ => continue,
            };
            let have = *mk.get(&p.id).unwrap_or(&0);
            let after = have.saturating_add(delta);
            if after > p.capacity {
                return Err(Error::CapacityExceeded {
                    transition: transition.to_string(),
                    place: p.id.clone(),
                    after,
                    capacity: p.capacity,
                });
            }
        }
        Ok(())
    }

    /// The marking after `transition` fires, leaving `mk` untouched. Does
    /// not check enablement — callers that care ask [`Model::enabled`]
    /// first. Read and inhibitor arcs move nothing.
    pub fn fire(&self, transition: &str, mk: &Marking) -> Marking {
        let mut next = mk.clone();
        for input in self.inputs(transition) {
            let entry = next.entry(input.place).or_insert(0);
            *entry = entry.saturating_sub(input.weight);
        }
        for output in self.outputs(transition) {
            let entry = next.entry(output.place).or_insert(0);
            *entry = entry.saturating_add(output.weight);
        }
        next
    }

    /// Transitions that can fire at `mk`, in declaration order.
    pub fn enabled_transitions(&self, mk: &Marking) -> Vec<String> {
        self.transitions
            .iter()
            .filter(|t| self.enabled(&t.id, mk))
            .map(|t| t.id.clone())
            .collect()
    }

    /// Human-readable descriptions of the constraints a *continuous* engine
    /// cannot represent: read arcs, inhibitors, non-kinetic input arcs, a
    /// capacity something can raise, guards, delays and unexpanded stages.
    /// Meant to be shown, not counted.
    pub fn gating(&self) -> Vec<String> {
        let mut reads = 0;
        let mut inhibits = 0;
        let mut static_count = 0;
        for a in &self.arcs {
            if a.is_read() {
                reads += 1;
            } else if a.is_inhibitor() {
                inhibits += 1;
            } else if !a.is_kinetic()
                && self.token_place(&a.from).is_some()
                && self.transition_by_id(&a.to).is_some()
            {
                static_count += 1;
            }
        }

        // A capacity only gates if something can push the place up to it.
        let mut raised: std::collections::HashSet<String> = std::collections::HashSet::new();
        for t in &self.transitions {
            for o in self.outputs(&t.id) {
                raised.insert(o.place);
            }
        }
        let caps: Vec<&str> = self
            .places
            .iter()
            .filter(|p| p.capacity > 0 && p.is_token() && raised.contains(p.id.as_str()))
            .map(|p| p.id.as_str())
            .collect();

        let guards: Vec<&str> = self
            .transitions
            .iter()
            .filter(|t| !t.guard.is_empty())
            .map(|t| t.id.as_str())
            .collect();

        let mut out = Vec::new();
        if reads > 0 {
            out.push(format!(
                "{reads} read arc(s) gate a firing without consuming; a continuous solver cannot test them"
            ));
        }
        if inhibits > 0 {
            out.push(format!(
                "{inhibits} inhibitor arc(s) block a firing above a threshold; a continuous solver cannot test them"
            ));
        }
        if static_count > 0 {
            out.push(format!(
                "{static_count} non-kinetic input arc(s) gate and consume without scaling the rate; a mass-action solver has no way to omit them from the rate law"
            ));
        }
        // Go's `%v` on a `[]string` prints `[a b c]` — space-separated, no
        // quotes, no commas — not Rust's `{:?}` (`["a", "b", "c"]`). This
        // wording is part of the byte-exact contract the SDE/Forecast
        // refusal goldens pin (pflow-solver's `sde`/`stochastic` tests), so
        // match it exactly rather than Rust's default Debug rendering.
        fn go_slice(items: &[&str]) -> String {
            format!("[{}]", items.join(" "))
        }

        if !caps.is_empty() {
            out.push(format!(
                "capacity is declared on {} but is a post-firing bound, which has no continuous analogue",
                go_slice(&caps)
            ));
        }
        if !guards.is_empty() {
            out.push(format!(
                "guards on {} are expressions evaluated at a firing instant, which a continuous solution does not have",
                go_slice(&guards)
            ));
        }

        let delayed: Vec<&str> = self
            .transitions
            .iter()
            .filter(|t| t.delay > 0.0)
            .map(|t| t.id.as_str())
            .collect();
        if !delayed.is_empty() {
            out.push(format!(
                "delays on {} are deterministic timers — inputs consumed at start, outputs produced a fixed time later — which mass action cannot express; declare stages instead for a continuous-compatible near-constant duration",
                go_slice(&delayed)
            ));
        }

        let staged: Vec<&str> = self
            .transitions
            .iter()
            .filter(|t| t.stages > 1)
            .map(|t| t.id.as_str())
            .collect();
        if !staged.is_empty() {
            out.push(format!(
                "stages on {} declare phase-type durations; an engine that has not expanded them (expand_stages) would run them as plain exponential",
                go_slice(&staged)
            ));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Arc, ArcType, Place, Transition};

    fn model(places: Vec<Place>, transitions: Vec<Transition>, arcs: Vec<Arc>) -> Model {
        Model {
            name: "t".to_string(),
            places,
            transitions,
            arcs,
            ..Default::default()
        }
    }

    fn place(id: &str) -> Place {
        Place {
            id: id.to_string(),
            ..Default::default()
        }
    }

    fn transition(id: &str) -> Transition {
        Transition {
            id: id.to_string(),
            ..Default::default()
        }
    }

    fn mk(pairs: &[(&str, i64)]) -> Marking {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn consuming_arc_enough_tokens() {
        let m = model(
            vec![place("p")],
            vec![transition("t")],
            vec![Arc {
                from: "p".into(),
                to: "t".into(),
                weight: 2,
                ..Default::default()
            }],
        );
        assert!(m.enabled("t", &mk(&[("p", 2)])));
    }

    #[test]
    fn consuming_arc_one_short() {
        let m = model(
            vec![place("p")],
            vec![transition("t")],
            vec![Arc {
                from: "p".into(),
                to: "t".into(),
                weight: 2,
                ..Default::default()
            }],
        );
        assert!(!m.enabled("t", &mk(&[("p", 1)])));
        let err = m.enabled_why_not("t", &mk(&[("p", 1)])).unwrap_err();
        assert!(err.to_string().contains("needs 2 token(s) on p"), "{err}");
    }

    #[test]
    fn read_arc_gates_at_its_weight() {
        let m = model(
            vec![place("key"), place("p")],
            vec![transition("t")],
            vec![
                Arc {
                    from: "p".into(),
                    to: "t".into(),
                    ..Default::default()
                },
                Arc {
                    from: "key".into(),
                    to: "t".into(),
                    weight: 3,
                    typ: ArcType::Read,
                    ..Default::default()
                },
            ],
        );
        assert!(!m.enabled("t", &mk(&[("p", 1), ("key", 2)])));
        assert!(m.enabled("t", &mk(&[("p", 1), ("key", 3)])));
    }

    #[test]
    fn inhibitor_arc_blocks_at_its_weight() {
        let m = model(
            vec![place("stop"), place("p")],
            vec![transition("t")],
            vec![
                Arc {
                    from: "p".into(),
                    to: "t".into(),
                    ..Default::default()
                },
                Arc {
                    from: "stop".into(),
                    to: "t".into(),
                    typ: ArcType::Inhibitor,
                    ..Default::default()
                },
            ],
        );
        assert!(!m.enabled("t", &mk(&[("p", 1), ("stop", 1)])));

        let m2 = model(
            vec![place("stop"), place("p")],
            vec![transition("t")],
            vec![
                Arc {
                    from: "p".into(),
                    to: "t".into(),
                    ..Default::default()
                },
                Arc {
                    from: "stop".into(),
                    to: "t".into(),
                    weight: 2,
                    typ: ArcType::Inhibitor,
                    ..Default::default()
                },
            ],
        );
        assert!(m2.enabled("t", &mk(&[("p", 1), ("stop", 1)])));
    }

    #[test]
    fn capacity_refuses_an_overflow() {
        let m = model(
            vec![
                place("p"),
                Place {
                    id: "q".into(),
                    capacity: 2,
                    ..Default::default()
                },
            ],
            vec![transition("t")],
            vec![
                Arc {
                    from: "p".into(),
                    to: "t".into(),
                    ..Default::default()
                },
                Arc {
                    from: "t".into(),
                    to: "q".into(),
                    ..Default::default()
                },
            ],
        );
        assert!(!m.enabled("t", &mk(&[("p", 1), ("q", 2)])));
    }

    #[test]
    fn capacity_full_place_admits_a_self_loop() {
        let m = model(
            vec![Place {
                id: "q".into(),
                capacity: 2,
                ..Default::default()
            }],
            vec![transition("t")],
            vec![
                Arc {
                    from: "q".into(),
                    to: "t".into(),
                    ..Default::default()
                },
                Arc {
                    from: "t".into(),
                    to: "q".into(),
                    ..Default::default()
                },
            ],
        );
        assert!(m.enabled("t", &mk(&[("q", 2)])));
    }

    #[test]
    fn capacity_zero_means_unbounded() {
        let m = model(
            vec![place("p"), place("q")],
            vec![transition("t")],
            vec![
                Arc {
                    from: "p".into(),
                    to: "t".into(),
                    ..Default::default()
                },
                Arc {
                    from: "t".into(),
                    to: "q".into(),
                    weight: 9000,
                    ..Default::default()
                },
            ],
        );
        assert!(m.enabled("t", &mk(&[("p", 1)])));
    }

    #[test]
    fn a_source_transition_is_always_enabled() {
        let m = model(
            vec![place("p")],
            vec![transition("t")],
            vec![Arc {
                from: "t".into(),
                to: "p".into(),
                ..Default::default()
            }],
        );
        assert!(m.enabled("t", &Marking::new()));
    }

    #[test]
    fn unknown_transition_is_not_enabled() {
        let m = model(vec![place("p")], vec![transition("t")], vec![]);
        assert!(!m.enabled("nope", &Marking::new()));
        let err = m.enabled_why_not("nope", &Marking::new()).unwrap_err();
        assert!(err.to_string().contains("no transition"), "{err}");
    }

    #[test]
    fn fire_moves_only_consuming_arcs() {
        let m = model(
            vec![place("in"), place("key"), place("stop"), place("out")],
            vec![transition("t")],
            vec![
                Arc {
                    from: "in".into(),
                    to: "t".into(),
                    weight: 2,
                    ..Default::default()
                },
                Arc {
                    from: "key".into(),
                    to: "t".into(),
                    typ: ArcType::Read,
                    ..Default::default()
                },
                Arc {
                    from: "stop".into(),
                    to: "t".into(),
                    typ: ArcType::Inhibitor,
                    ..Default::default()
                },
                Arc {
                    from: "t".into(),
                    to: "out".into(),
                    weight: 3,
                    ..Default::default()
                },
            ],
        );
        let before = mk(&[("in", 5), ("key", 1), ("stop", 0), ("out", 0)]);
        let after = m.fire("t", &before);

        assert_eq!(after["in"], 3);
        assert_eq!(after["key"], 1);
        assert_eq!(after["out"], 3);
        // Fire must not mutate its input marking.
        assert_eq!(before["in"], 5);
        assert_eq!(before["key"], 1);
    }

    #[test]
    fn gating_names_what_continuous_solvers_drop() {
        let m = model(
            vec![
                place("p"),
                Place {
                    id: "q".into(),
                    capacity: 4,
                    ..Default::default()
                },
            ],
            vec![
                Transition {
                    id: "t".into(),
                    guard: "tokens(\"p\") > 0".into(),
                    ..Default::default()
                },
                transition("fill"),
            ],
            vec![
                Arc {
                    from: "p".into(),
                    to: "t".into(),
                    typ: ArcType::Read,
                    ..Default::default()
                },
                Arc {
                    from: "q".into(),
                    to: "t".into(),
                    typ: ArcType::Inhibitor,
                    ..Default::default()
                },
                Arc {
                    from: "fill".into(),
                    to: "q".into(),
                    ..Default::default()
                },
            ],
        );
        let got = m.gating();
        assert_eq!(got.len(), 4, "{got:?}");
        let joined = got.join("\n");
        for want in ["read arc", "inhibitor arc", "capacity", "guard"] {
            assert!(
                joined.contains(want),
                "gating() does not mention {want:?}: {got:?}"
            );
        }

        let plain = model(
            vec![place("p")],
            vec![transition("t")],
            vec![Arc {
                from: "p".into(),
                to: "t".into(),
                ..Default::default()
            }],
        );
        assert!(plain.gating().is_empty());

        // A capacity nothing can reach is documentation, not a constraint.
        let unreachable = model(
            vec![
                Place {
                    id: "stock".into(),
                    initial: 100,
                    capacity: 200,
                    ..Default::default()
                },
                place("used"),
            ],
            vec![transition("consume")],
            vec![
                Arc {
                    from: "stock".into(),
                    to: "consume".into(),
                    ..Default::default()
                },
                Arc {
                    from: "consume".into(),
                    to: "used".into(),
                    ..Default::default()
                },
            ],
        );
        assert!(unreachable.gating().is_empty());
    }

    #[test]
    fn negative_weight_input_arc_is_never_enabled() {
        // A negative weight must not be treated as "always satisfied" —
        // `have < weight` is false for any have >= 0 when weight is
        // negative, which used to make this arc vacuously true and let
        // `fire` manufacture tokens on an empty place.
        let m = model(
            vec![place("p")],
            vec![transition("t")],
            vec![Arc {
                from: "p".into(),
                to: "t".into(),
                weight: -5,
                ..Default::default()
            }],
        );
        assert!(!m.enabled("t", &mk(&[("p", 0)])));
        let err = m.enabled_why_not("t", &mk(&[("p", 0)])).unwrap_err();
        assert!(matches!(err, Error::NegativeWeight { weight: -5, .. }), "{err}");
    }

    #[test]
    fn negative_weight_inhibitor_and_read_arcs_are_rejected_too() {
        let inhibitor = model(
            vec![place("stop"), place("p")],
            vec![transition("t")],
            vec![
                Arc {
                    from: "p".into(),
                    to: "t".into(),
                    ..Default::default()
                },
                Arc {
                    from: "stop".into(),
                    to: "t".into(),
                    weight: -1,
                    typ: ArcType::Inhibitor,
                    ..Default::default()
                },
            ],
        );
        assert!(!inhibitor.enabled("t", &mk(&[("p", 1), ("stop", 0)])));

        let read = model(
            vec![place("key"), place("p")],
            vec![transition("t")],
            vec![
                Arc {
                    from: "p".into(),
                    to: "t".into(),
                    ..Default::default()
                },
                Arc {
                    from: "key".into(),
                    to: "t".into(),
                    weight: -1,
                    typ: ArcType::Read,
                    ..Default::default()
                },
            ],
        );
        assert!(!read.enabled("t", &mk(&[("p", 1), ("key", 0)])));
    }

    #[test]
    fn capacity_check_saturates_instead_of_overflowing_on_an_extreme_weight() {
        // Regression test: `have + delta` used to be plain `i64` addition
        // over a weight read straight from (potentially untrusted)
        // deserialized model JSON. A model declaring i64::MAX would panic
        // this in a debug build and silently wrap past capacity in release.
        let m = model(
            vec![
                place("p"),
                Place {
                    id: "q".into(),
                    capacity: 2,
                    ..Default::default()
                },
            ],
            vec![transition("t")],
            vec![
                Arc {
                    from: "p".into(),
                    to: "t".into(),
                    ..Default::default()
                },
                Arc {
                    from: "t".into(),
                    to: "q".into(),
                    weight: i64::MAX,
                    ..Default::default()
                },
            ],
        );
        // Must not panic, and must correctly refuse (an addition that would
        // overflow can never fit in a finite capacity).
        assert!(!m.enabled("t", &mk(&[("p", 1), ("q", 1)])));
        let err = m.enabled_why_not("t", &mk(&[("p", 1), ("q", 1)])).unwrap_err();
        assert!(matches!(err, Error::CapacityExceeded { .. }), "{err}");
    }

    #[test]
    fn enabled_transitions_is_declaration_ordered() {
        let m = model(
            vec![place("p")],
            vec![transition("c"), transition("a"), transition("b")],
            vec![Arc {
                from: "p".into(),
                to: "b".into(),
                ..Default::default()
            }],
        );
        assert_eq!(m.enabled_transitions(&Marking::new()), vec!["c", "a"]);
    }
}
