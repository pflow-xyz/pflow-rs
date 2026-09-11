//! `GuardLink` lowering, ported from go-pflow's `metamodel/compose_guard.go`.
//!
//! A guard link gates a transition in one subnet on a place in another
//! without consuming from it. The structural lowering (read/inhibitor arcs)
//! is preferred wherever it applies — `reachability` and `verify` can see
//! it — and the guard-expression fallback is opaque to both.

use crate::bundle::{Link, LOWERING_AUTO, LOWERING_EXPR, LOWERING_INHIBITOR, LOWERING_STRUCTURAL};

/// Parses a guard-link condition over a place's token count, e.g. `"> 0"`,
/// `"== 0"`, `">= 3"`. An empty condition means `"> 0"`.
pub fn parse_condition(cond: &str) -> Result<(String, i64), String> {
    let cond = cond.trim();
    if cond.is_empty() {
        return Ok((">".to_string(), 0));
    }
    // Longest operators first, so ">=" is not read as ">".
    for candidate in [">=", "<=", "==", "!=", ">", "<"] {
        if let Some(rest) = cond.strip_prefix(candidate) {
            let rest = rest.trim();
            let v: i64 = rest
                .parse()
                .map_err(|_| format!("condition {cond:?}: {rest:?} is not an integer token count"))?;
            if v < 0 {
                return Err(format!("condition {cond:?}: a token count cannot be negative"));
            }
            return Ok((candidate.to_string(), v));
        }
    }
    Err(format!(
        "condition {cond:?} must start with one of >=, <=, ==, !=, >, < (for example \"> 0\")"
    ))
}

/// The chosen lowering of one guard link. When `strategy` is
/// [`LOWERING_STRUCTURAL`], `read`/`inhibit` are the arc weights to emit; a
/// zero weight means "no arc of that kind".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredGuard {
    pub strategy: String,
    pub read: i64,
    pub inhibit: i64,
}

/// The operator table: which read/inhibitor arcs express `"tokens(p) op n"`
/// exactly.
fn structural_arcs(op: &str, n: i64) -> (i64, i64, bool) {
    match op {
        ">=" => {
            if n == 0 {
                (0, 0, true)
            } else {
                (n, 0, true)
            }
        }
        ">" => (n + 1, 0, true),
        "<" => {
            if n == 0 {
                (0, 0, false)
            } else {
                (0, n, true)
            }
        }
        "<=" => (0, n + 1, true),
        "==" => {
            if n == 0 {
                (0, 1, true)
            } else {
                (n, n + 1, true)
            }
        }
        _ => (0, 0, false), // "!="
    }
}

/// Picks the lowering strategy for a guard link, rejecting an explicit
/// choice that the condition cannot support.
pub fn resolve_lowering(l: &Link) -> Result<LoweredGuard, String> {
    let (op, n) = parse_condition(&l.condition)?;
    let (read, inhibit, structural) = structural_arcs(&op, n);

    match l.lowering.as_str() {
        "" | LOWERING_AUTO => {
            if structural {
                Ok(LoweredGuard {
                    strategy: LOWERING_STRUCTURAL.to_string(),
                    read,
                    inhibit,
                })
            } else {
                Ok(LoweredGuard {
                    strategy: LOWERING_EXPR.to_string(),
                    read: 0,
                    inhibit: 0,
                })
            }
        }
        LOWERING_EXPR => Ok(LoweredGuard {
            strategy: LOWERING_EXPR.to_string(),
            read: 0,
            inhibit: 0,
        }),
        LOWERING_STRUCTURAL => {
            if !structural {
                return Err(format!(
                    "lowering {LOWERING_STRUCTURAL:?} cannot express condition {:?}; only {LOWERING_EXPR:?} can",
                    l.condition
                ));
            }
            Ok(LoweredGuard {
                strategy: LOWERING_STRUCTURAL.to_string(),
                read,
                inhibit,
            })
        }
        LOWERING_INHIBITOR => {
            if !structural || read != 0 {
                return Err(format!(
                    "lowering {LOWERING_INHIBITOR:?} needs an upper-bound condition such as \"== 0\", \"< n\" or \"<= n\" (an inhibitor arc cannot express a lower bound; use {LOWERING_STRUCTURAL:?}), got {:?}",
                    l.condition
                ));
            }
            Ok(LoweredGuard {
                strategy: LOWERING_STRUCTURAL.to_string(),
                read: 0,
                inhibit,
            })
        }
        other => Err(format!(
            "unknown lowering {other:?} (want {LOWERING_AUTO:?}, {LOWERING_EXPR:?}, {LOWERING_STRUCTURAL:?} or {LOWERING_INHIBITOR:?})"
        )),
    }
}

