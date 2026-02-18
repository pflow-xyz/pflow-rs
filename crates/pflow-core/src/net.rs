//! Core Petri net data structures.
//!
//! A Petri net consists of Places (states), Transitions (events), and Arcs (connections).

use std::collections::HashMap;

/// A state in a Petri net that can hold tokens.
#[derive(Debug, Clone)]
pub struct Place {
    pub label: String,
    pub initial: Vec<f64>,
    pub capacity: Vec<f64>,
    pub x: f64,
    pub y: f64,
    pub label_text: Option<String>,
}

impl Place {
    /// Creates a new Place.
    pub fn new(
        label: impl Into<String>,
        initial: Vec<f64>,
        capacity: Vec<f64>,
        x: f64,
        y: f64,
        label_text: Option<String>,
    ) -> Self {
        Self {
            label: label.into(),
            initial,
            capacity,
            x,
            y,
            label_text,
        }
    }

    /// Returns the sum of all tokens in this place.
    pub fn token_count(&self) -> f64 {
        if self.initial.is_empty() {
            0.0
        } else {
            self.initial.iter().sum()
        }
    }
}

/// An event that can occur in a Petri net.
#[derive(Debug, Clone)]
pub struct Transition {
    pub label: String,
    pub role: String,
    pub x: f64,
    pub y: f64,
    pub label_text: Option<String>,
}

impl Transition {
    pub fn new(
        label: impl Into<String>,
        role: impl Into<String>,
        x: f64,
        y: f64,
        label_text: Option<String>,
    ) -> Self {
        Self {
            label: label.into(),
            role: role.into(),
            x,
            y,
            label_text,
        }
    }
}

/// A directed connection between a Place and a Transition.
#[derive(Debug, Clone)]
pub struct Arc {
    pub source: String,
    pub target: String,
    pub weight: Vec<f64>,
    pub inhibit_transition: bool,
}

impl Arc {
    pub fn new(
        source: impl Into<String>,
        target: impl Into<String>,
        weight: Vec<f64>,
        inhibit_transition: bool,
    ) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            weight,
            inhibit_transition,
        }
    }

    /// Returns the sum of all weight values. Returns 1.0 if empty.
    pub fn weight_sum(&self) -> f64 {
        if self.weight.is_empty() {
            1.0
        } else {
            self.weight.iter().sum()
        }
    }
}

/// State map type: place label -> token count.
pub type State = HashMap<String, f64>;

/// A complete Petri net model.
#[derive(Debug, Clone)]
pub struct PetriNet {
    pub places: HashMap<String, Place>,
    pub transitions: HashMap<String, Transition>,
    pub arcs: Vec<Arc>,
    pub token: Vec<String>,
}

impl PetriNet {
    /// Creates an empty Petri net.
    pub fn new() -> Self {
        Self {
            places: HashMap::new(),
            transitions: HashMap::new(),
            arcs: Vec::new(),
            token: Vec::new(),
        }
    }

    /// Starts building a Petri net with the fluent API.
    pub fn build() -> super::builder::Builder {
        super::builder::Builder::new()
    }

    /// Adds a place to the net.
    pub fn add_place(
        &mut self,
        label: impl Into<String>,
        initial: Vec<f64>,
        capacity: Vec<f64>,
        x: f64,
        y: f64,
        label_text: Option<String>,
    ) -> &Place {
        let label = label.into();
        let p = Place::new(label.clone(), initial, capacity, x, y, label_text);
        self.places.insert(label.clone(), p);
        &self.places[&label]
    }

    /// Adds a transition to the net.
    pub fn add_transition(
        &mut self,
        label: impl Into<String>,
        role: impl Into<String>,
        x: f64,
        y: f64,
        label_text: Option<String>,
    ) -> &Transition {
        let label = label.into();
        let t = Transition::new(label.clone(), role, x, y, label_text);
        self.transitions.insert(label.clone(), t);
        &self.transitions[&label]
    }

