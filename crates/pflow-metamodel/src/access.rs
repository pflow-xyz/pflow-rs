//! Role-based access control, enforced in execution.
//!
//! go-pflow's own `metamodel/access.go` declares [`Role`] and [`AccessRule`]
//! (see `schema.rs`, where they are ported) and stops there — nothing in
//! go-pflow itself resolves role inheritance or checks a rule against a
//! caller's roles; that logic lives downstream, in petri-pilot's
//! `pkg/bridge/access.go` (`ResolveRoleHierarchy`, `AccessSpec.RulesForTransition`)
//! and its generated per-app `permissions.go`. This module is a from-scratch
//! Rust port of *that* enforcement shape, built directly on the [`Role`] /
//! [`AccessRule`] types both languages already share, so a Rust execution
//! path (`pflow-tokenmodel::Runtime`, `pflow-engine`) has something to call
//! rather than reimplementing role resolution a third time. There is no
//! go-pflow fixture to hold this to byte-for-byte; it is covered by unit
//! tests, the same interim state several other new-rather-than-ported checks
//! in this workspace are in (see ROADMAP.md Phase 2's dangling-arc check).
//!
//! Semantics, matching `pkg/bridge/access.go` and the generated
//! `permissions.go` pattern:
//!
//! - **No rule names a transition** (directly or via `"*"`): the transition
//!   is unrestricted, and [`AccessControl::allows`] returns `true` without
//!   needing any roles at all.
//! - **A rule names the transition with an empty `roles` list**: any
//!   authenticated caller may fire it — "authenticated" is the caller
//!   supplying at least one role of their own, mirroring the Go comment
//!   ("empty = any authenticated user").
//! - **A rule names specific roles**: the caller's *effective* roles (their
//!   own roles plus everything reachable through [`Role::inherits`],
//!   cycle-safe) must intersect the rule's `roles`.
//! - **Multiple rules can apply to one transition** (an exact match plus a
//!   `"*"` wildcard, say) — the caller is allowed if *any* applicable rule
//!   admits them, matching `RulesForTransition` returning every match rather
//!   than the first.
//! - **`AccessRule::guard`** (a state-dependent expression, e.g.
//!   `"user.id == customer_id"`) is carried through but not evaluated here:
//!   it needs a bindings/guard evaluator the way `pflow-tokenmodel::Runtime`
//!   already has one for action guards, and a caller wiring the two together
//!   is expected to evaluate it itself. [`AccessControl::rules_for_transition`]
//!   exposes the matched rules (guard text included) for exactly that.

use std::collections::{HashMap, HashSet};

use crate::error::Error;
use crate::schema::{AccessRule, Role};

/// Matches any transition when used as [`AccessRule::transition`].
pub const WILDCARD_TRANSITION: &str = "*";

/// Resolves roles and access rules against a caller's claimed roles.
#[derive(Debug, Clone, Default)]
pub struct AccessControl {
    pub roles: Vec<Role>,
    pub rules: Vec<AccessRule>,
}

impl AccessControl {
    pub fn new(roles: Vec<Role>, rules: Vec<AccessRule>) -> Self {
        Self { roles, rules }
    }

    /// A role's own id plus everything reachable through `inherits`,
    /// flattened and cycle-safe. An id with no declared [`Role`] resolves to
    /// itself alone.
    pub fn effective_roles(&self, role_id: &str) -> Vec<String> {
        let by_id: HashMap<&str, &Role> =
            self.roles.iter().map(|r| (r.id.as_str(), r)).collect();
        let mut seen: HashSet<String> = HashSet::new();
        let mut result = Vec::new();
        self.expand(role_id, &by_id, &mut seen, &mut result);
        result
    }

