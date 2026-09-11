//! `SupplyKind`/`classify_supply`. Ported from go-pflow's `stochastic/supply.go`.
//!
//! See the Go source for the full rationale (reproduced in the doc comments
//! below where it drives a decision this port has to make identically); the
//! short version is that "waiting on a place" means opposite things
//! depending on whether the place is something an operator could buy more
//! of (`Conserved`/`Bounded`) or is fed only by the net's own flow
//! (`Queue`/`State`), and conflating them inverts a contention report.

use std::collections::{HashMap, HashSet};

use pflow_metamodel::Model;

/// Whether a token place is something an operator could buy more of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupplyKind {
    /// Inside a P-invariant that holds tokens at time zero, and some
    /// transition spends those tokens on work arriving from outside the
    /// invariant (a barista pool: `available + busy` is fixed by headcount).
    /// Waiting on one is a capacity finding.
    Conserved,
    /// Carries a declared capacity but is not itself provably conserved
    /// (a pantry shelf). Waiting on one is a capacity finding.
    Bounded,
    /// Unbounded, filled only by the net's own flow. Waiting on one is a
    /// statement about demand, not something to go and buy.
    Queue,
    /// A conserved place whose tokens never serve outside work — a state
    /// marker (a stoplight's colour), not a resource. Waiting on one claims
    /// nothing.
    State,
}

impl SupplyKind {
    /// Whether waiting on this kind of place is a finding an operator can
    /// act on by acquiring more of something.
    pub fn is_capacity(self) -> bool {
        matches!(self, SupplyKind::Conserved | SupplyKind::Bounded)
    }
}

/// Dense mass-action incidence: `c[t][p] = output_weight - input_weight`
/// over Normal (flow) arcs only, restricted to token places — the same net
/// `toNet` builds for the ODE solver in go-pflow.
fn dense_incidence(m: &Model, places: &[String]) -> Vec<Vec<i64>> {
    let index: HashMap<&str, usize> = places
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), i))
        .collect();
    let mut c = vec![vec![0i64; places.len()]; m.transitions.len()];
    for (ti, t) in m.transitions.iter().enumerate() {
        for input in m.inputs(&t.id) {
            if let Some(&p) = index.get(input.place.as_str()) {
                c[ti][p] -= input.weight;
            }
        }
        for output in m.outputs(&t.id) {
            if let Some(&p) = index.get(output.place.as_str()) {
                c[ti][p] += output.weight;
            }
        }
    }
    c
}

/// Whether any transition spends this invariant's tokens on work that comes
/// from outside it — the discriminator between a resource pool (`Conserved`)
/// and a state marker (`State`) that both happen to be conserved.
fn serves_outside_work(
    m: &Model,
    token: &HashMap<String, SupplyKind>,
    members: &HashSet<&str>,
) -> bool {
    for t in &m.transitions {
        let mut draws = false;
        let mut outside = false;
        for input in m.inputs(&t.id) {
            if members.contains(input.place.as_str()) {
                draws = true;
                continue;
            }
            if token.contains_key(&input.place) {
                outside = true;
            }
        }
        if draws && outside {
            return true;
        }
    }
    false
}

