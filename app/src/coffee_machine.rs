use std::sync::Mutex;
use std::sync::Arc;
use pflow_metamodel::*;
use pflow_metamodel::dsl::{Builder, Dsl};

#[derive(Clone, Debug)]
struct Payment {
    amount: i32,
    to: String,
    from: String,
    fee: Option<i32>
}

impl CoffeeMachine {
    fn boil_water(&self, event: &Event<Context>) -> Result<(), StateMachineError> {
        println!("{}, {}", &event.action, &event.data.payment.params.amount);
        Ok(())
    }

    fn grind_beans(&self, event: &Event<Context>) -> Result<(), StateMachineError> {
        println!("{}, {}", &event.action, &event.data.payment.params.amount);
        Ok(())
    }

    fn brew_coffee(&self, event: &Event<Context>) -> Result<(), StateMachineError> {
        println!("{}, {}", &event.action, &event.data.payment.params.amount);
        Ok(())
    }

    fn pour_coffee(&self, event: &Event<Context>) -> Result<(), StateMachineError> {
        println!("{}, {}", &event.data.msg, &event.data.payment.params.amount);
        Ok(())
    }

    fn send(&self, event: &Event<Context>) -> Result<(), StateMachineError> {
        println!("{}, {}", &event.data.msg, &event.data.payment.params.amount);
        Ok(())
    }

    fn credit(&self, event: &Event<Context>) -> Result<(), StateMachineError> {
        println!("{}, {}", &event.data.msg, &event.data.payment.params.amount);
        Ok(())
    }
}

impl SubProcess<'_, Payment> for Transaction<Payment> {
    fn new(payment: Payment) -> Self {
        let model = Self::model(payment.clone());
        let state = Arc::new(Mutex::new(model.vm.initial_vector()));

        Self {
            model,
            state,
            params: payment,
        }
    }

    fn model(payload: Payment) -> Model {
        let mut net = petri_net::PetriNet::new();
        let mut b = Builder { net: &mut net };
        let amount_minus_fee = payload.amount - payload.fee.unwrap_or(0);
        b.model_type("petriNet");
        b.cell("Payment", Some(amount_minus_fee), Some(amount_minus_fee), 235, 145);
        b.cell("Confirmed", Some(0), Some(1), 75, 300);
        b.cell("Pending", Some(1), Some(1), 395, 300);
        b.func("credit", payload.to.as_str(), 395, 145);
        b.func("debit", payload.from.as_str(), 75, 145);
        b.func("send", payload.from.as_str(), 235, 300);
        b.arrow("debit", "Payment", payload.amount);
        b.guard("Confirmed", "debit", 1);
        b.arrow("send", "Confirmed", 1);
        b.arrow("Payment", "credit", amount_minus_fee);
        b.guard("send", "Payment", 1);
        b.arrow("Pending", "send", 1);
        b.guard("Pending", "credit", 1);
        let vm = Box::new(b.as_vasm());
        Model { net, vm }
    }

    fn execute(&self) -> Result<Vec<Event<Tx>>, String> {
        Ok(vec![self.event("send")?, self.event("credit")?])
    }

    fn event(&self, action: &str) -> Result<Event<Tx>, String> {
        let mut state = self.state.lock().expect("lock failed");
        let res = self.model.vm.transform(&state, action, 1);
        if res.is_ok() {
            state.clone_from(&res.output);
            Ok(Event {
                action: action.to_string(),
                seq: 0,
                state: state.clone(),
                data: res,
            })
        } else {
            Err(format!("failed to execute: {action}"))
        }
    }
}

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
    Pending --> send;
    send --> Sent;
    Sent --> credit;
    credit --> Payment;
    Payment --> pour_coffee;
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
            "Pending" => Ok(1),
            "Water" | "CoffeeBeans" | "Filter" | "Cup" => Ok(1),
            _ => Ok(0),
        }
    }
}