    fn expand<'a>(
        &'a self,
        role_id: &str,
        by_id: &HashMap<&'a str, &'a Role>,
        seen: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) {
        if !seen.insert(role_id.to_string()) {
            return; // cycle guard
        }
        out.push(role_id.to_string());
        if let Some(role) = by_id.get(role_id) {
            for parent in &role.inherits {
                self.expand(parent, by_id, seen, out);
            }
        }
    }

    /// Every declared role reachable from `user_roles`, deduplicated.
    fn all_effective(&self, user_roles: &[String]) -> HashSet<String> {
        let mut out = HashSet::new();
        for r in user_roles {
            for e in self.effective_roles(r) {
                out.insert(e);
            }
        }
        out
    }

    /// Rules that apply to `transition`: an exact match or the `"*"`
    /// wildcard, in declaration order.
    pub fn rules_for_transition(&self, transition: &str) -> Vec<&AccessRule> {
        self.rules
            .iter()
            .filter(|r| r.transition == transition || r.transition == WILDCARD_TRANSITION)
            .collect()
    }

    /// Whether `user_roles` may fire `transition`. `true` when no rule
    /// applies at all — access control is opt-in per transition.
    pub fn allows(&self, transition: &str, user_roles: &[String]) -> bool {
        self.allows_why_not(transition, user_roles).is_ok()
    }

    /// [`AccessControl::allows`] with the reason attached.
    pub fn allows_why_not(&self, transition: &str, user_roles: &[String]) -> Result<(), Error> {
        let applicable = self.rules_for_transition(transition);
        if applicable.is_empty() {
            return Ok(());
        }

        let effective = self.all_effective(user_roles);
        for rule in &applicable {
            if rule.roles.is_empty() {
                // Empty = any authenticated user: presence of a role claim
                // is the caller's proof of authentication.
                if !user_roles.is_empty() {
                    return Ok(());
                }
                continue;
            }
            if rule.roles.iter().any(|r| effective.contains(r)) {
                return Ok(());
            }
        }

        let mut allowed: Vec<String> = applicable
            .iter()
            .flat_map(|r| r.roles.clone())
            .collect();
        allowed.sort();
        allowed.dedup();

        Err(Error::AccessDenied {
            transition: transition.to_string(),
            roles: user_roles.to_vec(),
            allowed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(id: &str, inherits: &[&str]) -> Role {
        Role {
            id: id.to_string(),
            inherits: inherits.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn rule(transition: &str, roles: &[&str]) -> AccessRule {
        AccessRule {
            transition: transition.to_string(),
            roles: roles.iter().map(|s| s.to_string()).collect(),
            guard: String::new(),
        }
    }

    #[test]
    fn unrestricted_transition_allows_anyone() {
        let ac = AccessControl::new(vec![], vec![rule("approve", &["admin"])]);
        assert!(ac.allows("ship", &[]));
    }

    #[test]
    fn matching_role_allows() {
        let ac = AccessControl::new(vec![], vec![rule("approve", &["admin", "reviewer"])]);
        assert!(ac.allows("approve", &["reviewer".to_string()]));
        assert!(!ac.allows("approve", &["user".to_string()]));
    }

    #[test]
    fn inherited_role_allows() {
        let ac = AccessControl::new(
            vec![role("admin", &["user"])],
            vec![rule("post", &["user"])],
        );
        assert!(ac.allows("post", &["admin".to_string()]));
    }

    #[test]
    fn cyclic_inheritance_does_not_hang() {
        let ac = AccessControl::new(
            vec![role("a", &["b"]), role("b", &["a"])],
            vec![rule("t", &["a"])],
        );
        let effective = ac.effective_roles("a");
        assert_eq!(effective.len(), 2);
        assert!(ac.allows("t", &["b".to_string()]));
    }

    #[test]
    fn empty_roles_means_any_authenticated_user() {
        let ac = AccessControl::new(vec![], vec![rule("submit", &[])]);
        assert!(ac.allows("submit", &["anyone".to_string()]));
        assert!(!ac.allows("submit", &[]));
    }

    #[test]
    fn wildcard_rule_applies_to_every_transition() {
        let ac = AccessControl::new(vec![], vec![rule("*", &["admin"])]);
        assert!(ac.allows("anything", &["admin".to_string()]));
        assert!(!ac.allows("anything", &["guest".to_string()]));
    }

    #[test]
    fn any_applicable_rule_admitting_the_caller_is_enough() {
        let ac = AccessControl::new(
            vec![],
            vec![rule("edit", &["owner"]), rule("*", &["admin"])],
        );
        assert!(ac.allows("edit", &["admin".to_string()]));
        assert!(ac.allows("edit", &["owner".to_string()]));
        assert!(!ac.allows("edit", &["guest".to_string()]));
    }

    #[test]
    fn denial_names_who_would_be_allowed() {
        let ac = AccessControl::new(vec![], vec![rule("approve", &["admin", "reviewer"])]);
        let err = ac.allows_why_not("approve", &["guest".to_string()]).unwrap_err();
        match err {
            Error::AccessDenied { allowed, .. } => {
                assert_eq!(allowed, vec!["admin".to_string(), "reviewer".to_string()])
            }
            other => panic!("wrong error: {other}"),
        }
    }
}
