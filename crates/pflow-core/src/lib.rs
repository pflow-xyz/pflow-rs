//! Core Petri net types, fluent builder API, and state utilities.

pub mod builder;
pub mod net;
pub mod stateutil;

pub use builder::Builder;
pub use net::{Arc, PetriNet, Place, State, Transition};
