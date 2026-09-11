//! Evaluation-net derivations: net -> net transforms with no domain
//! knowledge in them, ported from go-pflow's `derive` package.
//!
//! A declared net states exact semantics; an analysis often wants a
//! deliberately different net — the same system under a semantic stance:
//!
//! - [`add_catalyzed_copy`]: a priority prior. Duplicate a transition and
//!   gate the copy's rate on a pattern of places (read arcs), so flow tilts
//!   toward the move whenever the pattern holds.
//! - [`replace_with_hazard`]: a threshold prior. Continuous (mass-action)
//!   semantics cannot express "fires when a count reaches N"; the honest
//!   continuous form of a threshold event is a hazard — a competing-risk
//!   drain from the condition place.
//! - [`write_only_places`] / [`drop_places`]: dead-coordinate removal. A
//!   place no arc reads is provably inert under mass action.
//! - [`drop_readbacks`]: convert a catalytic (read-and-return) transition
//!   into a consuming one.

use std::collections::{BTreeMap, HashSet};

use pflow_core::{Arc, PetriNet};

/// Duplicates transition `src` as `name` — same input and output arcs, same
/// weights — and additionally gates the copy on the catalyst places via
/// read arcs (an input and an equal output per catalyst, weight per the
/// map). Under mass action the copy's rate is then multiplied by the
/// catalyst markings.
///
/// The copy is added with no rate of its own; set its strength in the rates
/// map handed to the solver.
pub fn add_catalyzed_copy(
    net: &mut PetriNet,
    src: &str,
    name: &str,
    catalysts: &BTreeMap<String, f64>,
) -> Result<(), String> {
    if !net.transitions.contains_key(src) {
        return Err(format!("derive: transition {src:?} not in net"));
    }
    if net.transitions.contains_key(name) {
        return Err(format!("derive: transition {name:?} already exists"));
    }
    if net.places.contains_key(name) {
        return Err(format!("derive: {name:?} already names a place"));
    }
    for p in catalysts.keys() {
        if !net.places.contains_key(p) {
            return Err(format!("derive: catalyst place {p:?} not in net"));
        }
    }

    let role = net.transitions[src].role.clone();
    net.add_transition(name, role, 0.0, 0.0, None);

    let inputs: Vec<Arc> = net.input_arcs(src).into_iter().cloned().collect();
    for a in inputs {
        net.add_arc(a.source.clone(), name, a.weight.clone(), a.inhibit_transition);
    }
    let outputs: Vec<Arc> = net.output_arcs(src).into_iter().cloned().collect();
    for a in outputs {
        net.add_arc(name, a.target.clone(), a.weight.clone(), a.inhibit_transition);
    }
    for (p, &w) in catalysts {
        net.add_arc(p.clone(), name, vec![w], false);
        net.add_arc(name, p.clone(), vec![w], false);
    }
    Ok(())
}

/// Rewires transition `t` into a plain drain: every arc touching `t` is
/// removed and replaced by `source -> t -> target`, weight 1. `target` is
/// created (empty) if the net does not declare it.
pub fn replace_with_hazard(net: &mut PetriNet, t: &str, source: &str, target: &str) -> Result<(), String> {
    if !net.transitions.contains_key(t) {
        return Err(format!("derive: transition {t:?} not in net"));
    }
    if !net.places.contains_key(source) {
        return Err(format!("derive: source place {source:?} not in net"));
    }
    if !net.places.contains_key(target) {
        net.add_place(target, vec![], vec![], 0.0, 0.0, None);
    }
    net.arcs.retain(|a| a.source != t && a.target != t);
    net.add_arc(source, t, vec![1.0], false);
    net.add_arc(t, target, vec![1.0], false);
    Ok(())
}

/// Reports every place that no arc reads — nothing consumes from it, no
/// read loop touches it, no inhibitor tests it. Such a place is provably
/// inert under mass action. Sorted.
pub fn write_only_places(net: &PetriNet) -> Vec<String> {
    let mut read: HashSet<&str> = HashSet::new();
    for a in &net.arcs {
        if net.places.contains_key(&a.source) {
            read.insert(a.source.as_str());
        }
    }
    let mut out: Vec<String> = net
        .places
        .keys()
        .filter(|label| !read.contains(label.as_str()))
        .cloned()
        .collect();
    out.sort();
    out
}

/// Removes the named places and every arc touching them. Missing names are
/// ignored.
pub fn drop_places(net: &mut PetriNet, places: &[&str]) {
    let drop: HashSet<&str> = places.iter().copied().collect();
    for p in &drop {
        net.places.remove(*p);
    }
    net.arcs.retain(|a| !drop.contains(a.source.as_str()) && !drop.contains(a.target.as_str()));
}

