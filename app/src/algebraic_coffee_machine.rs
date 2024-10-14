use pflow_metamodel::dsl::{Builder, Dsl};
use pflow_metamodel::petri_net;
use std::collections::HashSet;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

/// Represents a place in the Petri net.
#[derive(Debug, Clone, EnumIter, Eq, PartialEq, Hash)]
enum State {
    Water,
    BoiledWater,
    CoffeeBeans,
    GroundCoffee,
    Filter,
    CoffeeInPot,
    Pending,
    Sent,
    Payment,
    Cup,
}

impl State {
    fn is_initial(&self) -> bool {
        match self {
            State::Water | State::CoffeeBeans | State::Filter | State::Cup | State::Pending => true,
            _ => false,
        }
    }
}

/// Represents a transition in the Petri net.
#[derive(Debug, Clone, Eq, PartialEq, Hash, EnumIter)]
enum Transition {
    BoilWater,
    GrindBeans,
    BrewCoffee,
    PourCoffee,
    Send,
    Credit,
}

/// Represents an arc in the Petri net.
#[derive(Debug, Clone)]
struct Arc {
    from: Node,
    to: Node,
}

/// Represents a node in the Petri net, which can be either a Place or a Transition.
#[derive(Debug, Clone)]
enum Node {
    Place(State),
    Transition(Transition),
}

#[derive(Debug, Clone)]
struct Guard {
    from: Node,
    to: Node,
}

#[derive(Debug, Clone)]
struct CoffeeMachine {
    state: HashSet<State>,
    arcs: Vec<Arc>,
    guards: Vec<Guard>,
}

impl CoffeeMachine {
    fn new() -> Self {
        let state = State::iter().filter(|p| p.is_initial()).collect();

        let arcs = vec![
            Arc {
                from: Node::Place(State::Water),
                to: Node::Transition(Transition::BoilWater),
            },
            Arc {
                from: Node::Transition(Transition::BoilWater),
                to: Node::Place(State::BoiledWater),
            },
            Arc {
                from: Node::Place(State::CoffeeBeans),
                to: Node::Transition(Transition::GrindBeans),
            },
            Arc {
                from: Node::Transition(Transition::GrindBeans),
                to: Node::Place(State::GroundCoffee),
            },
            Arc {
                from: Node::Place(State::BoiledWater),
                to: Node::Transition(Transition::BrewCoffee),
            },
            Arc {
                from: Node::Place(State::GroundCoffee),
                to: Node::Transition(Transition::BrewCoffee),
            },
            Arc {
                from: Node::Place(State::Filter),
                to: Node::Transition(Transition::BrewCoffee),
            },
            Arc {
                from: Node::Transition(Transition::BrewCoffee),
                to: Node::Place(State::CoffeeInPot),
            },
            Arc {
                from: Node::Place(State::CoffeeInPot),
                to: Node::Transition(Transition::PourCoffee),
            },
            Arc {
                from: Node::Place(State::Cup),
                to: Node::Transition(Transition::PourCoffee),
            },
            Arc {
                from: Node::Place(State::Pending),
                to: Node::Transition(Transition::Send),
            },
            Arc {
                from: Node::Transition(Transition::Send),
                to: Node::Place(State::Sent),
            },
            Arc {
                from: Node::Place(State::Sent),
                to: Node::Transition(Transition::Credit),
            },
            Arc {
                from: Node::Transition(Transition::Credit),
                to: Node::Place(State::Payment),
            },
        ];

        let guards = vec![
            Guard {
                from:Node::Transition(Transition::PourCoffee),
                to:  Node::Place(State::Payment),
            },
        ];

        CoffeeMachine {
            state,
            arcs,
            guards,
        }
    }

