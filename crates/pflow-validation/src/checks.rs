//! The individual checks `Validator::validate` runs. Ported from go-pflow's
//! `validation/checks.go`.

use std::collections::{HashMap, HashSet};

use pflow_metamodel::ArcType;
use pflow_reachability::{Analyzer, InvariantAnalyzer};

use crate::types::Validator;

/// Bounds the covering search `check_unbounded` runs. Validation is meant
/// to be fast and run on every edit, so this is deliberately well below
/// `pflow_reachability::Analyzer`'s own default (10,000).
const UNBOUNDED_SEARCH_LIMIT: usize = 2000;

impl<'m> Validator<'m> {
    /// Checks basic structural properties: an empty net, missing
    /// transitions/arcs, negative initial markings, initial tokens over
    /// capacity, and non-positive arc weights.
    pub(crate) fn check_structure(&mut self) {
        let model = self.model;

        if model.places.is_empty() {
            self.add_error("structure", "Net has no places", Vec::new(), "Add at least one place");
            return;
        }
        if model.transitions.is_empty() {
            self.add_warning(
                "structure",
                "Net has no transitions",
                Vec::new(),
                "Add transitions to enable dynamics",
            );
        }
        if model.arcs.is_empty() {
            self.add_warning(
                "structure",
                "Net has no arcs",
                Vec::new(),
                "Add arcs to connect places and transitions",
            );
        }

        for place in &model.places {
            if !place.is_token() {
                continue;
            }
            if place.initial < 0 {
                self.add_error(
                    "structure",
                    format!("Place '{}' has negative initial tokens", place.id),
                    vec![place.id.clone()],
                    "Set initial tokens to non-negative value",
                );
            }
            if place.capacity > 0 && place.initial > place.capacity {
                self.add_error(
                    "structure",
                    format!(
                        "Place '{}' initial tokens ({}) exceed capacity ({})",
                        place.id, place.initial, place.capacity
                    ),
                    vec![place.id.clone()],
                    "Reduce initial tokens or increase capacity",
                );
            }
        }

        for (i, arc) in model.arcs.iter().enumerate() {
            // A dangling arc — an endpoint naming neither a place nor a
            // transition the model declares — is not caught anywhere else:
            // `Model::inputs`/`outputs`/`tests` simply skip an arc whose
            // place-side endpoint doesn't resolve (`token_place` returns
            // `None`), so a typoed id silently drops the arc from the
            // firing rule rather than refusing to load. go-pflow has no
            // equivalent check to port (its `checks.go` has none), but
            // ROADMAP.md calls it out explicitly and it costs nothing here.
            let from_place = model.place_by_id(&arc.from).is_some();
            let from_trans = model.transition_by_id(&arc.from).is_some();
            let to_place = model.place_by_id(&arc.to).is_some();
            let to_trans = model.transition_by_id(&arc.to).is_some();
            let valid_shape = (from_place && to_trans) || (from_trans && to_place);
            if !valid_shape {
                self.add_error(
                    "structure",
                    format!(
                        "Arc {} ({} -> {}) is dangling: one endpoint is not a declared place/transition, or both endpoints are the same kind",
                        i, arc.from, arc.to
                    ),
                    vec![arc.from.clone(), arc.to.clone()],
                    "Fix the arc's endpoint id(s), or add the missing place/transition",
                );
                continue;
            }

            if arc.effective_weight() <= 0 {
                self.add_error(
                    "structure",
                    format!("Arc {} ({} -> {}) has non-positive weight", i, arc.from, arc.to),
                    vec![arc.from.clone(), arc.to.clone()],
                    "Set arc weight to positive value",
                );
            }
        }
    }

