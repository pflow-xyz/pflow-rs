use pflow_metamodel::*;

petri_net!( TicTacToe {
    {
        "modelType": "petriNet",
        "version": "v0",
        "places": {
            "10": { "offset": 0, "initial": 1, "capacity": 1, "x": 80,  "y": 182 },
            "11": { "offset": 1, "initial": 1, "capacity": 1, "x": 160, "y": 182 },
            "12": { "offset": 2, "initial": 1, "capacity": 1, "x": 240, "y": 182 },
            "20": { "offset": 3, "initial": 1, "capacity": 1, "x": 80,  "y": 262 },
            "21": { "offset": 4, "initial": 1, "capacity": 1, "x": 160, "y": 262 },
            "22": { "offset": 5, "initial": 1, "capacity": 1, "x": 240, "y": 262 },
            "00": { "offset": 6, "initial": 1, "capacity": 1, "x": 80,  "y": 102 },
            "01": { "offset": 7, "initial": 1, "capacity": 1, "x": 160, "y": 102 },
            "02": { "offset": 8, "initial": 1, "capacity": 1, "x": 240, "y": 102 },
            "next": { "offset": 9, "capacity": 1, "x": 480, "y": 502 }
        },
        "transitions": {
            "X00": { "offset":  0, "role": "x", "x": 400, "y": 102 },
            "X01": { "offset":  1, "role": "x", "x": 480, "y": 102 },
            "X02": { "offset":  2, "role": "x", "x": 560, "y": 102 },
            "X10": { "offset":  3, "role": "x", "x": 400, "y": 182 },
            "X11": { "offset":  4, "role": "x", "x": 480, "y": 182 },
            "X12": { "offset":  5, "role": "x", "x": 560, "y": 182 },
            "X20": { "offset":  6, "role": "x", "x": 400, "y": 262 },
            "X21": { "offset":  7, "role": "x", "x": 480, "y": 262 },
            "X22": { "offset":  8, "role": "x", "x": 560, "y": 262 },
            "O00": { "offset":  9, "role": "o", "x": 80,  "y": 422 },
            "O01": { "offset": 10, "role": "o", "x": 160, "y": 422 },
            "O02": { "offset": 11, "role": "o", "x": 240, "y": 422 },
            "O10": { "offset": 12, "role": "o", "x": 80,  "y": 502 },
            "O11": { "offset": 13, "role": "o", "x": 160, "y": 502 },
            "O12": { "offset": 14, "role": "o", "x": 240, "y": 502 },
            "O20": { "offset": 15, "role": "o", "x": 80,  "y": 582 },
            "O21": { "offset": 16, "role": "o", "x": 160, "y": 582 },
            "O22": { "offset": 17, "role": "o", "x": 240, "y": 582 }
        },
        "arcs": [
            { "source": "00",   "target": "X00"  },
            { "source": "X00",  "target": "next" },
            { "source": "01",   "target": "X01"  },
            { "source": "X01",  "target": "next" },
            { "source": "02",   "target": "X02"  },
            { "source": "X02",  "target": "next" },
            { "source": "10",   "target": "X10"  },
            { "source": "X10",  "target": "next" },
            { "source": "11",   "target": "X11"  },
            { "source": "X11",  "target": "next" },
            { "source": "12",   "target": "X12"  },
            { "source": "X12",  "target": "next" },
            { "source": "20",   "target": "X20"  },
            { "source": "X20",  "target": "next" },
            { "source": "21",   "target": "X21"  },
            { "source": "X21",  "target": "next" },
            { "source": "22",   "target": "X22"  },
            { "source": "X22",  "target": "next" },
            { "source": "00",   "target": "O00"  },
            { "source": "next", "target": "O00"  },
            { "source": "01",   "target": "O01"  },
            { "source": "next", "target": "O01"  },
            { "source": "02",   "target": "O02"  },
            { "source": "next", "target": "O02"  },
            { "source": "10",   "target": "O10"  },
            { "source": "next", "target": "O10"  },
            { "source": "11",   "target": "O11"  },
            { "source": "next", "target": "O11"  },
            { "source": "12",   "target": "O12"  },
            { "source": "next", "target": "O12"  },
            { "source": "20",   "target": "O20"  },
            { "source": "next", "target": "O20"  },
            { "source": "21",   "target": "O21"  },
            { "source": "next", "target": "O21"  },
            { "source": "22",   "target": "O22"  },
            { "source": "next", "target": "O22"  }
        ]
    }
});

