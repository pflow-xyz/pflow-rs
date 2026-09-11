//! Hierarchical statecharts (statemachine.md's regions, states, guarded
//! event transitions) on top of `pflow_metamodel`'s firing rule, ported
//! from go-pflow's `statemachine` package.
//!
//! [`builder::ChartBuilder`] constructs a [`types::Chart`];
//! [`types::Chart::to_meta_model`] / [`types::Chart::to_meta_subnet`] compile
//! it to a `pflow_metamodel::Model` / composable `pflow_compose` `Subnet`
//! (`model.rs`); [`machine::Machine`] executes it by routing every event to
//! [`pflow_metamodel::Model::enabled`] / [`pflow_metamodel::Model::fire`]
//! rather than a second firing-rule implementation (ROADMAP.md ground rule
//! 4) — see `machine.rs`'s module doc for the one documented behavioral gap
//! this inherits from `ToMetaModel` itself.
//!
//! ```
//! use pflow_statemachine::builder::ChartBuilder;
//! use pflow_statemachine::machine::Machine;
//!
//! let mut b = ChartBuilder::new("light");
//! b.region("state")
//!     .state("red").initial()
//!     .state("green")
//!     .state("yellow")
//!     .end_region()
//!     .when("timer").in_("state:red").go_to("state:green")
//!     .when("timer").in_("state:green").go_to("state:yellow")
//!     .when("timer").in_("state:yellow").go_to("state:red");
//!
//! let mut m = Machine::new(b.build());
//! assert_eq!(m.state("state"), "red");
//! m.send_event("timer");
//! assert_eq!(m.state("state"), "green");
//! ```

pub mod builder;
pub mod machine;
pub mod model;
pub mod types;

pub use builder::ChartBuilder;
pub use machine::Machine;
pub use types::{
    callback, increment, increment_by, set, Action, CallbackAction, Chart, Guard, IncrementAction,
    Region, SetAction, State, StatePath, Transition,
};
