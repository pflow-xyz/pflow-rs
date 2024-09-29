use pflow_metamodel::*;

state_machine!( BasicStateMachine {
    Crash --> [*];
    Moving --> Crash;
    Moving --> Still;
    Still --> Moving;
    Still --> [*];
    [*] --> Still;
});

#[derive(Debug, Clone)]
struct Context {
    pub msg: String,
}

// REVIEW: State implementation for SimpleStateMachine could be left out entirely
// workflow start with empty state and don't need to evaluate resources
impl State for BasicStateMachine {
    fn evaluate_preconditions(&self) -> Result<bool, StateMachineError> {
        Ok(true) // no preconditions: workflow start with empty state
    }

    fn evaluate_resource(&self, _label: &str) -> Result<i32, StateMachineError> {
        Ok(-1) // no resources to evaluate: workflows don't need resources
    }
}

impl Process<Context> for BasicStateMachine {
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
            let context = event_log.last().expect("last event").data.clone();
            if let Some(transaction) = self.process_action(action, current_seq, context) {
                event_log.push(transaction);
            } else {
                break;
            }
            if current_seq > 9 && event_log.last().expect("last event").action == "Crash-->[*]" {
                break; // prevent infinite loop
            }
            let available_actions = self.next_action();
            current_action = if current_seq % 2 == 0 {
                available_actions.first().cloned()
            } else {
                available_actions.last().cloned()
            };
            current_seq += 1;
        }

        let evt = Event {
            action: "__end__".to_string(),
            seq: current_seq + 1,
            state: self.state.lock().expect("lock failed").clone(),
            data: Context { msg: "Simple state machine stopped".to_string() },
        };
        event_log.push(evt);
        event_log
    }

    fn process_action(&self, action: &str, seq: u64, ctx: Context) -> Option<Event<Context>> {
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
        use rand::seq::SliceRandom;
        let state = self.state.lock().expect("lock failed");
        let mut actions: Vec<String> = self.model.vm.transitions.keys()
            .filter(|action| self.model.vm.transform(&state, action, 1).is_ok())
            .cloned()
            .collect();
        actions.shuffle(&mut rand::thread_rng());
        println!("Available actions: {:?}", actions);
        actions
    }

    fn execute_action(&self, event: Event<Context>) -> Result<Event<Context>, StateMachineError> {
        println!("{} - Executing action: {}", event.seq, event.action);
        match event.action.as_str() {
            "Crash-->[*]" |
            "Moving-->Crash" |
            "Moving-->Still" |
            "Still-->Moving" |
            "Still-->[*]" |
            "[*]-->Still" => Ok(event),
            _ => Err(StateMachineError::InvalidAction),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_state_machine() {
        let sm = BasicStateMachine::new();
        println!("https://pflow.dev/?z={}", sm.model.net.to_zblob().base64_zipped);
        for event in sm.run(Context { msg: "Start".to_string() }) {
            println!("{event:?}");
        }
    }
}
