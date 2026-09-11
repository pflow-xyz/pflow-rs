//! Shape B `Model` (places, transitions, arcs, stages, schedules,
//! parameters) and the shared firing rule, ported from go-pflow's
//! `metamodel` package.
//!
//! This is the one home for the firing rule (ROADMAP.md ground rule 4):
//! every engine — SSA, state machine, workflow, token runtime — is meant to
//! route enablement and firing through [`Model::enabled`] and
//! [`Model::fire`] rather than reimplementing any part of it.

pub mod access;
pub mod error;
pub mod firing;
pub mod parameters;
pub mod schedule;
pub mod schema;
pub mod stages;

pub use access::AccessControl;
pub use error::{Error, Result};
pub use firing::{ArcRef, Marking};
pub use schedule::RateSegment;
pub use schema::{
    AccessRule, Arc, ArcType, AssertedClass, Binding, Constraint, ControlGroup, Disruption, Event,
    EventField, FieldOption, Model, Parameter, ParameterArc, Place, Player, Presentation, Role,
    Simulation, SolverConfig, StateKind, Transition, TransitionField, ViewDecl,
};
pub use stages::StageExpansion;
