//! `pflow_metamodel::Model` / `pflow_compose::bundle::Subnet` emitters for
//! [`Chart`], ported from go-pflow's `statemachine/metasubnet.go`.
//!
//! Structural mapping:
//!   - each `(region, state)` pair -> one place named `"region_state"`
//!     (`"region_state_sub"` for a nested substate); the initial top-level
//!     state of a region gets `initial = 1`, all others `0`.
//!   - each [`Transition`] -> one net transition named `"event_N"` (1-based
//!     index across all transitions, stable ordering).
//!   - source state -> transition (consume, weight 1), transition -> target
//!     state (produce, weight 1).
//!   - an [`IncrementAction`](crate::types::IncrementAction) becomes a
//!     single weighted arc, `weight = amount` — go-pflow's older
//!     `tokenmodel/petri`-targeting `ToSubnet` had to duplicate the arc
//!     `amount` times because that substrate's arcs are all weight 1;
//!     `pflow_metamodel::Arc` carries a weight, so it is said once.
//!   - `SetAction`/`CallbackAction` have no structural representation
//!     (go-pflow's own `addMetaTransitions` only lowers `IncrementAction`
//!     too) — [`crate::machine::Machine`] applies them after firing.
//!   - a transition with a guard closure is emitted with
//!     `guard_unrepresentable = true` and an explanatory `description`: the
//!     closure cannot cross into the model's string `guard` field, so the
//!     compiled net is a sound-for-safety, not-sound-for-liveness
//!     over-approximation of the chart (see go-pflow's package doc on the
//!     same point, and `pflow-compose`'s `ROADMAP.md` note on
//!     `GuardUnrepresentable`).
//!
//! Each region's mutual exclusion — the invariant that makes a statechart a
//! statechart — is emitted as a `Constraint`, so it stays provable of
//! whatever the chart composes into.

use pflow_compose::bundle::{NetType, Port, PortKind, Subnet};
use pflow_compose::normalize_kinds;
use pflow_metamodel::{Arc as MetaArc, Constraint, Model, Place, StateKind, Transition as MetaTransition};

use crate::types::{path_to_place_name, sorted_keys, Chart, IncrementAction, StatePath};

impl Chart {
    /// Emits the chart as a `pflow_metamodel::Model`, with no composition
    /// boundary.
    pub fn to_meta_model(&self) -> Model {
        let mut m = Model {
            name: self.name.clone(),
            ..Default::default()
        };
        self.add_meta_places(&mut m);
        self.add_meta_transitions(&mut m, "");
        m.constraints.extend(self.region_mutex_constraints());
        normalize_kinds(&mut m);
        m
    }

    /// Emits the chart as a composable `Subnet`.
    ///
    /// Boundary:
    ///   - `"evt:<name>"` in-ports, backed by an `"event:<name>"` place, one
    ///     per distinct event. Delivering an event is structurally "put a
    ///     token in that place".
    ///   - `"out:<region>:<state>"` out-ports for terminal states (no
    ///     transition has the state as a source), so completion is
    ///     observable.
    ///
    /// `NetType::Workflow`: a chart's marking is a cursor, not a pool of
    /// fungible resources — the distinction that stops a `TokenLink` from
    /// fusing a chart state with an inventory counter.
    pub fn to_meta_subnet(&self) -> Subnet {
        let mut m = Model {
            name: self.name.clone(),
            ..Default::default()
        };
        self.add_meta_places(&mut m);

        let mut ports = Vec::new();
        let events = unique_events(self);
        for evt in &events {
            let place_id = format!("event:{evt}");
            m.places.push(Place {
                id: place_id.clone(),
                kind: Some(StateKind::Token),
                exported: true,
                description: format!("delivery slot for event {evt}"),
                ..Default::default()
            });
            ports.push(Port {
                id: format!("evt:{evt}"),
                kind: Some(PortKind::In),
                place: place_id,
                schema: "event".into(),
                ..Default::default()
            });
        }

        self.add_meta_transitions(&mut m, "event:");

        for ts in terminal_states(self) {
            if let Some(p) = m.place_by_id_mut(&ts.place_id) {
                p.exported = true;
            }
            ports.push(Port {
                id: format!("out:{}", ts.label),
                kind: Some(PortKind::Out),
                place: ts.place_id,
                schema: "state".into(),
                ..Default::default()
            });
        }

        m.constraints.extend(self.region_mutex_constraints());
        normalize_kinds(&mut m);
        ports.sort_by(|a, b| a.id.cmp(&b.id));

        Subnet {
            typ: "PetriNet".into(),
            id: self.name.clone(),
            net_type: NetType::Workflow,
            model: m,
            ports,
        }
    }

