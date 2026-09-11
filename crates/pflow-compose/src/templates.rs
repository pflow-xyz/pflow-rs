//! Parameterized Petri net patterns, ported from go-pflow's `templates`
//! package. Each generator returns a plain [`PetriNet`] (Shape A), matching
//! the Go originals, which build directly on `petri.PetriNet` rather than
//! the Shape B `metamodel.Model` the rest of this crate otherwise uses.

use std::collections::HashMap;

use pflow_core::PetriNet;

/// Numeric parameters a template accepts, by name. Missing keys fall back
/// to the generator's own default, exactly as `getIntParam`/`getFloatParam`
/// do on the Go side.
pub type Params = HashMap<String, f64>;

fn get(params: &Params, name: &str, default: f64) -> f64 {
    params.get(name).copied().unwrap_or(default)
}

/// SIR epidemic model (Susceptible -> Infected -> Recovered).
pub fn sir(params: &Params) -> PetriNet {
    let population = get(params, "population", 1000.0);
    let initial_infected = get(params, "initial_infected", 10.0);
    let initial_susceptible = population - initial_infected;

    let mut net = PetriNet::new();
    net.add_place("S", vec![initial_susceptible], vec![], 100.0, 100.0, Some("Susceptible".into()));
    net.add_place("I", vec![initial_infected], vec![], 200.0, 100.0, Some("Infected".into()));
    net.add_place("R", vec![0.0], vec![], 300.0, 100.0, Some("Recovered".into()));

    net.add_transition("infection", "default", 150.0, 100.0, Some("Infection".into()));
    net.add_transition("recovery", "default", 250.0, 100.0, Some("Recovery".into()));

    net.add_arc("S", "infection", vec![1.0], false);
    net.add_arc("I", "infection", vec![1.0], false);
    net.add_arc("infection", "I", vec![2.0], false);

    net.add_arc("I", "recovery", vec![1.0], false);
    net.add_arc("recovery", "R", vec![1.0], false);

    net
}

/// SEIR epidemic model (Susceptible -> Exposed -> Infected -> Recovered).
pub fn seir(params: &Params) -> PetriNet {
    let population = get(params, "population", 1000.0);
    let initial_exposed = get(params, "initial_exposed", 5.0);
    let initial_infected = get(params, "initial_infected", 5.0);
    let initial_susceptible = population - initial_exposed - initial_infected;

    let mut net = PetriNet::new();
    net.add_place("S", vec![initial_susceptible], vec![], 100.0, 100.0, Some("Susceptible".into()));
    net.add_place("E", vec![initial_exposed], vec![], 200.0, 100.0, Some("Exposed".into()));
    net.add_place("I", vec![initial_infected], vec![], 300.0, 100.0, Some("Infected".into()));
    net.add_place("R", vec![0.0], vec![], 400.0, 100.0, Some("Recovered".into()));

    net.add_transition("exposure", "default", 150.0, 100.0, Some("Exposure".into()));
    net.add_transition("incubation", "default", 250.0, 100.0, Some("Incubation".into()));
    net.add_transition("recovery", "default", 350.0, 100.0, Some("Recovery".into()));

    net.add_arc("S", "exposure", vec![1.0], false);
    net.add_arc("I", "exposure", vec![1.0], false);
    net.add_arc("exposure", "E", vec![1.0], false);
    net.add_arc("exposure", "I", vec![1.0], false);

    net.add_arc("E", "incubation", vec![1.0], false);
    net.add_arc("incubation", "I", vec![1.0], false);

    net.add_arc("I", "recovery", vec![1.0], false);
    net.add_arc("recovery", "R", vec![1.0], false);

    net
}

/// Queueing system with arrivals, waiting queue and service.
pub fn queue(params: &Params) -> PetriNet {
    let servers = get(params, "servers", 1.0);
    let initial_queue = get(params, "initial_queue", 0.0);
    let queue_capacity = get(params, "queue_capacity", 0.0);

    let mut net = PetriNet::new();
    let cap = if queue_capacity > 0.0 { vec![queue_capacity] } else { vec![] };

    net.add_place("Queue", vec![initial_queue], cap, 100.0, 100.0, Some("Waiting Queue".into()));
    net.add_place("Processing", vec![0.0], vec![], 200.0, 100.0, Some("Being Processed".into()));
    net.add_place("Completed", vec![0.0], vec![], 300.0, 100.0, Some("Completed".into()));
    net.add_place("Servers", vec![servers], vec![], 200.0, 50.0, Some("Available Servers".into()));

    net.add_transition("arrive", "default", 50.0, 100.0, Some("Arrival".into()));
    net.add_transition("start_service", "default", 150.0, 100.0, Some("Start Service".into()));
    net.add_transition("complete", "default", 250.0, 100.0, Some("Complete".into()));

    net.add_arc("arrive", "Queue", vec![1.0], false);

    net.add_arc("Queue", "start_service", vec![1.0], false);
    net.add_arc("Servers", "start_service", vec![1.0], false);
    net.add_arc("start_service", "Processing", vec![1.0], false);

    net.add_arc("Processing", "complete", vec![1.0], false);
    net.add_arc("complete", "Completed", vec![1.0], false);
    net.add_arc("complete", "Servers", vec![1.0], false);

    net
}

