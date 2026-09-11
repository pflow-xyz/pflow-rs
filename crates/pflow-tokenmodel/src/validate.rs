//! Schema validation.

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::schema::Schema;

impl Schema {
    /// Validates the schema for structural correctness.
    pub fn validate(&self) -> Result<()> {
        let mut state_ids = HashSet::new();
        let mut action_ids = HashSet::new();

        for st in &self.states {
            if st.id.is_empty() {
                return Err(Error::EmptyId);
            }
            if !state_ids.insert(st.id.clone()) {
                return Err(Error::DuplicateId(st.id.clone()));
            }
        }

        for a in &self.actions {
            if a.id.is_empty() {
                return Err(Error::EmptyId);
            }
            if !action_ids.insert(a.id.clone()) {
                return Err(Error::DuplicateId(a.id.clone()));
            }
        }

        for arc in &self.arcs {
            let source_is_state = state_ids.contains(&arc.source);
            let source_is_action = action_ids.contains(&arc.source);
            let target_is_state = state_ids.contains(&arc.target);
            let target_is_action = action_ids.contains(&arc.target);

            if !source_is_state && !source_is_action {
                return Err(Error::InvalidArcSource(arc.source.clone()));
            }
            if !target_is_state && !target_is_action {
                return Err(Error::InvalidArcTarget(arc.target.clone()));
            }
            if (source_is_state && target_is_state) || (source_is_action && target_is_action) {
                return Err(Error::InvalidArcConnection);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::*;

    #[test]
    fn test_valid_schema() {
        let mut s = Schema::new("test");
        s.add_token_state("p1", 1);
        s.add_action(Action {
            id: "t1".into(),
            guard: String::new(),
            event_id: String::new(),
            event_bindings: None,
        });
        s.add_arc(Arc {
            source: "p1".into(),
            target: "t1".into(),
            keys: vec![],
            value: String::new(),
            ..Default::default()
        });
        assert!(s.validate().is_ok());
    }

    #[test]
    fn test_empty_id() {
        let mut s = Schema::new("test");
        s.add_state(State {
            id: String::new(),
            kind: Kind::Token,
            initial: None,
            typ: String::new(),
            exported: false,
        });
        assert!(s.validate().is_err());
    }

    #[test]
    fn test_duplicate_id() {
        let mut s = Schema::new("test");
        s.add_token_state("p1", 1);
        s.add_token_state("p1", 2);
        assert!(s.validate().is_err());
    }

    #[test]
    fn test_invalid_arc_source() {
        let mut s = Schema::new("test");
        s.add_token_state("p1", 1);
        s.add_action(Action {
            id: "t1".into(),
            guard: String::new(),
            event_id: String::new(),
            event_bindings: None,
        });
        s.add_arc(Arc {
            source: "missing".into(),
            target: "t1".into(),
            keys: vec![],
            value: String::new(),
            ..Default::default()
        });
        assert!(s.validate().is_err());
    }

    #[test]
    fn test_state_to_state_arc() {
        let mut s = Schema::new("test");
        s.add_token_state("p1", 1);
        s.add_token_state("p2", 0);
        s.add_arc(Arc {
            source: "p1".into(),
            target: "p2".into(),
            keys: vec![],
            value: String::new(),
            ..Default::default()
        });
        assert!(s.validate().is_err());
    }
}