const WIN_SETS: [[&str; 3]; 8] = [
    ["00", "01", "02"],
    ["10", "11", "12"],
    ["20", "21", "22"],
    ["00", "10", "20"],
    ["01", "11", "21"],
    ["02", "12", "22"],
    ["00", "11", "22"],
    ["20", "11", "02"],
];

#[derive(Debug, Clone)]
struct Context {
    pub msg: String,
    pub board: std::collections::HashMap<String, Option<String>>,
    pub game_over: bool,
    pub winner: Option<String>,
}

impl Default for Context {
    fn default() -> Self {
        Context::new("TicTacToe game started".to_string())
    }
}

impl Context {
    fn new(msg: String) -> Self {
        Self {
            msg,
            board: [
                ("00".to_string(), None), ("01".to_string(), None), ("02".to_string(), None),
                ("10".to_string(), None), ("11".to_string(), None), ("12".to_string(), None),
                ("20".to_string(), None), ("21".to_string(), None), ("22".to_string(), None),
            ].iter().cloned().collect(),
            game_over: false,
            winner: None,
        }
    }

    fn move_player(&mut self, player: &str, cell: &str) -> Result<(), String> {
        if self.board.contains_key(cell) {
            if self.board[cell].is_none() {
                self.board.insert(cell.to_string(), Some(player.to_string()));
                if self.is_winner(player) {
                    self.game_over = true;
                    self.winner = Some(player.to_string());
                }
                Ok(())
            } else {
                Err(format!("Cell {} is already taken", cell))
            }
        } else {
            Err(format!("Invalid cell: {}", cell))
        }
    }

    fn is_winner(&self, player: &str) -> bool {
        for win_set in WIN_SETS {
            if win_set.iter().all(|cell| self.board[&cell.to_string()].as_deref() == Some(player)) {
               return true;
            }
        }
        false
    }
}

impl State for TicTacToe {
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

    /// This is a dummy implementation that always returns 1
    fn evaluate_resource(&self, label: &str) -> Result<i32, StateMachineError> {
        println!("Measuring resource: {label}");
        match label {
            "10" | "11" | "12" | "20" | "21" | "22" | "00" | "01" | "02" => Ok(1),
            _ => Ok(0),
        }
    }
}

impl Process<Context> for TicTacToe {
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
            let ctx = event_log.last().expect("last event").data.clone();
            if let Some(transaction) = self.process_action(action, current_seq, ctx) {
                event_log.push(transaction);
                if event_log.last().expect("last event").data.game_over {
                    break;
                }
            } else {
                break;
            }
            current_action = self.next_action().first().cloned();
            current_seq += 1;
        }
        let data = event_log.last().expect("last event").data.clone();

        let evt = Event {
            action: "__end__".to_string(),
            seq: current_seq + 1,
            state: self.state.lock().expect("lock failed").clone(),
            data,
        };
        event_log.push(evt);
        event_log
    }

    fn process_action(&self, action: &str, seq: u64, ctx: Context) -> Option<Event<Context>> {
        let mut state = self.state.lock().expect("lock failed");
        let res = self.model.vm.transform(&state, action, 1);

        if res.is_ok() {
            let mut data = ctx.clone();
            data.msg = format!("Player {} moved to {}", action.chars().nth(0).unwrap(), &action[1..]);
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
                        data: ctx,
                    };
                    Some(evt)
                }
                Ok(transaction) => Some(transaction),
            }
        } else {
            None
        }
    }

    /// NOTE: this simulation plays both sides of the game randomly
    /// because the transition key order is not deterministic
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
            "X00" | "X01" | "X02" | "X10" | "X11" | "X12" | "X20" | "X21" | "X22" |
            "O00" | "O01" | "O02" | "O10" | "O11" | "O12" | "O20" | "O21" | "O22" => {
                let (player, coord) = if event.action.starts_with("X") {
                    ("X", &event.action[1..])
                } else {
                    ("O", &event.action[1..])
                };
                let mut ctx = event.data.clone();
                ctx.move_player(player, coord).expect("move failed");
                Ok(Event {
                    action: event.action.clone(),
                    seq: event.seq,
                    state: event.state.clone(),
                    data: ctx,
                })
            },
            _ => Err(StateMachineError::InvalidAction),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tic_tac_toe() {
        let ttt = TicTacToe::new();
        println!("https://pflow.dev/?z={}", ttt.model.net.to_zblob().base64_zipped);
        for event in ttt.run(Context::new("Start".to_string())) {
            println!("{event:?}");
        }
    }
}