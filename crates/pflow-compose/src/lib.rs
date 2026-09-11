//! Composition, templates, derivation and model-extension operations,
//! ported from go-pflow's `metamodel` (`compose*.go`, `queue.go`,
//! `patterns_compose.go`), `templates` and `derive` packages, plus
//! petri-pilot's `petri_extend`/`sim_extend` op set.
//!
//! # Bundle -> Flatten
//!
//! [`Bundle`] holds independently-authored [`Subnet`]s and the typed
//! [`Link`]s between them; [`Bundle::flatten`] lowers the whole thing to a
//! single `pflow_metamodel::Model`. Place fusion (`TokenLink`/`DataLink`)
//! and transition fusion (`EventLink`) are computed over union-find
//! equivalence classes, which is what makes flattening associative and
//! order-independent — see [`unionfind`]. `GuardLink` lowers to a
//! read/inhibitor arc wherever the condition has a structural form, and to
//! a guard-expression conjunct otherwise (`W_GUARD_OPAQUE`) — see
//! [`guard`].
//!
//! ```
//! use pflow_compose::bundle::{Bundle, Endpoint, Link, LinkKind, NetType, Subnet};
//! use pflow_metamodel::{Arc, Model, Place, StateKind, Transition};
//!
//! fn net(id: &str) -> Model {
//!     Model {
//!         name: id.to_string(),
//!         places: vec![Place { id: "p".into(), kind: Some(StateKind::Token), initial: 1, ..Default::default() }],
//!         transitions: vec![Transition { id: "t".into(), ..Default::default() }],
//!         arcs: vec![Arc { from: "p".into(), to: "t".into(), weight: 1, ..Default::default() }],
//!         ..Default::default()
//!     }
//! }
//!
//! let mut b = Bundle::new("demo");
//! b.add_subnet(Subnet { id: "a".into(), model: net("a"), ..Default::default() });
//! let flat = b.flatten().unwrap();
//! assert_eq!(flat.places.len(), 1);
//! ```
//!
//! # Templates and derive
//!
//! [`templates`] generates parameterized `pflow_core::PetriNet` patterns
//! (SIR, SEIR, queue, producer-consumer, workflow) — Shape A, matching
//! go-pflow's `templates` package, which builds on `petri.PetriNet` rather
//! than the Shape B model the rest of this crate uses.
//!
//! [`derive`] builds evaluation variants of a declared `PetriNet`:
//! catalyzed copies, hazard replacement, dead-place pruning and readback
//! removal — the net->net transforms go-pflow's `derive` package defines.
//!
//! # Extension operations
//!
//! [`extend`] applies the `add_place`/`add_transition`/`add_arc`/... op set
//! petri-pilot's `petri_extend`/`sim_extend` MCP tools expose, and diffs two
//! models the way `sim_compare` does.

pub mod bundle;
mod derive;
pub mod exprrewrite;
mod extend;
mod flatten;
mod fuse;
mod guard;
mod matrix;
mod queue;
mod resource_pool;
pub mod templates;
mod unionfind;
mod validate;

pub use bundle::{
    ArcMergePolicy, Bundle, Endpoint, FlattenMap, Link, LinkKind, NetType, Port, PortKind,
    PortTarget, Subnet, ValidationError, ValidationResult,
};
pub use derive::{
    add_catalyzed_copy, drop_places, drop_readbacks, drop_transitions, replace_with_hazard,
    write_only_places,
};
pub use exprrewrite::{place_refs, rewrite_place_refs};
pub use extend::{apply_operation, apply_operations, compare_models, parse_operations, ModelDiff, Operation};
pub use flatten::normalize_kinds;
pub use guard::{and_guards, guard_conjunct, parse_condition, resolve_lowering, LoweredGuard};
pub use matrix::link_legal;
pub use queue::{is_unbounded_queue, new_queue, QueueSpec, QUEUE_DEQUEUE, QUEUE_ENQUEUE, QUEUE_ITEMS, QUEUE_SLOTS};
pub use resource_pool::{new_resource_pool, POOL_ACQUIRE, POOL_AVAILABLE, POOL_IN_USE, POOL_RELEASE};