    fn execute_process(&mut self) {
        // REVIEW: there is room to add parallel execution of transitions here
        self.execute_transition(Transition::BoilWater);
        self.execute_transition(Transition::GrindBeans);
        self.execute_transition(Transition::BrewCoffee);
        self.execute_transition(Transition::Send);
        self.execute_transition(Transition::Credit);
        self.execute_transition(Transition::PourCoffee);
    }

    fn prepare_transition(&self, transition: &Transition) -> (bool, Vec<State>, Vec<State>) {
        let mut places_to_remove = vec![];
        let mut places_to_add = vec![];

        // Check guards
        for guard in &self.guards {
            match (&guard.from, &guard.to) {
                (Node::Place(p), Node::Transition(t)) if t == transition => {
                    if !self.state.contains(p) {
                        return (false, places_to_remove, places_to_add);
                    }
                }
                (Node::Transition(t), Node::Place(p)) if t == transition => {
                    if !self.state.contains(p) {
                        return (false, places_to_remove, places_to_add);
                    }
                }
                _ => continue,
            }
        }

        // Update places based on arcs
        for arc in &self.arcs {
            match &arc.from {
                Node::Transition(t) if t == transition => {
                    if let Node::Place(p) = &arc.to {
                        places_to_add.push(p.clone());
                    }
                }
                Node::Place(p) if self.state.contains(p) => {
                    if let Node::Transition(t) = &arc.to {
                        if t == transition {
                            places_to_remove.push(p.clone());
                        }
                    }
                }
                _ => continue,
            }
        }

        (true, places_to_remove, places_to_add)
    }

    fn execute_transition(&mut self, transition: Transition) {
        println!("Executing transition: {:?}", transition);

        let (can_execute, places_to_remove, places_to_add) = self.prepare_transition(&transition);

        if !can_execute {
            println!("Guard condition not met for transition: {:?}", transition);
            return;
        }

        for place in places_to_remove {
            self.state.remove(&place);
        }

        self.state.extend(places_to_add);
    }

    fn to_model(&self) -> pflow_metamodel::Model {
        let mut net = petri_net::PetriNet::new();
        let mut b = Builder { net: &mut net };
        b.model_type("petriNet");
        let mut x = 100;
        let mut y = 100;
        let margin = 60;

        // Add places
        for state in State::iter() {
            let initial_tokens = if state.is_initial() { 1 } else { 0 };
            b.cell(&format!("{:?}", state), Some(initial_tokens), Some(1), x, y);
            x += margin;
        }
        x = 100;
        y += margin;

        // Add transitions
        for transition in Transition::iter() {
            b.func(&format!("{:?}", transition), "default", x, y);
            x += margin;
        }

        // Add arcs
        for arc in &self.arcs {
            match (&arc.from, &arc.to) {
                (Node::Place(from), Node::Transition(to)) => {
                    b.arrow(&format!("{:?}", from), &format!("{:?}", to), 1);
                }
                (Node::Transition(from), Node::Place(to)) => {
                    b.arrow(&format!("{:?}", from), &format!("{:?}", to), 1);
                }
                _ => {}
            }
        }

        // Add guards
        for guard in &self.guards {
            match (&guard.from, &guard.to) {
                (Node::Place(from), Node::Transition(to)) => {
                    b.guard(&format!("{:?}", from), &format!("{:?}", to), 1);
                }
                (Node::Transition(from), Node::Place(to)) => {
                    b.guard(&format!("{:?}", from), &format!("{:?}", to), 1);
                }
                _ => {}
            }
        }

        let vm = Box::new(b.as_vasm());
        pflow_metamodel::Model { net, vm }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_petri_net_process() {
        let mut cm = CoffeeMachine::new();
        cm.execute_process();
        println!("{:?}", cm.state);
    }

    #[test]
    fn test_petri_net_model() {
        let cm = CoffeeMachine::new();
        let model = cm.to_model();
        println!("{:?}", model);
        println!(
            "coffee_machine https://pflow.dev/?z={}",
            cm.to_model().net.to_zblob().base64_zipped
        );
    }
}
