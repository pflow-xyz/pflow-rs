use pflow_metamodel::*;

petri_net!( CoffeeMachine {
    {
        "modelType": "petriNet",
        "version": "v0",
        "places": {
            "Water":        { "offset": 0, "initial": 1, "capacity": 1, "x": 100, "y": 200 },
            "BoiledWater":  { "offset": 1, "initial": 0, "capacity": 1, "x": 260, "y": 200 },
            "CoffeeBeans":  { "offset": 2, "initial": 1, "capacity": 1, "x": 376, "y": 434 },
            "GroundCoffee": { "offset": 3, "initial": 0, "capacity": 1, "x": 541, "y": 469 },
            "Filter":       { "offset": 4, "initial": 1, "capacity": 1, "x": 660, "y": 200 },
            "CoffeeInPot":  { "offset": 5, "initial": 0, "capacity": 1, "x": 740, "y": 200 },
            "Cup":          { "offset": 6, "initial": 1, "capacity": 1, "x": 900, "y": 200 }
        },
        "transitions": {
            "boil_water":  { "offset": 0, "role": "default", "x": 191, "y": 489 },
            "brew_coffee": { "offset": 1, "role": "default", "x": 548, "y": 118 },
            "grind_beans": { "offset": 2, "role": "default", "x": 420, "y": 200 },
            "pour_coffee": { "offset": 3, "role": "default", "x": 820, "y": 200 }
        },
        "arcs": [
            { "source": "Water",        "target": "boil_water",   "weight": 1 },
            { "source": "boil_water",   "target": "BoiledWater",  "weight": 1 },
            { "source": "CoffeeBeans",  "target": "grind_beans",  "weight": 1 },
            { "source": "grind_beans",  "target": "GroundCoffee", "weight": 1 },
            { "source": "BoiledWater",  "target": "brew_coffee",  "weight": 1 },
            { "source": "GroundCoffee", "target": "brew_coffee",  "weight": 1 },
            { "source": "Filter",       "target": "brew_coffee",  "weight": 1 },
            { "source": "brew_coffee",  "target": "CoffeeInPot",  "weight": 1 },
            { "source": "CoffeeInPot",  "target": "pour_coffee",  "weight": 1 },
            { "source": "Cup",          "target": "pour_coffee",  "weight": 1 }
        ]
    }
});

impl State for CoffeeMachine {
    fn evaluate_preconditions(&self) -> Result<bool, StateMachineError> {
        let mut state = self.state.lock().expect("lock failed");
        for (label, place) in &self.model.net.places {
            if let Some(initial) = place.initial {
                if initial != 0 {
                    let measurement = self.evaluate_resource(label)?;
                    let offset = usize::try_from(place.offset).expect("offset conversion failed");
                    state[offset] = measurement;
                }
            }
        }
        Ok(true)
    }

    fn evaluate_resource(&self, label: &str) -> Result<i32, StateMachineError> {
        println!("Measuring resource: {label}");
        match label {
            "Water" | "CoffeeBeans" | "Filter" | "Cup" => Ok(1),
            _ => Ok(0),
        }
    }
}

#[derive(Debug)]
struct Context {
    pub msg: String,
}

impl Process<Context> for CoffeeMachine {
    fn run(&self, context: Context) -> Vec<Event<Context>> {
        let action = self.next_action();
        if action.is_empty() || !self.evaluate_preconditions().unwrap_or(false) {
            vec![]
        } else {
            let evt = Event {
                action: "__begin__".to_string(),
                seq: 0,
                state: self.model.vm.initial_vector(),
                data: context,
            };
            self.run_impl(Some(&action[0]), None, vec![evt])
        }
    }

    fn run_impl(
        &self,
        action: Option<&str>,
        seq: Option<u64>,
        mut event_log: Vec<Event<Context>>,
    ) -> Vec<Event<Context>> {
        let mut current_action = action.map(ToString::to_string);
        let mut current_seq = seq.unwrap_or(0) + 1;

        while let Some(ref action) = current_action {
            if let Some(transaction) = self.process_action(action, current_seq) {
                event_log.push(transaction);
            } else {
                break;
            }
            current_action = self.next_action().first().cloned();
            current_seq += 1;
        }

        let evt = Event {
            action: "__end__".to_string(),
            seq: current_seq + 1,
            state: self.state.lock().expect("lock failed").clone(),
            data: Context { msg: "Coffee machine stopped".to_string() },
        };
        event_log.push(evt);
        event_log
    }

    /// calculate the next action to be executed based on the current state
    /// transform state before calling execute_action
    fn process_action(&self, action: &str, seq: u64) -> Option<Event<Context>> {
        let mut state = self.state.lock().expect("lock failed");
        let res = self.model.vm.transform(&state, action, 1);

        if res.is_ok() {
            *state = res.output;
            let evt = Event {
                action: action.to_string(),
                seq,
                state: state.clone(),
                data: Context { msg: format!("completed! #{seq}: {action}") },
            };
            let transaction = self.execute_action(evt);

            match transaction {
                Err(e) => {
                    let evt = Event {
                        action: format!("__error__::{action}::{e:?}"),
                        seq,
                        state: state.clone(),
                        data: Context { msg: "Action failed".to_string() },
                    };
                    Some(evt)
                }
                Ok(transaction) => Some(transaction),
            }
        } else {
            None
        }
    }

    fn next_action(&self) -> Vec<String> {
        let state = self.state.lock().expect("lock failed");
        for action in self.model.vm.transitions.keys() {
            if self.model.vm.transform(&state, action, 1).is_ok() {
                return vec![action.clone()];
            }
        }
        vec![]
    }

    fn execute_action(&self, event: Event<Context>) -> Result<Event<Context>, StateMachineError> {
        println!("{} - Executing action: {}", event.seq, event.action);
        match event.action.as_str() {
            "boil_water" | "brew_coffee" | "grind_beans" | "pour_coffee" => Ok(event),
            _ => Err(StateMachineError::InvalidAction),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coffee_machine() {
        let cm = CoffeeMachine::new();
        println!("https://pflow.dev/?z={}", cm.model.net.to_zblob().base64_zipped);
        for event in cm.run(Context { msg: "Start".to_string() }) {
            println!("{event:?}");
        }
    }
}