/// Removes the named transitions and every arc touching them. Missing names
/// are ignored.
///
/// Ordering trap this exists to avoid: dropping a transition's input PLACES
/// instead leaves the transition with no inputs, and under mass action a
/// transition with no inputs is a constant source — remove the transition
/// first, then its private places.
pub fn drop_transitions(net: &mut PetriNet, transitions: &[&str]) {
    let drop: HashSet<&str> = transitions.iter().copied().collect();
    for t in &drop {
        net.transitions.remove(*t);
    }
    net.arcs.retain(|a| !drop.contains(a.source.as_str()) && !drop.contains(a.target.as_str()));
}

/// Removes, for transition `t`, every output arc returning to one of `t`'s
/// own input places — converting a catalytic detector into a consuming
/// one, so firing destroys the pattern it detected.
pub fn drop_readbacks(net: &mut PetriNet, t: &str) -> Result<(), String> {
    if !net.transitions.contains_key(t) {
        return Err(format!("derive: transition {t:?} not in net"));
    }
    let inputs: HashSet<String> = net.input_arcs(t).into_iter().map(|a| a.source.clone()).collect();
    net.arcs.retain(|a| !(a.source == t && inputs.contains(&a.target)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain_net() -> PetriNet {
        let mut net = PetriNet::new();
        net.add_place("a", vec![5.0], vec![], 0.0, 0.0, None);
        net.add_place("b", vec![0.0], vec![], 0.0, 0.0, None);
        net.add_place("watch", vec![1.0], vec![], 0.0, 0.0, None);
        net.add_transition("t", "default", 0.0, 0.0, None);
        net.add_arc("a", "t", vec![1.0], false);
        net.add_arc("t", "b", vec![1.0], false);
        net.add_arc("watch", "t", vec![1.0], false);
        net.add_arc("t", "watch", vec![1.0], false); // readback
        net
    }

    #[test]
    fn add_catalyzed_copy_duplicates_arcs_and_gates_on_catalyst() {
        let mut net = chain_net();
        net.add_place("threat", vec![1.0], vec![], 0.0, 0.0, None);
        let mut cat = BTreeMap::new();
        cat.insert("threat".to_string(), 1.0);
        add_catalyzed_copy(&mut net, "t", "t_prime", &cat).unwrap();
        assert!(net.transitions.contains_key("t_prime"));
        assert_eq!(net.input_arcs("t_prime").len(), 3); // a, watch, threat
        assert_eq!(net.output_arcs("t_prime").len(), 3); // b, watch, threat
    }

    #[test]
    fn add_catalyzed_copy_rejects_unknown_source() {
        let mut net = chain_net();
        let cat = BTreeMap::new();
        assert!(add_catalyzed_copy(&mut net, "nope", "x", &cat).is_err());
    }

    #[test]
    fn replace_with_hazard_rewires_to_a_plain_drain() {
        let mut net = chain_net();
        replace_with_hazard(&mut net, "t", "a", "sink").unwrap();
        assert_eq!(net.input_arcs("t").len(), 1);
        assert_eq!(net.output_arcs("t").len(), 1);
        assert!(net.places.contains_key("sink"));
    }

    #[test]
    fn write_only_places_finds_unread_places() {
        let mut net = PetriNet::new();
        net.add_place("in", vec![1.0], vec![], 0.0, 0.0, None);
        net.add_place("out", vec![0.0], vec![], 0.0, 0.0, None);
        net.add_transition("t", "default", 0.0, 0.0, None);
        net.add_arc("in", "t", vec![1.0], false);
        net.add_arc("t", "out", vec![1.0], false);
        assert_eq!(write_only_places(&net), vec!["out".to_string()]);
    }

    #[test]
    fn drop_places_removes_place_and_touching_arcs() {
        let mut net = chain_net();
        drop_places(&mut net, &["watch"]);
        assert!(!net.places.contains_key("watch"));
        assert!(net.arcs.iter().all(|a| a.source != "watch" && a.target != "watch"));
    }

    #[test]
    fn drop_transitions_removes_transition_and_touching_arcs() {
        let mut net = chain_net();
        drop_transitions(&mut net, &["t"]);
        assert!(!net.transitions.contains_key("t"));
        assert!(net.arcs.is_empty());
    }

    #[test]
    fn drop_readbacks_removes_only_the_output_that_returns_to_an_input() {
        let mut net = chain_net();
        drop_readbacks(&mut net, "t").unwrap();
        assert_eq!(net.output_arcs("t").len(), 1); // only t -> b remains
        assert_eq!(net.input_arcs("t").len(), 2); // inputs untouched
    }
}
