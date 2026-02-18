//! Fluent builder API for constructing Petri nets.

use std::collections::HashMap;

use crate::net::PetriNet;

/// Fluent builder for Petri net construction.
pub struct Builder {
    net: PetriNet,
    next_x: f64,
    place_y: f64,
    trans_y: f64,
}

impl Builder {
    /// Creates a new Builder.
    pub fn new() -> Self {
        Self {
            net: PetriNet::new(),
            next_x: 100.0,
            place_y: 100.0,
            trans_y: 200.0,
        }
    }

    /// Adds a place with the given label and initial token count.
    pub fn place(mut self, label: &str, initial: f64) -> Self {
        self.net.add_place(
            label,
            vec![initial],
            vec![],
            self.next_x,
            self.place_y,
            None,
        );
        self.next_x += 100.0;
        self
    }

    /// Adds a place with initial tokens and capacity limit.
    pub fn place_with_capacity(mut self, label: &str, initial: f64, capacity: f64) -> Self {
        self.net.add_place(
            label,
            vec![initial],
            vec![capacity],
            self.next_x,
            self.place_y,
            None,
        );
        self.next_x += 100.0;
        self
    }

    /// Adds a transition with the given label.
    pub fn transition(mut self, label: &str) -> Self {
        self.net
            .add_transition(label, "default", self.next_x, self.trans_y, None);
        self.next_x += 100.0;
        self
    }

    /// Adds a transition with a specific role.
    pub fn transition_with_role(mut self, label: &str, role: &str) -> Self {
        self.net
            .add_transition(label, role, self.next_x, self.trans_y, None);
        self.next_x += 100.0;
        self
    }

    /// Adds an arc from source to target with the given weight.
    pub fn arc(mut self, source: &str, target: &str, weight: f64) -> Self {
        self.net
            .add_arc(source, target, vec![weight], false);
        self
    }

    /// Adds an inhibitor arc from source to target.
    pub fn inhibitor_arc(mut self, source: &str, target: &str, weight: f64) -> Self {
        self.net
            .add_arc(source, target, vec![weight], true);
        self
    }

    /// Adds bidirectional arcs for a flow pattern: place -> transition -> place.
    pub fn flow(mut self, from_place: &str, transition: &str, to_place: &str, weight: f64) -> Self {
        self.net
            .add_arc(from_place, transition, vec![weight], false);
        self.net
            .add_arc(transition, to_place, vec![weight], false);
        self
    }

    /// Creates a sequential chain of places connected by transitions.
    ///
    /// Elements must be odd length: place, trans, place, trans, place...
    /// The first place gets `initial_tokens`, all others get 0.
    pub fn chain(mut self, initial_tokens: f64, elements: &[&str]) -> Self {
        if elements.len() < 3 || elements.len() % 2 == 0 {
            return self;
        }

        self = self.place(elements[0], initial_tokens);

        let mut i = 1;
        while i < elements.len() {
            let trans = elements[i];
            let next_place = elements[i + 1];

            self = self.transition(trans);
            self = self.place(next_place, 0.0);
            self.net
                .add_arc(elements[i - 1], trans, vec![1.0], false);
            self.net
                .add_arc(trans, next_place, vec![1.0], false);
            i += 2;
        }

        self
    }

    /// Creates a standard SIR epidemic model.
    pub fn sir(self, susceptible: f64, infected: f64, recovered: f64) -> Self {
        self.place("S", susceptible)
            .place("I", infected)
            .place("R", recovered)
            .transition("infect")
            .transition("recover")
            .arc("S", "infect", 1.0)
            .arc("I", "infect", 1.0)
            .arc("infect", "I", 2.0)
            .arc("I", "recover", 1.0)
            .arc("recover", "R", 1.0)
    }

    /// Returns the completed Petri net.
    pub fn done(self) -> PetriNet {
        self.net
    }

    /// Returns the net and a rates map initialized to the given default rate.
    pub fn with_rates(self, default_rate: f64) -> (PetriNet, HashMap<String, f64>) {
        let mut rates = HashMap::new();
        for label in self.net.transitions.keys() {
            rates.insert(label.clone(), default_rate);
        }
        (self.net, rates)
    }

    /// Returns the net and allows setting custom rates.
    pub fn with_custom_rates(
        self,
        rates: HashMap<String, f64>,
    ) -> (PetriNet, HashMap<String, f64>) {
        (self.net, rates)
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_builder() {
        let net = Builder::new()
            .place("A", 10.0)
            .place("B", 0.0)
            .transition("t1")
            .arc("A", "t1", 1.0)
            .arc("t1", "B", 1.0)
            .done();

        assert_eq!(net.places.len(), 2);
        assert_eq!(net.transitions.len(), 1);
        assert_eq!(net.arcs.len(), 2);
        assert_eq!(net.places["A"].token_count(), 10.0);
    }

    #[test]
    fn test_sir_builder() {
        let (net, rates) = PetriNet::build().sir(999.0, 1.0, 0.0).with_rates(1.0);

        assert_eq!(net.places.len(), 3);
        assert_eq!(net.transitions.len(), 2);
        assert_eq!(net.arcs.len(), 5);
        assert_eq!(net.places["S"].token_count(), 999.0);
        assert_eq!(net.places["I"].token_count(), 1.0);
        assert_eq!(net.places["R"].token_count(), 0.0);
        assert_eq!(rates["infect"], 1.0);
        assert_eq!(rates["recover"], 1.0);
    }

    #[test]
    fn test_chain_builder() {
        let net = Builder::new()
            .chain(1.0, &["Received", "start", "Processing", "finish", "Complete"])
            .done();

        assert_eq!(net.places.len(), 3);
        assert_eq!(net.transitions.len(), 2);
        assert_eq!(net.places["Received"].token_count(), 1.0);
        assert_eq!(net.places["Processing"].token_count(), 0.0);
        assert_eq!(net.places["Complete"].token_count(), 0.0);
    }

    #[test]
    fn test_with_rates() {
        let (net, rates) = Builder::new()
            .place("A", 10.0)
            .transition("t1")
            .transition("t2")
            .arc("A", "t1", 1.0)
            .with_rates(0.5);

        assert_eq!(rates.len(), 2);
        assert_eq!(rates["t1"], 0.5);
        assert_eq!(rates["t2"], 0.5);
        assert_eq!(net.places.len(), 1);
    }

    #[test]
    fn test_flow_builder() {
        let net = Builder::new()
            .place("input", 5.0)
            .place("output", 0.0)
            .transition("process")
            .flow("input", "process", "output", 1.0)
            .done();

        assert_eq!(net.arcs.len(), 2);
    }
}