    /// Checks for disconnected places/transitions and transitions missing
    /// an input or output side.
    pub(crate) fn check_connectivity(&mut self) {
        let model = self.model;

        let mut connected: HashSet<&str> = HashSet::new();
        for arc in &model.arcs {
            connected.insert(arc.from.as_str());
            connected.insert(arc.to.as_str());
        }

        for place in &model.places {
            if !connected.contains(place.id.as_str()) {
                self.add_warning(
                    "connectivity",
                    format!("Place '{}' is not connected to any transition", place.id),
                    vec![place.id.clone()],
                    "Add arcs to connect this place",
                );
            }
        }
        for trans in &model.transitions {
            if !connected.contains(trans.id.as_str()) {
                self.add_warning(
                    "connectivity",
                    format!("Transition '{}' is not connected", trans.id),
                    vec![trans.id.clone()],
                    "Add input and output arcs",
                );
            }
        }

        let mut inputs: HashMap<&str, usize> = HashMap::new();
        let mut outputs: HashMap<&str, usize> = HashMap::new();
        for arc in &model.arcs {
            if model.place_by_id(&arc.from).is_some() {
                *inputs.entry(arc.to.as_str()).or_insert(0) += 1;
            }
            if model.place_by_id(&arc.to).is_some() {
                *outputs.entry(arc.from.as_str()).or_insert(0) += 1;
            }
        }

        for trans in &model.transitions {
            if !inputs.contains_key(trans.id.as_str()) {
                self.add_warning(
                    "connectivity",
                    format!("Transition '{}' has no input places", trans.id),
                    vec![trans.id.clone()],
                    "Add input arcs from places",
                );
            }
            if !outputs.contains_key(trans.id.as_str()) {
                self.add_warning(
                    "connectivity",
                    format!("Transition '{}' has no output places", trans.id),
                    vec![trans.id.clone()],
                    "Add output arcs to places",
                );
            }
        }
    }

    /// A simple heuristic: a transition that cannot fire at the *initial*
    /// marking because some input place doesn't have enough tokens yet.
    /// This is not a claim the transition can never fire — only that it
    /// can't fire immediately; full liveness needs the reachability graph.
    pub(crate) fn check_deadlocks(&mut self) {
        let model = self.model;
        let initial = model.initial_marking();

        let mut by_transition: HashMap<&str, Vec<(&str, i64)>> = HashMap::new();
        for arc in &model.arcs {
            if arc.typ != ArcType::Normal {
                continue;
            }
            if model.place_by_id(&arc.from).is_some() {
                by_transition
                    .entry(arc.to.as_str())
                    .or_default()
                    .push((arc.from.as_str(), arc.effective_weight()));
            }
        }

        for trans in &model.transitions {
            let Some(inputs) = by_transition.get(trans.id.as_str()) else {
                continue;
            };
            let mut blocked = Vec::new();
            for &(place, weight) in inputs {
                if initial.get(place).copied().unwrap_or(0) < weight {
                    blocked.push(place.to_string());
                }
            }
            if !blocked.is_empty() {
                let mut location = vec![trans.id.clone()];
                location.extend(blocked.iter().cloned());
                self.add_warning(
                    "deadlock",
                    format!(
                        "Transition '{}' cannot fire with initial marking (insufficient tokens in: {:?})",
                        trans.id, blocked
                    ),
                    location,
                    "Increase initial tokens in input places or adjust arc weights",
                );
            }
        }
    }

