//! Runtime execution of token model schemas.

use std::collections::HashMap;
use std::fmt;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::schema::Schema;
use crate::snapshot::{Bindings, BindingsExt, Snapshot};

/// Evaluates guard expressions.
pub trait GuardEvaluator {
    fn evaluate(
        &self,
        expr: &str,
        bindings: &Bindings,
    ) -> std::result::Result<bool, String>;

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
        }
    }

    pub fn clone_runtime(&self) -> Self {
        Self {
            schema: self.schema.clone(),
            snapshot: self.snapshot.clone(),
            sequence: self.sequence,
            check_constraints: self.check_constraints,
            guard_evaluator: None, // Guard evaluator is not cloned
        }
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

    /// Returns true if an action can execute.
    pub fn enabled(&self, action_id: &str) -> bool {
        if self.schema.action_by_id(action_id).is_none() {
            return false;
        }

        for arc in self.schema.input_arcs(action_id) {
            if let Some(st) = self.schema.state_by_id(&arc.source) {
                if st.is_token() && self.tokens(&arc.source) < 1 {
                    return false;
                }
            }
        }

        true
    }

    /// Returns all actions that can execute.
    pub fn enabled_actions(&self) -> Vec<String> {
        self.schema
            .actions
            .iter()
            .filter(|a| self.enabled(&a.id))
            .map(|a| a.id.clone())
            .collect()
    }

    /// Executes an action (token semantics only).
    pub fn execute(&mut self, action_id: &str) -> Result<()> {
        if !self.enabled(action_id) {
            return Err(Error::ActionNotEnabled(action_id.to_string()));
        }

        // Process input arcs
        for arc in self.schema.input_arcs(action_id) {
            if let Some(st) = self.schema.state_by_id(&arc.source) {
                if st.is_token() {
                    self.snapshot.add_tokens(&arc.source, -1);
                }
            }
        }

        // Process output arcs
        for arc in self.schema.output_arcs(action_id) {
            if let Some(st) = self.schema.state_by_id(&arc.target) {
                if st.is_token() {
                    self.snapshot.add_tokens(&arc.target, 1);
                }
            }
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
    pub fn execute_with_bindings(
        &mut self,
        action_id: &str,
        bindings: &Bindings,
    ) -> Result<()> {
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

            if is_token {
                self.snapshot.add_tokens(&arc.source, -1);
            } else {
                self.apply_data_arc(&arc.source, arc, bindings, false);
            }
        }

        for arc in &output_arcs {
            let is_token = self
                .schema
                .state_by_id(&arc.target)
                .map(|s| s.is_token())
                .unwrap_or(false);

            if is_token {
                self.snapshot.add_tokens(&arc.target, 1);
            } else {
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
        });
        s.add_arc(Arc {
            source: "process".into(),
            target: "done".into(),
            keys: vec![],
            value: String::new(),
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
        });
        schema.add_arc(Arc {
            source: "transfer".into(),
            target: "balances".into(),
            keys: vec!["to".into()],
            value: String::new(),
        });

        let mut rt = Runtime::new(schema);

        // Set initial balance
        rt.snapshot.set_data_map_value(
            "balances",
            "alice",
            Value::Number(100.into()),
        );

        let mut bindings = Bindings::new();
        bindings.insert("from".into(), Value::String("alice".into()));
        bindings.insert("to".into(), Value::String("bob".into()));
        bindings.insert("amount".into(), Value::Number(30.into()));

        rt.execute_with_bindings("transfer", &bindings).unwrap();

        assert_eq!(rt.get_map_i64("balances", "alice"), 70);
        assert_eq!(rt.get_map_i64("balances", "bob"), 30);
    }
}
