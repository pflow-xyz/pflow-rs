//! Runtime execution of token model schemas.
//!
//! # A documented, deliberate divergence from go-pflow
//!
//! go-pflow's `tokenmodel.Runtime` (`tokenmodel/runtime.go`) executes token
//! arcs with its own private, hardcoded rule: `Enabled` checks only
//! `tokens < 1`, so declared arc weights are never read; there is no way to
//! declare an inhibitor or read arc at all; `Execute` always moves exactly
//! one token per arc; and `Enabled`/`Execute` (the plain, bindings-free
//! entry points) never evaluate an action's `Guard`. ROADMAP.md Phase 5
//! names these as go-pflow's own known defects and gives two options: fix
//! both sides on the shared firing rule from Phase 0, together, or document
//! this runtime as an identity/CID surface only and route real execution
//! through `pflow-metamodel`. Coordinating a go-pflow-side fix is not
//! something this repository can do unilaterally (ground rule: "do not fix
//! one side alone"), so this `Runtime` takes the second option: it now
//! builds a `pflow_metamodel::Model` from the schema's token places and
//! arcs on every enablement/execution call and routes through
//! [`pflow_metamodel::Model::enabled`] / [`pflow_metamodel::Model::fire`] —
//! the one shared firing rule (ROADMAP.md ground rule 4) — instead of
//! reimplementing a fifth copy of it.
//!
//! Concretely, this means:
//!
//! - [`Runtime::enabled`] and [`Runtime::execute`] honour declared arc
//!   [`crate::schema::Arc::weight`] and [`crate::schema::ArcType`]
//!   (inhibitor/read), where go-pflow's `Enabled`/`Execute` cannot — there
//!   is nothing in go-pflow's wire format for them to read (see
//!   `schema.rs`'s doc comment on `ArcType`).
//! - [`Runtime::execute`] moves the arc's effective weight, not a hardcoded
//!   one token.
//! - Data-state arcs are unaffected: they never had counts or a firing rule
//!   to begin with, and [`Runtime::apply_arcs`] still handles them exactly
//!   as before.
//!
//! **What is unchanged, on purpose:** `enabled`/`execute` still do not
//! evaluate an action's `Guard` — that was true of go-pflow's plain
//! entry points too (only `ExecuteWithBindings`/`ExecuteWithGuardFuncs` did),
//! and a guard expression needs bindings this crate has no way to invent.
//! Guard evaluation on `execute_with_bindings`/`execute_with_guard_funcs` is
//! unchanged from before this fix. Token-state effects on that path now also
//! route through [`pflow_metamodel::Model::fire`] (inside
//! [`Runtime::apply_arcs`]) instead of a second, private weight-math
//! implementation that used to live beside it — one home for firing, per
//! the module doc above. Data arcs are untouched: they never had a firing
//! rule to begin with.
//!
//! **Follow-up needed on the go-pflow side:** until `go-pflow/tokenmodel`
//! itself is fixed (or `Arc` grows the same weight/type fields — see
//! `schema.rs`), this Rust runtime is *correct* where go-pflow is buggy,
//! which is an intentional divergence in behaviour, not a golden that
//! go-pflow produced and Rust replays. A schema whose arcs are all
//! weight-1/normal-typed (the only shape go-pflow can express today)
//! behaves identically on both sides; anything using the new fields is
//! Rust-only until go-pflow adds them.

use std::collections::HashMap;
use std::fmt;

use pflow_metamodel::AccessControl;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::schema::{ArcType as TokenArcType, Schema};
use crate::snapshot::{Bindings, BindingsExt, Snapshot};

/// Evaluates guard expressions.
pub trait GuardEvaluator {
    fn evaluate(&self, expr: &str, bindings: &Bindings) -> std::result::Result<bool, String>;

    fn evaluate_constraint(
        &self,
        expr: &str,
        tokens: &HashMap<String, i64>,
    ) -> std::result::Result<bool, String>;
}

