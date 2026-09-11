//! Core Petri net types, fluent builder API, and state utilities.

pub mod builder;
pub mod colors;
pub mod json;
pub mod net;
pub mod stateutil;

pub use builder::Builder;
pub use colors::{unfold, ArcKind, ColorMap, ColorRef, UnfoldedArc, UnfoldedNet};
pub use json::{from_json, FloatVec, RawArc, RawPlace, RawTransition, ShapeADocument};
pub use net::{Arc, PetriNet, Place, State, Transition};