    fn region_mutex_constraints(&self) -> Vec<Constraint> {
        let mut out = Vec::new();
        for region_name in sorted_keys(&self.regions) {
            let region = &self.regions[&region_name];
            let mut sum = String::new();
            for state_name in sorted_keys(&region.states) {
                let place_id = format!("{region_name}_{state_name}");
                if !sum.is_empty() {
                    sum.push_str(" + ");
                }
                sum.push_str(&format!("tokens(\"{place_id}\")"));
            }
            if sum.is_empty() {
                continue;
            }
            out.push(Constraint {
                id: format!("region_mutex_{region_name}"),
                expr: format!("{sum} == 1"),
            });
        }
        out
    }

    fn add_meta_places(&self, m: &mut Model) {
        for region_name in sorted_keys(&self.regions) {
            let region = &self.regions[&region_name];
            for state_name in sorted_keys(&region.states) {
                let state = &region.states[&state_name];
                let initial = if state.initial { 1 } else { 0 };
                m.places.push(Place {
                    id: format!("{region_name}_{state_name}"),
                    kind: Some(StateKind::Token),
                    initial,
                    ..Default::default()
                });
                for sub_name in sorted_keys(&state.children) {
                    let sub = &state.children[&sub_name];
                    let sub_initial = if sub.initial && state.initial { 1 } else { 0 };
                    m.places.push(Place {
                        id: format!("{region_name}_{state_name}_{sub_name}"),
                        kind: Some(StateKind::Token),
                        initial: sub_initial,
                        ..Default::default()
                    });
                }
            }
        }

        // Counter places targeted by IncrementAction.
        for t in &self.transitions {
            for a in &t.actions {
                if let Some(inc) = as_increment(a.as_ref()) {
                    if m.place_by_id(&inc.place_name).is_none() {
                        m.places.push(Place {
                            id: inc.place_name.clone(),
                            kind: Some(StateKind::Token),
                            description: "counter".into(),
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }

    fn add_meta_transitions(&self, m: &mut Model, event_place_prefix: &str) {
        for (i, t) in self.transitions.iter().enumerate() {
            let txn_id = format!("{}_{}", t.event, i + 1);

            let unrepresentable = t.guard.is_some();
            let description = if unrepresentable {
                "guard not represented: the chart's precondition is a Rust \
                    closure, so this net over-approximates it (sound for safety, not for \
                    liveness)"
                    .to_string()
            } else {
                String::new()
            };

            m.transitions.push(MetaTransition {
                id: txn_id.clone(),
                event: t.event.clone(),
                description,
                guard_unrepresentable: unrepresentable,
                ..Default::default()
            });

            let source_place = path_to_place_name(&StatePath(t.source.clone()));
            let target_place = path_to_place_name(&StatePath(t.target.clone()));

            if !event_place_prefix.is_empty() && !t.event.is_empty() {
                m.arcs.push(MetaArc {
                    from: format!("{event_place_prefix}{}", t.event),
                    to: txn_id.clone(),
                    weight: 1,
                    ..Default::default()
                });
            }
            if !source_place.is_empty() {
                m.arcs.push(MetaArc {
                    from: source_place,
                    to: txn_id.clone(),
                    weight: 1,
                    ..Default::default()
                });
            }
            if !target_place.is_empty() {
                m.arcs.push(MetaArc {
                    from: txn_id.clone(),
                    to: target_place,
                    weight: 1,
                    ..Default::default()
                });
            }

            for a in &t.actions {
                if let Some(inc) = as_increment(a.as_ref()) {
                    let n = if inc.amount <= 0 { 1 } else { inc.amount };
                    m.arcs.push(MetaArc {
                        from: txn_id.clone(),
                        to: inc.place_name.clone(),
                        weight: n,
                        ..Default::default()
                    });
                }
            }
        }
    }
}

/// Recognizes an [`IncrementAction`] among a transition's actions, the way
/// go-pflow's `addMetaTransitions` does with a type switch
/// (`a.(*IncrementAction)`).
fn as_increment(a: &dyn crate::types::Action) -> Option<&IncrementAction> {
    a.as_any().downcast_ref::<IncrementAction>()
}

struct TerminalState {
    place_id: String,
    label: String,
}

fn terminal_states(c: &Chart) -> Vec<TerminalState> {
    let mut sources = std::collections::HashSet::new();
    for t in &c.transitions {
        let p = path_to_place_name(&StatePath(t.source.clone()));
        if !p.is_empty() {
            sources.insert(p);
        }
    }
    let mut out = Vec::new();
    for region_name in sorted_keys(&c.regions) {
        let region = &c.regions[&region_name];
        for state_name in sorted_keys(&region.states) {
            let place_id = format!("{region_name}_{state_name}");
            if !sources.contains(&place_id) {
                out.push(TerminalState {
                    place_id,
                    label: format!("{region_name}:{state_name}"),
                });
            }
        }
    }
    out
}

fn unique_events(c: &Chart) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for t in &c.transitions {
        if seen.insert(t.event.clone()) {
            out.push(t.event.clone());
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use crate::builder::ChartBuilder;
    use crate::types::increment_by;
    use pflow_compose::bundle::{NetType, PortKind};

    fn traffic_light() -> super::Chart {
        let mut b = ChartBuilder::new("light");
        b.region("state")
            .state("red")
            .initial()
            .state("green")
            .state("yellow")
            .end_region()
            .when("timer")
            .in_("state:red")
            .go_to("state:green")
            .do_action(Box::new(increment_by("cycles", 2)))
            .when("timer")
            .in_("state:green")
            .go_to("state:yellow")
            .when("timer")
            .in_("state:yellow")
            .go_to("state:red");
        b.build()
    }

    #[test]
    fn to_meta_model_has_one_place_per_state_plus_counter() {
        let m = traffic_light().to_meta_model();
        let ids: Vec<&str> = m.places.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"state_red"));
        assert!(ids.contains(&"state_green"));
        assert!(ids.contains(&"state_yellow"));
        assert!(ids.contains(&"cycles"));
        assert_eq!(m.place_by_id("state_red").unwrap().initial, 1);
        assert_eq!(m.place_by_id("state_green").unwrap().initial, 0);
        assert_eq!(m.transitions.len(), 3);
        assert_eq!(m.constraints.len(), 1);
        assert_eq!(m.constraints[0].expr, "tokens(\"state_green\") + tokens(\"state_red\") + tokens(\"state_yellow\") == 1");

        // IncrementAction lowers to a single weighted arc, not two duplicates.
        let cycles_arcs: Vec<_> = m.arcs.iter().filter(|a| a.to == "cycles").collect();
        assert_eq!(cycles_arcs.len(), 1);
        assert_eq!(cycles_arcs[0].weight, 2);
    }

    #[test]
    fn to_meta_subnet_has_event_in_ports_and_terminal_out_ports() {
        let sub = traffic_light().to_meta_subnet();
        assert_eq!(sub.net_type, NetType::Workflow);
        let evt_ports: Vec<_> = sub
            .ports
            .iter()
            .filter(|p| p.kind == Some(PortKind::In))
            .collect();
        assert_eq!(evt_ports.len(), 1);
        assert_eq!(evt_ports[0].id, "evt:timer");

        // A traffic light has no terminal state: every state is some
        // transition's source, so there are no out-ports.
        let out_ports: Vec<_> = sub
            .ports
            .iter()
            .filter(|p| p.kind == Some(PortKind::Out))
            .collect();
        assert!(out_ports.is_empty());
    }

    #[test]
    fn terminal_state_gets_an_out_port() {
        let mut b = ChartBuilder::new("switch");
        b.region("state")
            .state("on")
            .initial()
            .state("off")
            .end_region()
            .when("flip")
            .in_("state:on")
            .go_to("state:off");
        let sub = b.build().to_meta_subnet();

        let out_ports: Vec<_> = sub
            .ports
            .iter()
            .filter(|p| p.kind == Some(PortKind::Out))
            .collect();
        assert_eq!(out_ports.len(), 1);
        assert_eq!(out_ports[0].id, "out:state:off");
    }

    #[test]
    fn guard_closure_marks_transition_unrepresentable() {
        let mut b = ChartBuilder::new("gate");
        b.region("state")
            .state("closed")
            .initial()
            .state("open")
            .end_region()
            .when("go")
            .in_("state:closed")
            .go_to("state:open")
            .if_guard(Box::new(|_m| true));
        let m = b.build().to_meta_model();

        assert!(m.transitions[0].guard_unrepresentable);
        assert!(!m.transitions[0].description.is_empty());
    }

    #[test]
    fn no_guard_leaves_transition_fully_represented() {
        let m = traffic_light().to_meta_model();
        assert!(!m.transitions[0].guard_unrepresentable);
        assert!(m.transitions[0].description.is_empty());
    }
}