/// Describes a failed constraint check.
#[derive(Debug, Clone)]
pub struct ConstraintViolation {
    pub constraint_id: String,
    pub constraint_expr: String,
    pub snapshot: Snapshot,
    pub err: Option<String>,
}

impl fmt::Display for ConstraintViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(err) = &self.err {
            write!(f, "constraint {} error: {}", self.constraint_id, err)
        } else {
            write!(f, "constraint {} violated", self.constraint_id)
        }
    }
}

/// Execution runtime for a schema.
pub struct Runtime {
    pub schema: Schema,
    pub snapshot: Snapshot,
    pub sequence: u64,
    pub check_constraints: bool,
    pub guard_evaluator: Option<Box<dyn GuardEvaluator>>,
    /// Optional role-based access control, checked by [`Runtime::execute_as`]
    /// / [`Runtime::access_allows`]. `None` means every action is open to
    /// every caller — access control is opt-in, matching
    /// [`pflow_metamodel::AccessControl::allows`]'s "no rule, no
    /// restriction" default.
    pub access_control: Option<AccessControl>,
}

impl Runtime {
    pub fn new(schema: Schema) -> Self {
        let snapshot = Snapshot::from_schema(&schema);
        Self {
            schema,
            snapshot,
            sequence: 0,
            check_constraints: true,
            guard_evaluator: None,
            access_control: None,
        }
    }

    pub fn clone_runtime(&self) -> Self {
        Self {
            schema: self.schema.clone(),
            snapshot: self.snapshot.clone(),
            sequence: self.sequence,
            check_constraints: self.check_constraints,
            guard_evaluator: None, // Guard evaluator is not cloned
            access_control: self.access_control.clone(),
        }
    }

    /// Projects the schema's token states and the arcs touching them into a
    /// [`pflow_metamodel::Model`] for enablement/firing purposes — see the
    /// module doc for why. Data states and data arcs take no part; they are
    /// handled separately by [`Runtime::apply_data_arc`], exactly as before.
    /// Rebuilt on every call rather than cached: token model schemas are
    /// small, and a `Schema` has no mutation hook that would let a cache go
    /// stale safely.
    fn firing_model(&self) -> pflow_metamodel::Model {
        use pflow_metamodel::{Arc as MArc, ArcType as MArcType, Place, Transition};

        let mut places = Vec::new();
        for st in self.schema.token_states() {
            places.push(Place {
                id: st.id.clone(),
                initial: st.initial_tokens(),
                ..Default::default()
            });
        }

        let mut transitions = Vec::new();
        for a in &self.schema.actions {
            transitions.push(Transition {
                id: a.id.clone(),
                ..Default::default()
            });
        }

        let mut arcs = Vec::new();
        for arc in &self.schema.arcs {
            let source_is_token = self
                .schema
                .state_by_id(&arc.source)
                .map(|s| s.is_token())
                .unwrap_or(false);
            let target_is_token = self
                .schema
                .state_by_id(&arc.target)
                .map(|s| s.is_token())
                .unwrap_or(false);

            if source_is_token {
                // Input arc: token state -> action. Carries weight/type.
                arcs.push(MArc {
                    from: arc.source.clone(),
                    to: arc.target.clone(),
                    weight: arc.weight,
                    typ: match arc.typ {
                        TokenArcType::Normal => MArcType::Normal,
                        TokenArcType::Inhibitor => MArcType::Inhibitor,
                        TokenArcType::Read => MArcType::Read,
                    },
                    ..Default::default()
                });
            } else if target_is_token {
                // Output arc: action -> token state. Read/inhibitor arcs
                // only make sense place -> transition, so an output arc is
                // always normal (matches pflow-metamodel's own contract).
                arcs.push(MArc {
                    from: arc.source.clone(),
                    to: arc.target.clone(),
                    weight: arc.weight,
                    ..Default::default()
                });
            }
        }

        pflow_metamodel::Model {
            name: self.schema.name.clone(),
            places,
            transitions,
            arcs,
            ..Default::default()
        }
    }