    /// Reports places that can accumulate tokens without limit: a place
    /// covered by a P-invariant is bounded by that invariant's constant,
    /// and a coverability (pump) witness proves unboundedness outright
    /// rather than merely suggesting it.
    pub(crate) fn check_unbounded(&mut self) {
        let model = self.model;
        let analyzer = InvariantAnalyzer::new(model);

        let basis = analyzer.p_invariant_basis();
        let mut covered: HashSet<&str> = HashSet::new();
        for vec in &basis.basis {
            for (i, &c) in vec.iter().enumerate() {
                if c > 0 {
                    covered.insert(basis.labels[i].as_str());
                }
            }
        }

        if let Some(w) =
            Analyzer::new(model).with_max_states(UNBOUNDED_SEARCH_LIMIT).find_unbounded_witness()
        {
            for name in &w.places {
                if let Some(place) = model.place_by_id(name) {
                    if place.capacity > 0 {
                        continue; // an explicit capacity caps it
                    }
                }
                self.add_error(
                    "unbounded",
                    format!(
                        "Place '{}' is unbounded: repeating [{}] adds tokens indefinitely",
                        name,
                        w.pump.join(" -> ")
                    ),
                    vec![name.clone()],
                    "Add a capacity, or consume the accumulated tokens on the cycle",
                );
            }
        }

        let mut place_inputs: HashMap<&str, usize> = HashMap::new();
        let mut place_outputs: HashMap<&str, usize> = HashMap::new();
        for arc in &model.arcs {
            if model.place_by_id(&arc.to).is_some() {
                *place_inputs.entry(arc.to.as_str()).or_insert(0) += 1;
            }
            if model.place_by_id(&arc.from).is_some() {
                *place_outputs.entry(arc.from.as_str()).or_insert(0) += 1;
            }
        }

        for place in &model.places {
            if !place.is_token() || place.capacity > 0 {
                continue;
            }
            let inputs = place_inputs.get(place.id.as_str()).copied().unwrap_or(0);
            let outputs = place_outputs.get(place.id.as_str()).copied().unwrap_or(0);

            if inputs > 0 && outputs == 0 {
                self.add_info(
                    "unbounded",
                    format!("Place '{}' is a sink (only inputs, no outputs)", place.id),
                    vec![place.id.clone()],
                );
            }
            if outputs > 0 && inputs == 0 {
                self.add_info(
                    "unbounded",
                    format!("Place '{}' is a source (only outputs, no inputs)", place.id),
                    vec![place.id.clone()],
                );
            }

            if !covered.contains(place.id.as_str()) && inputs > 0 {
                self.add_warning(
                    "unbounded",
                    format!(
                        "Place '{}' is not covered by any P-invariant, so nothing structurally bounds it",
                        place.id
                    ),
                    vec![place.id.clone()],
                    "Add a capacity, or balance the flow so the place participates in a conservation law",
                );
            }
        }
    }

    /// Checks whether every transition balances input and output weight
    /// (strict token conservation), and separately reports the full
    /// minimal-support P-invariant basis — conservation laws that hold at
    /// every reachable marking, whether or not the net conserves the total
    /// token count.
    pub(crate) fn check_conservation(&mut self) {
        let model = self.model;

        let mut conserved = true;
        let mut non_conserving: Vec<String> = Vec::new();

        for trans in &model.transitions {
            let input_sum: i64 = model.inputs(&trans.id).iter().map(|a| a.weight).sum();
            let output_sum: i64 = model.outputs(&trans.id).iter().map(|a| a.weight).sum();
            if input_sum != output_sum {
                conserved = false;
                non_conserving.push(trans.id.clone());
            }
        }
        self.result.summary.conserved = conserved;

        if !conserved {
            non_conserving.sort();
            self.add_info(
                "conservation",
                format!(
                    "Net does not conserve total tokens (transitions with unbalanced flow: {:?})",
                    non_conserving
                ),
                non_conserving,
            );
        } else {
            self.add_info("conservation", "Net conserves tokens (all transitions have balanced input/output)", Vec::new());
        }

        let initial = model.initial_marking();
        let invariants = InvariantAnalyzer::new(model).find_p_invariants(&initial);
        if invariants.is_empty() {
            self.result.invariants = Vec::new();
            return;
        }

        let mut rendered: Vec<String> = invariants.iter().map(|inv| inv.render()).collect();
        rendered.sort();
        self.result.invariants = rendered.clone();

        self.add_info(
            "conservation",
            format!("Found {} conservation law(s): {}", rendered.len(), rendered.join("; ")),
            Vec::new(),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::types::Validator;
    use pflow_metamodel::schema::{Arc, ArcType as AT, Model, Place, Transition};

    fn cycle_model() -> Model {
        Model {
            name: "cycle".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
            ],
            transitions: vec![
                Transition { id: "t0".into(), ..Default::default() },
                Transition { id: "t1".into(), ..Default::default() },
            ],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
                Arc { from: "p1".into(), to: "t1".into(), ..Default::default() },
                Arc { from: "t1".into(), to: "p0".into(), ..Default::default() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_clean_conservative_cycle_has_no_errors() {
        let model = cycle_model();
        let result = Validator::new(&model).validate();
        assert!(result.valid, "{:?}", result.errors);
        assert!(result.summary.conserved);
        assert_eq!(result.invariants, vec!["p0 + p1 == 1".to_string()]);
    }

    #[test]
    fn empty_net_is_an_error() {
        let model = Model { name: "empty".into(), ..Default::default() };
        let result = Validator::new(&model).validate();
        assert!(!result.valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, "structure");
    }

    #[test]
    fn negative_initial_marking_is_an_error() {
        let model = Model {
            name: "bad".into(),
            places: vec![Place { id: "p0".into(), initial: -1, ..Default::default() }],
            ..Default::default()
        };
        let result = Validator::new(&model).validate();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.message.contains("negative initial tokens")));
    }

    #[test]
    fn initial_over_capacity_is_an_error() {
        let model = Model {
            name: "over".into(),
            places: vec![Place { id: "p0".into(), initial: 5, capacity: 2, ..Default::default() }],
            ..Default::default()
        };
        let result = Validator::new(&model).validate();
        assert!(result.errors.iter().any(|e| e.message.contains("exceed capacity")));
    }

    #[test]
    fn disconnected_place_and_transition_warn() {
        let model = Model {
            name: "disc".into(),
            places: vec![Place { id: "p0".into(), initial: 1, ..Default::default() }],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: Vec::new(),
            ..Default::default()
        };
        let result = Validator::new(&model).validate();
        assert!(result.warnings.iter().any(|w| w.message.contains("Place 'p0' is not connected")));
        assert!(result.warnings.iter().any(|w| w.message.contains("Transition 't0' is not connected")));
    }

    #[test]
    fn unbounded_place_is_reported_as_an_error() {
        let model = Model {
            name: "unbounded".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
            ],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let result = Validator::new(&model).validate();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.category == "unbounded" && e.location == vec!["p1".to_string()]));
    }