/// Labels every token place in `m`.
///
/// Conservation is decided by a Farkas-style P-invariant basis
/// ([`pflow_tropical::invariants::p_invariants_from_dense`]) rather than by
/// naming convention. A truncated or unclassifiable basis can only lose
/// invariants, so an unlabelled place falls back to [`SupplyKind::Queue`].
pub fn classify_supply(m: &Model) -> HashMap<String, SupplyKind> {
    let mut out: HashMap<String, SupplyKind> = HashMap::new();
    let initial = m.initial_marking();

    let places: Vec<String> = m
        .places
        .iter()
        .filter(|p| p.is_token())
        .map(|p| p.id.clone())
        .collect();
    for p in &m.places {
        if !p.is_token() {
            continue;
        }
        if p.capacity > 0 {
            out.insert(p.id.clone(), SupplyKind::Bounded);
        } else {
            out.insert(p.id.clone(), SupplyKind::Queue);
        }
    }
    if places.is_empty() {
        return out;
    }

    let dense = dense_incidence(m, &places);
    let basis = pflow_tropical::p_invariants_from_dense(&dense);
    for vec in &basis {
        let members: HashSet<&str> = vec
            .iter()
            .enumerate()
            .filter(|(_, &c)| c != 0)
            .map(|(i, _)| places[i].as_str())
            .collect();
        let kind = if serves_outside_work(m, &out, &members) {
            SupplyKind::Conserved
        } else {
            SupplyKind::State
        };
        for &place in &members {
            if out.get(place) == Some(&SupplyKind::Conserved) {
                continue;
            }
            let is_token = out.contains_key(place);
            if is_token && *initial.get(place).unwrap_or(&0) > 0 {
                out.insert(place.to_string(), kind);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_metamodel::{Arc, ArcType, Place, Transition};

    /// Ported from go-pflow's `stochastic/testdata/stoplight.json`: a
    /// three-place cycle, only `red` marked at time zero.
    fn stoplight() -> Model {
        Model {
            name: "stoplight".into(),
            places: vec![
                Place {
                    id: "red".into(),
                    initial: 1,
                    ..Default::default()
                },
                Place {
                    id: "green".into(),
                    ..Default::default()
                },
                Place {
                    id: "yellow".into(),
                    ..Default::default()
                },
            ],
            transitions: vec![
                Transition {
                    id: "go".into(),
                    ..Default::default()
                },
                Transition {
                    id: "slow".into(),
                    ..Default::default()
                },
                Transition {
                    id: "stop".into(),
                    ..Default::default()
                },
            ],
            arcs: vec![
                Arc {
                    from: "red".into(),
                    to: "go".into(),
                    ..Default::default()
                },
                Arc {
                    from: "go".into(),
                    to: "green".into(),
                    ..Default::default()
                },
                Arc {
                    from: "green".into(),
                    to: "slow".into(),
                    ..Default::default()
                },
                Arc {
                    from: "slow".into(),
                    to: "yellow".into(),
                    ..Default::default()
                },
                Arc {
                    from: "yellow".into(),
                    to: "stop".into(),
                    ..Default::default()
                },
                Arc {
                    from: "stop".into(),
                    to: "red".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    /// Ported from go-pflow's `stochastic/gating_test.go::staffedShop`: a
    /// barista pool (`available`/`busy`) that only ever changes outcome
    /// because `start` holds a barista for the whole brew, split from an
    /// unrelated `queue` fed by a source transition with no input (so
    /// `queue` cannot appear in any invariant).
    fn staffed_shop(baristas: i64) -> Model {
        Model {
            name: "staffed".into(),
            places: vec![
                Place {
                    id: "queue".into(),
                    ..Default::default()
                },
                Place {
                    id: "brewing".into(),
                    ..Default::default()
                },
                Place {
                    id: "served".into(),
                    ..Default::default()
                },
                Place {
                    id: "available".into(),
                    initial: baristas,
                    ..Default::default()
                },
                Place {
                    id: "busy".into(),
                    ..Default::default()
                },
            ],
            transitions: vec![
                Transition {
                    id: "arrive".into(),
                    rate: 12.0,
                    ..Default::default()
                },
                Transition {
                    id: "start".into(),
                    rate: 60.0,
                    ..Default::default()
                },
                Transition {
                    id: "finish".into(),
                    rate: 20.0,
                    ..Default::default()
                },
            ],
            arcs: vec![
                Arc {
                    from: "arrive".into(),
                    to: "queue".into(),
                    ..Default::default()
                },
                Arc {
                    from: "queue".into(),
                    to: "start".into(),
                    ..Default::default()
                },
                Arc {
                    from: "available".into(),
                    to: "start".into(),
                    ..Default::default()
                },
                Arc {
                    from: "start".into(),
                    to: "brewing".into(),
                    ..Default::default()
                },
                Arc {
                    from: "start".into(),
                    to: "busy".into(),
                    ..Default::default()
                },
                Arc {
                    from: "brewing".into(),
                    to: "finish".into(),
                    ..Default::default()
                },
                Arc {
                    from: "busy".into(),
                    to: "finish".into(),
                    ..Default::default()
                },
                Arc {
                    from: "finish".into(),
                    to: "served".into(),
                    ..Default::default()
                },
                Arc {
                    from: "finish".into(),
                    to: "available".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    /// A stoplight's `red + yellow + green == 1` is exactly as much a
    /// P-invariant as a barista pool's `available + busy == 1`; what
    /// separates them is whether the invariant's tokens are spent on work
    /// arriving from outside it. `go` takes only the red light.
    #[test]
    fn stoplight_red_is_state_not_capacity() {
        let kinds = classify_supply(&stoplight());
        assert_eq!(
            kinds.get("red"),
            Some(&SupplyKind::State),
            "stoplight: red should be a state marker, not a resource"
        );
        assert!(
            !kinds["red"].is_capacity(),
            "stoplight: red ranks as a capacity finding, so a contention report offers \
             the colour of a traffic light as something to acquire more of"
        );
    }

    /// The staffing answer: `start` draws a barista *and* an order, so the
    /// pool serves work arriving from outside the invariant and stays a
    /// capacity finding.
    #[test]
    fn staffed_shop_available_is_conserved() {
        let kinds = classify_supply(&staffed_shop(2));
        assert_eq!(kinds["available"], SupplyKind::Conserved);
        assert!(kinds["available"].is_capacity());
    }

    /// `queue` is fed only by a source transition with no input, so it can
    /// never sit in a P-invariant and falls back to the unbounded default.
    #[test]
    fn source_fed_queue_is_queue() {
        let kinds = classify_supply(&staffed_shop(2));
        assert_eq!(kinds["queue"], SupplyKind::Queue);
        assert!(!kinds["queue"].is_capacity());
    }

    #[test]
    fn declared_capacity_without_invariant_is_bounded() {
        let mut m = staffed_shop(2);
        m.places
            .iter_mut()
            .find(|p| p.id == "queue")
            .unwrap()
            .capacity = 20;
        let kinds = classify_supply(&m);
        assert_eq!(kinds["queue"], SupplyKind::Bounded);
    }

    #[test]
    fn inhibitor_and_read_arcs_are_not_flow() {
        // A read/inhibitor test must not be mistaken for a conserving flow
        // arc when building the dense incidence matrix.
        let mut m = staffed_shop(2);
        m.arcs.push(Arc {
            from: "served".into(),
            to: "start".into(),
            typ: ArcType::Read,
            ..Default::default()
        });
        let kinds = classify_supply(&m);
        assert_eq!(kinds["available"], SupplyKind::Conserved);
    }
}