    /// The current token marking, keyed by token state id.
    fn marking(&self) -> pflow_metamodel::Marking {
        self.schema
            .token_states()
            .iter()
            .map(|st| (st.id.clone(), self.snapshot.get_tokens(&st.id)))
            .collect()
    }

    pub fn tokens(&self, state_id: &str) -> i64 {
        self.snapshot.get_tokens(state_id)
    }

    pub fn set_tokens(&mut self, state_id: &str, count: i64) {
        self.snapshot.set_tokens(state_id, count);
    }

    pub fn data(&self, state_id: &str) -> Option<&Value> {
        self.snapshot.get_data(state_id)
    }

    pub fn set_data(&mut self, state_id: &str, value: Value) {
        self.snapshot.set_data(state_id, value);
    }

    /// Returns true if an action can execute: routed through
    /// [`pflow_metamodel::Model::enabled`] (see the module doc) rather than
    /// go-pflow's hardcoded `< 1` check, so declared weights, inhibitor and
    /// read arcs are honoured.
    pub fn enabled(&self, action_id: &str) -> bool {
        if self.schema.action_by_id(action_id).is_none() {
            return false;
        }
        self.firing_model().enabled(action_id, &self.marking())
    }

    /// Returns all actions that can execute.
    pub fn enabled_actions(&self) -> Vec<String> {
        let model = self.firing_model();
        let marking = self.marking();
        self.schema
            .actions
            .iter()
            .filter(|a| model.enabled(&a.id, &marking))
            .map(|a| a.id.clone())
            .collect()
    }

    /// Whether `roles` may fire `action_id`, per [`Runtime::access_control`].
    /// `true` when no access control is configured at all.
    pub fn access_allows(&self, action_id: &str, roles: &[String]) -> bool {
        match &self.access_control {
            None => true,
            Some(ac) => ac.allows(action_id, roles),
        }
    }

    /// Executes an action (token semantics only), enforcing
    /// [`Runtime::access_control`] first.
    pub fn execute_as(&mut self, action_id: &str, roles: &[String]) -> Result<()> {
        if let Some(ac) = &self.access_control {
            ac.allows_why_not(action_id, roles)?;
        }
        self.execute(action_id)
    }

    /// Executes an action (token semantics only). Moves each arc's
    /// [`crate::schema::Arc::effective_weight`], not a hardcoded one token
    /// — see the module doc.
    pub fn execute(&mut self, action_id: &str) -> Result<()> {
        if !self.enabled(action_id) {
            return Err(Error::ActionNotEnabled(action_id.to_string()));
        }

        let model = self.firing_model();
        let before = self.marking();
        let after = model.fire(action_id, &before);
        for (place, count) in after {
            self.snapshot.set_tokens(&place, count);
        }

        self.sequence += 1;

        if self.check_constraints {
            let violations = self.check_constraints_impl();
            if let Some(v) = violations.first() {
                if let Some(err) = &v.err {
                    return Err(Error::ConstraintEvaluation(
                        v.constraint_id.clone(),
                        err.clone(),
                    ));
                }
                return Err(Error::ConstraintViolated(v.constraint_id.clone()));
            }
        }

        Ok(())
    }

    /// Executes an action with variable bindings.
    pub fn execute_with_bindings(&mut self, action_id: &str, bindings: &Bindings) -> Result<()> {
        let action = self
            .schema
            .action_by_id(action_id)
            .ok_or_else(|| Error::ActionNotFound(action_id.to_string()))?
            .clone();

        // Evaluate guard
        if !action.guard.is_empty() {
            if let Some(evaluator) = &self.guard_evaluator {
                match evaluator.evaluate(&action.guard, bindings) {
                    Ok(true) => {}
                    Ok(false) => return Err(Error::GuardNotSatisfied),
                    Err(e) => return Err(Error::GuardEvaluation(e)),
                }
            }
        }

        if !self.enabled(action_id) {
            return Err(Error::ActionNotEnabled(action_id.to_string()));
        }

        self.apply_arcs(action_id, bindings);
        self.sequence += 1;

        if self.check_constraints {
            let violations = self.check_constraints_impl();
            if let Some(v) = violations.first() {
                if let Some(err) = &v.err {
                    return Err(Error::ConstraintEvaluation(
                        v.constraint_id.clone(),
                        err.clone(),
                    ));
                }
                return Err(Error::ConstraintViolated(v.constraint_id.clone()));
            }
        }

        Ok(())
    }