    /// Adds an arc to the net.
    pub fn add_arc(
        &mut self,
        source: impl Into<String>,
        target: impl Into<String>,
        weight: Vec<f64>,
        inhibit_transition: bool,
    ) {
        let a = Arc::new(source, target, weight, inhibit_transition);
        self.arcs.push(a);
    }

    /// Returns all arcs that lead into the given transition.
    pub fn input_arcs(&self, transition_label: &str) -> Vec<&Arc> {
        self.arcs
            .iter()
            .filter(|a| a.target == transition_label)
            .collect()
    }

    /// Returns all arcs that lead out from the given transition.
    pub fn output_arcs(&self, transition_label: &str) -> Vec<&Arc> {
        self.arcs
            .iter()
            .filter(|a| a.source == transition_label)
            .collect()
    }

    /// Creates a state map from the net's initial state.
    /// If `custom_state` is provided, those values override defaults.
    pub fn set_state(&self, custom_state: Option<&State>) -> State {
        let mut state = State::new();
        for (label, place) in &self.places {
            if let Some(custom) = custom_state {
                if let Some(&v) = custom.get(label) {
                    state.insert(label.clone(), v);
                    continue;
                }
            }
            state.insert(label.clone(), place.token_count());
        }
        state
    }

    /// Creates a rate map for all transitions.
    /// If `custom_rates` is provided, those values override the default of 1.0.
    pub fn set_rates(&self, custom_rates: Option<&HashMap<String, f64>>) -> HashMap<String, f64> {
        let mut rates = HashMap::new();
        for label in self.transitions.keys() {
            if let Some(custom) = custom_rates {
                if let Some(&v) = custom.get(label) {
                    rates.insert(label.clone(), v);
                    continue;
                }
            }
            rates.insert(label.clone(), 1.0);
        }
        rates
    }
}

impl Default for PetriNet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_place_token_count() {
        let p = Place::new("test", vec![3.0, 7.0], vec![], 0.0, 0.0, None);
        assert_eq!(p.token_count(), 10.0);

        let empty = Place::new("empty", vec![], vec![], 0.0, 0.0, None);
        assert_eq!(empty.token_count(), 0.0);
    }

    #[test]
    fn test_arc_weight_sum() {
        let a = Arc::new("a", "b", vec![2.0, 3.0], false);
        assert_eq!(a.weight_sum(), 5.0);

        let empty = Arc::new("a", "b", vec![], false);
        assert_eq!(empty.weight_sum(), 1.0);
    }

    #[test]
    fn test_petri_net_basic() {
        let mut net = PetriNet::new();
        net.add_place("S", vec![999.0], vec![], 0.0, 0.0, None);
        net.add_place("I", vec![1.0], vec![], 0.0, 0.0, None);
        net.add_transition("infect", "default", 0.0, 0.0, None);
        net.add_arc("S", "infect", vec![1.0], false);
        net.add_arc("I", "infect", vec![1.0], false);
        net.add_arc("infect", "I", vec![2.0], false);

        assert_eq!(net.places.len(), 2);
        assert_eq!(net.transitions.len(), 1);
        assert_eq!(net.arcs.len(), 3);

        let inputs = net.input_arcs("infect");
        assert_eq!(inputs.len(), 2);

        let outputs = net.output_arcs("infect");
        assert_eq!(outputs.len(), 1);
    }

    #[test]
    fn test_set_state() {
        let mut net = PetriNet::new();
        net.add_place("A", vec![10.0], vec![], 0.0, 0.0, None);
        net.add_place("B", vec![0.0], vec![], 0.0, 0.0, None);

        let state = net.set_state(None);
        assert_eq!(state["A"], 10.0);
        assert_eq!(state["B"], 0.0);

        let mut custom = State::new();
        custom.insert("A".into(), 5.0);
        let state = net.set_state(Some(&custom));
        assert_eq!(state["A"], 5.0);
        assert_eq!(state["B"], 0.0);
    }
}
