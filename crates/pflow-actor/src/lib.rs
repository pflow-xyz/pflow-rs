//! Actor model (message bus, per-actor subnets, metamodel bundle
//! composition), ported from go-pflow's `actor` package — scoped to "bus,
//! subnets, metabundle" per this crate's ROADMAP.md Phase 5 entry.
//!
//! go-pflow's `actor.go`/`builder.go` (the `Actor`/`Bus` runtime that
//! dispatches signals through an internal Petri net or `workflow.Workflow`
//! and the fluent `ActorSystem` builder around it) are **not ported**:
//! that is a separate execution engine, and the scope named here is the
//! structural half — the bus (`bus.rs`) and the metamodel-layer bundle
//! emission (`metabundle.rs`) that composes an [`ActorSystem`]'s topology
//! with the rest of the ecosystem's `pflow_compose::Bundle`s. [`Bus`]
//! itself is still fully functional as a synchronous pub/sub mechanism —
//! see its module doc for the concurrency adaptations that entails.
//!
//! ```
//! use std::collections::HashMap;
//! use pflow_actor::{ActorSystem, Actor, Behavior, Trigger, Signal};
//!
//! let mut system = ActorSystem::new("sys");
//! let mut worker = Actor::new("worker");
//! worker.behaviors.insert(
//!     "b".into(),
//!     Behavior {
//!         id: "b".into(),
//!         triggers: HashMap::from([("task".to_string(), Trigger { signal_type: "task".into(), condition: None })]),
//!         ..Default::default()
//!     },
//! );
//! system.add_actor(worker);
//! system.bus.subscribe("worker", "task", Box::new(|_ctx, _sig| Ok(())));
//!
//! system.bus.publish_sync(&mut system.actors, Signal { typ: "task".into(), ..Default::default() });
//!
//! let bundle = system.to_meta_bundle();
//! assert_eq!(bundle.subnets.len(), 1);
//! ```

pub mod bus;
pub mod metabundle;
pub mod types;

pub use bus::{Bus, BusStats, Middleware};
pub use metabundle::ActorSystem;
pub use types::{Actor, ActorContext, Behavior, Emitter, Signal, SignalHandler, Subscription, Trigger};