    fn apply_arcs(&mut self, action_id: &str, bindings: &Bindings) {
        // Token arcs route through the shared firing rule (see the module
        // doc), the same way `execute` does — this used to have its own
        // hand-rolled weight math here, a second private firing-mutation
        // implementation beside `pflow_metamodel::Model::fire`, which is
        // exactly the divergence ROADMAP.md ground rule 4 exists to
        // prevent. Data arcs take no part in the firing rule and keep their
        // own key/value application below, unchanged.
        let model = self.firing_model();
        let before = self.marking();
        let after = model.fire(action_id, &before);
        for (place, count) in after {
            self.snapshot.set_tokens(&place, count);
        }

        // Clone arcs to avoid borrow issues
        let input_arcs: Vec<_> = self
            .schema
            .input_arcs(action_id)
            .into_iter()
            .cloned()
            .collect();
        let output_arcs: Vec<_> = self
            .schema
            .output_arcs(action_id)
            .into_iter()
            .cloned()
            .collect();

        for arc in &input_arcs {
            let is_token = self
                .schema
                .state_by_id(&arc.source)
                .map(|s| s.is_token())
                .unwrap_or(false);

            if !is_token {
                self.apply_data_arc(&arc.source, arc, bindings, false);
            }
        }

        for arc in &output_arcs {
            let is_token = self
                .schema
                .state_by_id(&arc.target)
                .map(|s| s.is_token())
                .unwrap_or(false);

            if !is_token {
                self.apply_data_arc(&arc.target, arc, bindings, true);
            }
        }
    }

    fn apply_data_arc(
        &mut self,
        state_id: &str,
        arc: &crate::schema::Arc,
        bindings: &Bindings,
        add: bool,
    ) {
        let value_name = if arc.value.is_empty() {
            "amount"
        } else {
            &arc.value
        };
        let amount = bindings.get_i64(value_name);

        if arc.keys.is_empty() {
            return;
        }

        if arc.keys.len() == 1 {
            let key = bindings.get_string(&arc.keys[0]);
            if key.is_empty() {
                return;
            }

            let current = self.get_map_i64(state_id, &key);
            let new_val = if add {
                current + amount
            } else {
                current - amount
            };
            self.snapshot
                .set_data_map_value(state_id, &key, Value::Number(new_val.into()));
        } else if arc.keys.len() == 2 {
            let key1 = bindings.get_string(&arc.keys[0]);
            let key2 = bindings.get_string(&arc.keys[1]);
            if key1.is_empty() || key2.is_empty() {
                return;
            }

            // Nested map access
            let current = self.get_nested_map_i64(state_id, &key1, &key2);
            let new_val = if add {
                current + amount
            } else {
                current - amount
            };

            // Build nested structure
            let entry = self
                .snapshot
                .data
                .entry(state_id.to_string())
                .or_insert_with(|| Value::Object(Default::default()));

            if let Value::Object(outer) = entry {
                let nested = outer
                    .entry(key1)
                    .or_insert_with(|| Value::Object(Default::default()));
                if let Value::Object(inner) = nested {
                    inner.insert(key2, Value::Number(new_val.into()));
                }
            }
        }
    }

    fn get_map_i64(&self, state_id: &str, key: &str) -> i64 {
        self.snapshot
            .get_data_map_value(state_id, key)
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    }

    fn get_nested_map_i64(&self, state_id: &str, key1: &str, key2: &str) -> i64 {
        self.snapshot
            .get_data_map(state_id)
            .and_then(|m| m.get(key1))
            .and_then(|v| v.as_object())
            .and_then(|m| m.get(key2))
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    }