/// Renders the expression form of a guard-link condition against a flat
/// place ID.
pub fn guard_conjunct(flat_place: &str, cond: &str) -> Result<String, String> {
    let (op, n) = parse_condition(cond)?;
    Ok(format!("tokens({flat_place:?}) {op} {n}"))
}

/// Combines guard expressions with `&&`, skipping empties and
/// parenthesising each so precedence cannot change meaning.
pub fn and_guards(exprs: &[String]) -> String {
    let parts: Vec<String> = exprs
        .iter()
        .map(|e| e.trim())
        .filter(|e| !e.is_empty())
        .map(|e| format!("({e})"))
        .collect();
    match parts.len() {
        0 => String::new(),
        1 => {
            // Keep a lone guard unwrapped so single-subnet output stays
            // identical to the input.
            let p = &parts[0];
            p.strip_prefix('(')
                .and_then(|p| p.strip_suffix(')'))
                .unwrap_or(p)
                .to_string()
        }
        _ => parts.join(" && "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_and_explicit_conditions() {
        assert_eq!(parse_condition("").unwrap(), (">".to_string(), 0));
        assert_eq!(parse_condition(">= 2").unwrap(), (">=".to_string(), 2));
        assert_eq!(parse_condition("== 0").unwrap(), ("==".to_string(), 0));
    }

    #[test]
    fn rejects_negative_and_non_integer() {
        assert!(parse_condition(">= -1").is_err());
        assert!(parse_condition(">= x").is_err());
    }

    #[test]
    fn structural_table_matches_go_reference() {
        assert_eq!(structural_arcs(">=", 0), (0, 0, true));
        assert_eq!(structural_arcs(">=", 3), (3, 0, true));
        assert_eq!(structural_arcs(">", 0), (1, 0, true));
        assert_eq!(structural_arcs("<", 0), (0, 0, false));
        assert_eq!(structural_arcs("<", 4), (0, 4, true));
        assert_eq!(structural_arcs("<=", 3), (0, 4, true));
        assert_eq!(structural_arcs("==", 0), (0, 1, true));
        assert_eq!(structural_arcs("==", 3), (3, 4, true));
        assert_eq!(structural_arcs("!=", 3), (0, 0, false));
    }

    #[test]
    fn auto_lowering_prefers_structural() {
        let l = Link {
            condition: ">= 2".to_string(),
            ..Default::default()
        };
        let lowered = resolve_lowering(&l).unwrap();
        assert_eq!(lowered.strategy, LOWERING_STRUCTURAL);
        assert_eq!(lowered.read, 2);
        assert_eq!(lowered.inhibit, 0);
    }

    #[test]
    fn auto_lowering_falls_back_to_expr_for_not_equal() {
        let l = Link {
            condition: "!= 3".to_string(),
            ..Default::default()
        };
        let lowered = resolve_lowering(&l).unwrap();
        assert_eq!(lowered.strategy, LOWERING_EXPR);
    }

    #[test]
    fn inhibitor_lowering_rejects_lower_bound() {
        let l = Link {
            condition: ">= 2".to_string(),
            lowering: LOWERING_INHIBITOR.to_string(),
            ..Default::default()
        };
        assert!(resolve_lowering(&l).is_err());
    }

    #[test]
    fn and_guards_unwraps_a_single_guard() {
        assert_eq!(and_guards(&["a > 0".to_string()]), "a > 0");
        assert_eq!(
            and_guards(&["a > 0".to_string(), "b > 0".to_string()]),
            "(a > 0) && (b > 0)"
        );
        assert_eq!(and_guards(&[]), "");
    }

    #[test]
    fn guard_conjunct_renders_tokens_call() {
        assert_eq!(guard_conjunct("beans", ">= 2").unwrap(), r#"tokens("beans") >= 2"#);
    }
}