/// Producer-consumer pattern with a bounded buffer.
pub fn producer_consumer(params: &Params) -> Result<PetriNet, String> {
    let buffer_size = get(params, "buffer_size", 10.0);
    let initial_buffer = get(params, "initial_buffer", 0.0);
    let producers = get(params, "producers", 1.0);
    let consumers = get(params, "consumers", 1.0);

    if initial_buffer > buffer_size {
        return Err(format!(
            "initial_buffer ({initial_buffer}) cannot exceed buffer_size ({buffer_size})"
        ));
    }

    let mut net = PetriNet::new();
    net.add_place("Buffer", vec![initial_buffer], vec![buffer_size], 200.0, 100.0, Some("Buffer".into()));
    net.add_place("ProducerReady", vec![producers], vec![], 100.0, 50.0, Some("Producers Ready".into()));
    net.add_place("ConsumerReady", vec![consumers], vec![], 300.0, 50.0, Some("Consumers Ready".into()));
    net.add_place("Consumed", vec![0.0], vec![], 400.0, 100.0, Some("Consumed Items".into()));

    net.add_transition("produce", "default", 150.0, 100.0, Some("Produce".into()));
    net.add_transition("consume", "default", 250.0, 100.0, Some("Consume".into()));

    net.add_arc("ProducerReady", "produce", vec![1.0], false);
    net.add_arc("produce", "Buffer", vec![1.0], false);
    net.add_arc("produce", "ProducerReady", vec![1.0], false);

    net.add_arc("Buffer", "consume", vec![1.0], false);
    net.add_arc("ConsumerReady", "consume", vec![1.0], false);
    net.add_arc("consume", "Consumed", vec![1.0], false);
    net.add_arc("consume", "ConsumerReady", vec![1.0], false);

    Ok(net)
}

/// A sequential workflow with `stages` steps.
pub fn workflow(params: &Params) -> Result<PetriNet, String> {
    let stages = get(params, "stages", 3.0) as i64;
    let initial_items = get(params, "initial_items", 10.0);

    if stages < 2 {
        return Err("stages must be >= 2".to_string());
    }

    let mut net = PetriNet::new();
    for i in 0..=stages {
        let (initial, label) = if i == 0 {
            (initial_items, "Start".to_string())
        } else if i == stages {
            (0.0, "Complete".to_string())
        } else {
            (0.0, format!("Stage {i}"))
        };
        let place_name = format!("Stage{i}");
        let x = 100.0 + (i as f64) * 100.0;
        net.add_place(place_name.as_str(), vec![initial], vec![], x, 100.0, Some(label));
    }

    for i in 0..stages {
        let trans_name = format!("process{}", i + 1);
        let label = format!("Process Step {}", i + 1);
        let x = 150.0 + (i as f64) * 100.0;
        net.add_transition(trans_name.as_str(), "default", x, 100.0, Some(label));

        let src = format!("Stage{i}");
        let dst = format!("Stage{}", i + 1);
        net.add_arc(src, trans_name.as_str(), vec![1.0], false);
        net.add_arc(trans_name.as_str(), dst, vec![1.0], false);
    }

    Ok(net)
}

/// Names every registered template, matching `templates.List()`.
pub fn list() -> Vec<&'static str> {
    vec!["sir", "seir", "queue", "producer-consumer", "workflow"]
}

/// Generates a net by template name, matching `templates.Get(name).Generate(params)`.
pub fn generate(name: &str, params: &Params) -> Result<PetriNet, String> {
    match name {
        "sir" => Ok(sir(params)),
        "seir" => Ok(seir(params)),
        "queue" => Ok(queue(params)),
        "producer-consumer" => producer_consumer(params),
        "workflow" => workflow(params),
        other => Err(format!("unknown template: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sir_defaults_match_go_reference() {
        let net = sir(&Params::new());
        assert_eq!(net.places["S"].initial, vec![990.0]);
        assert_eq!(net.places["I"].initial, vec![10.0]);
        assert_eq!(net.places["R"].initial, vec![0.0]);
        assert_eq!(net.arcs.len(), 5);
    }

    #[test]
    fn seir_places_and_transitions() {
        let net = seir(&Params::new());
        assert_eq!(net.places.len(), 4);
        assert_eq!(net.transitions.len(), 3);
    }

    #[test]
    fn queue_unbounded_by_default() {
        let net = queue(&Params::new());
        assert!(net.places["Queue"].capacity.is_empty());
    }

    #[test]
    fn queue_bounded_when_capacity_given() {
        let mut params = Params::new();
        params.insert("queue_capacity".to_string(), 5.0);
        let net = queue(&params);
        assert_eq!(net.places["Queue"].capacity, vec![5.0]);
    }

    #[test]
    fn producer_consumer_rejects_overfull_initial_buffer() {
        let mut params = Params::new();
        params.insert("buffer_size".to_string(), 2.0);
        params.insert("initial_buffer".to_string(), 3.0);
        assert!(producer_consumer(&params).is_err());
    }

    #[test]
    fn workflow_builds_n_plus_one_places() {
        let mut params = Params::new();
        params.insert("stages".to_string(), 4.0);
        let net = workflow(&params).unwrap();
        assert_eq!(net.places.len(), 5);
        assert_eq!(net.transitions.len(), 4);
    }

    #[test]
    fn workflow_rejects_fewer_than_two_stages() {
        let mut params = Params::new();
        params.insert("stages".to_string(), 1.0);
        assert!(workflow(&params).is_err());
    }

    #[test]
    fn generate_dispatches_by_name_and_rejects_unknown() {
        assert!(generate("sir", &Params::new()).is_ok());
        assert!(generate("nope", &Params::new()).is_err());
    }

    #[test]
    fn list_names_every_template() {
        assert_eq!(list().len(), 5);
    }
}
