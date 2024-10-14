/*!
In the provided code, the concept of models is implemented using state machines and Petri nets. Here’s a brief comparison of how models are used in this context:

**State Machines and Petri Nets:**
The code defines state machines using macros like `state_machine!` and `petri_net!`.
These macros help in defining the states, transitions, and actions of the models.
Examples include `CoffeeMachine`, `TicTacToe`, and `BasicStateMachine`.

**Model Structs:**
Each state machine or Petri net is associated with a model struct that implements the `State` and `Process` traits.
These structs manage the state and transitions of the models.
For instance, `CoffeeMachine`, `TicTacToe`, and `BasicStateMachine` structs implement the `State` and `Process` traits to handle state evaluation and action processing.

**Context Structs:**
Context structs like `Context` and `GameContext` are used to hold additional data relevant to the state machine's operation.
These structs are passed around during the execution of the state machine to maintain and update the state.

**Serialization and Deserialization:**
The `serde` library is used for serialization and deserialization of context data, making it easier to manage and transfer state information.
*/

/// Module for server-related functionality.
pub mod server;

/// Module for storage-related functionality.
pub mod storage;

/// Module for command-related functionality.
pub mod command;

/// Module for the CoffeeMachine state machine.
mod coffee_machine;

/// Module for the TicTacToe state machine.
mod tic_tac_toe;

/// Module for the BasicStateMachine state machine.
mod basic_state_machine;

/// Module for the algebraic coffee machine state machine.
mod algebraic_coffee_machine;