    #[test]
    fn explicit_capacity_suppresses_the_unbounded_error() {
        let model = Model {
            name: "capped".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), capacity: 10, ..Default::default() },
            ],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let result = Validator::new(&model).validate();
        assert!(!result.errors.iter().any(|e| e.category == "unbounded"));
    }

    #[test]
    fn non_conserving_net_is_reported_but_not_invalid() {
        // t0 produces 2 tokens into p1 from 1 in p0: not conservative, but
        // still a valid, bounded (given capacity) net.
        let model = Model {
            name: "grows".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), capacity: 2, ..Default::default() },
            ],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), weight: 2, ..Default::default() },
            ],
            ..Default::default()
        };
        let result = Validator::new(&model).validate();
        assert!(!result.summary.conserved);
        assert!(result.valid);
    }

    #[test]
    fn unset_weight_defaults_to_one_and_is_not_flagged() {
        let model = Model {
            name: "zero".into(),
            places: vec![Place { id: "p0".into(), initial: 1, ..Default::default() }],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![Arc {
                from: "p0".into(),
                to: "t0".into(),
                typ: AT::Read,
                weight: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        // Shape B has no distinct representation for an explicit zero
        // weight: `weight: 0` always means "unset, defaults to 1" (see
        // `Arc::effective_weight`), unlike go-pflow's Shape A vector
        // weights, which could hold an explicit 0. So this must NOT be
        // flagged.
        let result = Validator::new(&model).validate();
        assert!(!result.errors.iter().any(|e| e.message.contains("non-positive weight")));
    }

    #[test]
    fn negative_weight_arc_is_an_error() {
        let model = Model {
            name: "negative".into(),
            places: vec![Place { id: "p0".into(), initial: 1, ..Default::default() }],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![Arc { from: "p0".into(), to: "t0".into(), weight: -1, ..Default::default() }],
            ..Default::default()
        };
        let result = Validator::new(&model).validate();
        assert!(result.errors.iter().any(|e| e.message.contains("non-positive weight")));
    }

    #[test]
    fn dangling_arc_endpoint_is_an_error() {
        let model = Model {
            name: "dangling".into(),
            places: vec![Place { id: "p0".into(), initial: 1, ..Default::default() }],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![Arc { from: "p0".into(), to: "does_not_exist".into(), ..Default::default() }],
            ..Default::default()
        };
        let result = Validator::new(&model).validate();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.message.contains("dangling")));
    }

    #[test]
    fn arc_between_two_places_is_dangling() {
        let model = Model {
            name: "same-kind".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
            ],
            arcs: vec![Arc { from: "p0".into(), to: "p1".into(), ..Default::default() }],
            ..Default::default()
        };
        let result = Validator::new(&model).validate();
        assert!(result.errors.iter().any(|e| e.message.contains("dangling")));
    }
}