#[derive(Debug, Clone)]
struct Context {
    pub msg: String,
    pub payment: Transaction<Payment>,
}

impl Process<Context> for CoffeeMachine {
    fn run(&self, context: Context) -> Vec<Event<Context>> {
        let precheck_ok = self.evaluate_preconditions().unwrap_or(false);
        let action = self.next_action();
        let mut seq = 0;
        let mut event_log = vec![Event {
            action: "__begin__".to_string(),
            seq,
            state: self.model.vm.initial_vector(),
            data: context.clone(),
        }];
        seq +=1;

        match context.payment.execute() {
            Ok(res) => {
                for event in res {
                    let transaction = self.execute_action(Event{
                        action: event.action,
                        seq,
                        state: self.state.lock().expect("lock failed").clone(),
                        data: context.clone(),
                    });
                    match transaction {
                        Ok(transaction) => {
                            event_log.push(transaction);
                            seq += 1;
                        },
                        Err(e) => {
                            println!("{e:?}");
                            return vec![Event {
                                action: "__error__".to_string(),
                                seq: 0,
                                state: vec![],
                                data: Context {
                                    msg: format!("payment subprocess failed:{e:?}"),
                                    ..context
                                },
                            }];
                        },
                    }
                    seq += 1;
                }
            },
            Err(e) => {
                println!("{e:?}");
                return vec![Event {
                    action: "__error__".to_string(),
                    seq: 0,
                    state: vec![],
                    data: Context {
                        msg: format!("payment failed:{e:?}"),
                        ..context
                    },
                }];
            },
        }

        if action.is_empty() || !precheck_ok {
            vec![Event {
                action: "__error__".to_string(),
                seq: 0,
                state: self.state.lock().expect("lock failed").clone(),
                data: Context {
                    msg: format!("precheck_ok: {precheck_ok} {:?}", action),
                    ..context
                },
            }]
        } else {
            self.run_impl(Some(&action[0]), None, event_log)
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

        let context = event_log.last().expect("last event").data.clone();
        let evt = Event {
            action: "__end__".to_string(),
            seq: current_seq,
            state: self.state.lock().expect("lock failed").clone(),
            data: Context {
                msg: "Coffee machine stopped".to_string(), ..context
            },
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
                data: data.clone(),
            };
            let transaction = self.execute_action(evt);

            match transaction {
                Err(e) => {
                    let evt = Event {
                        action: format!("__error__::{action}::{e:?}"),
                        seq,
                        state: state.clone(),
                        data: Context {
                            msg: "Action failed".to_string(), ..data
                        },
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
            "boil_water" => self.boil_water(&event).map(|_| event),
            "grind_beans" => self.grind_beans(&event).map(|_| event),
            "brew_coffee" => self.brew_coffee(&event).map(|_| event),
            "pour_coffee" => self.pour_coffee(&event).map(|_| event),
            "send" => self.send(&event).map(|_| event),
            "credit" => self.credit(&event).map(|_| event),
            _ => Err(StateMachineError::InvalidAction),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_model() {
        let payment = Transaction::new(Payment {
            amount: 99,
            to: "receiver".to_string(),
            from: "sender".to_string(),
            fee: None,
        });

        println!(
            "https://pflow.dev/?z={}",
            payment.model.net.to_zblob().base64_zipped
        );
    }

    #[test]
    fn test_coffee_machine() {
        let cm = CoffeeMachine::new();
        println!(
            "coffee_machine https://pflow.dev/?z={}",
            cm.model.net.to_zblob().base64_zipped
        );
        let events = cm.run(Context {
            msg: "Start".to_string(),
            payment: Transaction::new(Payment {
                amount: 100,
                to: "VendingMachine".to_string(),
                from: "Bob".to_string(),
                fee: None,
            }),
        });

        if events.is_empty() {
            panic!("unexpected error");
        } else {
            if let Some(evt) = events.last() {
                println!("{:?}", evt);
            }
        }
    }
}