    fn check_constraints_impl(&self) -> Vec<ConstraintViolation> {
        let mut violations = Vec::new();

        let evaluator = match &self.guard_evaluator {
            Some(e) => e,
            None => return violations,
        };

        for c in &self.schema.constraints {
            match evaluator.evaluate_constraint(&c.expr, &self.snapshot.tokens) {
                Ok(true) => {}
                Ok(false) => {
                    violations.push(ConstraintViolation {
                        constraint_id: c.id.clone(),
                        constraint_expr: c.expr.clone(),
                        snapshot: self.snapshot.clone(),
                        err: None,
                    });
                }
                Err(e) => {
                    violations.push(ConstraintViolation {
                        constraint_id: c.id.clone(),
                        constraint_expr: c.expr.clone(),
                        snapshot: self.snapshot.clone(),
                        err: Some(e),
                    });
                }
            }
        }

        violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Action, Arc, Kind, State};

    fn make_simple_schema() -> Schema {
        let mut s = Schema::new("test");
        s.add_state(State {
            id: "ready".into(),
            kind: Kind::Token,
            initial: Some(Value::Number(1.into())),
            typ: "int".into(),
            exported: false,
        });
        s.add_state(State {
            id: "done".into(),
            kind: Kind::Token,
            initial: Some(Value::Number(0.into())),
            typ: "int".into(),
            exported: false,
        });
        s.add_action(Action {
            id: "process".into(),
            guard: String::new(),
            event_id: String::new(),
            event_bindings: None,
        });
        s.add_arc(Arc {
            source: "ready".into(),
            target: "process".into(),
            keys: vec![],
            value: String::new(),
            ..Default::default()
        });
        s.add_arc(Arc {
            source: "process".into(),
            target: "done".into(),
            keys: vec![],
            value: String::new(),
            ..Default::default()
        });
        s
    }

    #[test]
    fn test_runtime_basic() {
        let schema = make_simple_schema();
        let mut rt = Runtime::new(schema);

        assert_eq!(rt.tokens("ready"), 1);
        assert_eq!(rt.tokens("done"), 0);
        assert!(rt.enabled("process"));

        rt.execute("process").unwrap();

        assert_eq!(rt.tokens("ready"), 0);
        assert_eq!(rt.tokens("done"), 1);
        assert!(!rt.enabled("process"));
    }

    #[test]
    fn test_runtime_not_enabled() {
        let schema = make_simple_schema();
        let mut rt = Runtime::new(schema);

        rt.execute("process").unwrap();
        let result = rt.execute("process");
        assert!(result.is_err());
    }

    #[test]
    fn test_enabled_actions() {
        let schema = make_simple_schema();
        let rt = Runtime::new(schema);

        let enabled = rt.enabled_actions();
        assert_eq!(enabled, vec!["process"]);
    }

    #[test]
    fn test_execute_with_bindings() {
        let mut schema = Schema::new("erc20");
        schema.add_data_state(
            "balances",
            "map[address]uint256",
            Some(Value::Object(Default::default())),
            true,
        );
        schema.add_action(Action {
            id: "transfer".into(),
            guard: String::new(),
            event_id: String::new(),
            event_bindings: None,
        });
        schema.add_arc(Arc {
            source: "balances".into(),
            target: "transfer".into(),
            keys: vec!["from".into()],
            value: String::new(),
            ..Default::default()
        });
        schema.add_arc(Arc {
            source: "transfer".into(),
            target: "balances".into(),
            keys: vec!["to".into()],
            value: String::new(),
            ..Default::default()
        });

        let mut rt = Runtime::new(schema);

        // Set initial balance
        rt.snapshot
            .set_data_map_value("balances", "alice", Value::Number(100.into()));

        let mut bindings = Bindings::new();
        bindings.insert("from".into(), Value::String("alice".into()));
        bindings.insert("to".into(), Value::String("bob".into()));
        bindings.insert("amount".into(), Value::Number(30.into()));

        rt.execute_with_bindings("transfer", &bindings).unwrap();

        assert_eq!(rt.get_map_i64("balances", "alice"), 70);
        assert_eq!(rt.get_map_i64("balances", "bob"), 30);
    }

