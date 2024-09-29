use pflow_metamodel::*;

state_machine!( CoffeeMachine {
    ModelType::PetriNet;
    Water --> boil_water;
    boil_water --> BoiledWater;
    CoffeeBeans --> grind_beans;
    grind_beans --> GroundCoffee;
    BoiledWater --> brew_coffee;
    GroundCoffee --> brew_coffee;
    Filter --> brew_coffee;
    brew_coffee --> CoffeeInPot;
    CoffeeInPot --> pour_coffee;
    Cup --> pour_coffee;
});

impl State for CoffeeMachine {
    fn evaluate_preconditions(&self) -> Result<bool, StateMachineError> {
        let mut state = self.state.lock().expect("lock failed");
        for (label, place) in &self.model.net.places {
            let measurement = self.evaluate_resource(label)?;
            let offset = usize::try_from(place.offset).expect("offset conversion failed");
            state[offset] = measurement;
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

#[derive(Debug, Clone)]
struct Context {
    pub msg: String,
}

impl Process<Context> for CoffeeMachine {
    fn run(&self, context: Context) -> Vec<Event<Context>> {
        let precheck_ok = self.evaluate_preconditions().unwrap_or(false);
        let action = self.next_action();

        if action.is_empty() || !precheck_ok {
            vec![Event {
                action: "__error__".to_string(),
                seq: 0,
                state: self.state.lock().expect("lock failed").clone(),
                data: Context { msg: format!("precheck_ok: {precheck_ok} {:?}", action) },
            }]
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
            let context = event_log.last().expect("last event").data.clone();
            if let Some(transaction) = self.process_action(action, current_seq, context) {
                event_log.push(transaction);
            } else {
                break;
            }
            current_action = self.next_action().first().cloned();
            current_seq += 1;
        }

        let evt = Event {
            action: "__end__".to_string(),
            seq: current_seq,
            state: self.state.lock().expect("lock failed").clone(),
            data: Context { msg: "Coffee machine stopped".to_string() },
        };
        event_log.push(evt);
        event_log
    }

    /// calculate the next action to be executed based on the current state
    /// transform state before calling execute_action
    fn process_action(&self, action: &str, seq: u64, ctx: Context) -> Option<Event<Context>> {
        let mut state = self.state.lock().expect("lock failed");
        let res = self.model.vm.transform(&state, action, 1);
        let mut data = ctx.clone();
        data.msg = format!("{action}::{res:?}");

        if res.is_ok() {
            *state = res.output;
            let evt = Event {
                action: action.to_string(),
                seq,
                state: state.clone(),
                data,
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