    // --- Phase 5 fix: weights, inhibitors, read arcs, access control ---

    #[test]
    fn execute_moves_the_declared_weight_not_a_hardcoded_one_token() {
        let mut s = Schema::new("weighted");
        s.add_token_state("pool", 5);
        s.add_token_state("out", 0);
        s.add_action(Action {
            id: "drain".into(),
            guard: String::new(),
            event_id: String::new(),
            event_bindings: None,
        });
        s.add_arc(Arc {
            source: "pool".into(),
            target: "drain".into(),
            weight: 3,
            ..Default::default()
        });
        s.add_arc(Arc {
            source: "drain".into(),
            target: "out".into(),
            weight: 3,
            ..Default::default()
        });

        let mut rt = Runtime::new(s);
        assert!(rt.enabled("drain"));
        rt.execute("drain").unwrap();
        assert_eq!(rt.tokens("pool"), 2);
        assert_eq!(rt.tokens("out"), 3);

        // go-pflow's own Runtime.Enabled hardcodes `< 1`, so it would call
        // this enabled with 2 tokens against a weight-3 arc. This one must
        // not.
        assert!(!rt.enabled("drain"));
    }

    #[test]
    fn inhibitor_arc_blocks_what_go_pflow_cannot_express() {
        let mut s = Schema::new("inhibited");
        s.add_token_state("p", 1);
        s.add_token_state("stop", 1);
        s.add_action(Action {
            id: "t".into(),
            guard: String::new(),
            event_id: String::new(),
            event_bindings: None,
        });
        s.add_arc(Arc {
            source: "p".into(),
            target: "t".into(),
            ..Default::default()
        });
        s.add_arc(Arc {
            source: "stop".into(),
            target: "t".into(),
            typ: crate::schema::ArcType::Inhibitor,
            ..Default::default()
        });

        let rt = Runtime::new(s);
        assert!(!rt.enabled("t"), "inhibitor at its weight must block");
    }

    #[test]
    fn read_arc_gates_without_consuming() {
        let mut s = Schema::new("read");
        s.add_token_state("key", 1);
        s.add_token_state("p", 1);
        s.add_token_state("out", 0);
        s.add_action(Action {
            id: "t".into(),
            guard: String::new(),
            event_id: String::new(),
            event_bindings: None,
        });
        s.add_arc(Arc {
            source: "p".into(),
            target: "t".into(),
            ..Default::default()
        });
        s.add_arc(Arc {
            source: "key".into(),
            target: "t".into(),
            typ: crate::schema::ArcType::Read,
            ..Default::default()
        });
        s.add_arc(Arc {
            source: "t".into(),
            target: "out".into(),
            ..Default::default()
        });

        let mut rt = Runtime::new(s);
        assert!(rt.enabled("t"));
        rt.execute("t").unwrap();
        assert_eq!(rt.tokens("key"), 1, "a read arc must not be consumed");
        assert_eq!(rt.tokens("p"), 0);
        assert_eq!(rt.tokens("out"), 1);
    }

    #[test]
    fn execute_as_enforces_access_control() {
        let schema = make_simple_schema();
        let mut rt = Runtime::new(schema);
        rt.access_control = Some(pflow_metamodel::AccessControl::new(
            vec![],
            vec![pflow_metamodel::AccessRule {
                transition: "process".into(),
                roles: vec!["admin".into()],
                guard: String::new(),
            }],
        ));

        assert!(rt.execute_as("process", &["guest".to_string()]).is_err());
        assert_eq!(rt.tokens("ready"), 1, "a refused call must not fire");

        assert!(rt.execute_as("process", &["admin".to_string()]).is_ok());
        assert_eq!(rt.tokens("done"), 1);
    }

    #[test]
    fn access_allows_defaults_open_with_no_access_control() {
        let schema = make_simple_schema();
        let rt = Runtime::new(schema);
        assert!(rt.access_allows("process", &[]));
    }
}
