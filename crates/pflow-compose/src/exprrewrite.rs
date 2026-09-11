//! Rewriting quoted place references inside guard/invariant/objective
//! expressions, ported from go-pflow's `metamodel/exprrewrite.go`.

use regex::{Captures, Regex};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Matches an aggregate call whose sole argument is a quoted place
/// reference: `tokens("p")`, `sum('balances')`, `count("q")`, `minOf("x")`,
/// `min("x")`. Two guard dialects are covered — petri-pilot's `pkg/dsl`
/// (`sum`/`count`/`tokens`/`minOf`/`maxOf`) and go-pflow's
/// `tokenmodel/guard` (`min`/`max`) — and both quote styles appear in the
/// wild.
fn place_ref_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"\b(tokens|sum|count|minOf|maxOf|min|max)\(\s*(?:"([^"]*)"|'([^']*)')\s*\)"#)
            .expect("static regex")
    })
}

/// Returns the place IDs a guard, invariant or objective expression
/// references, in order of first appearance and deduplicated.
pub fn place_refs(expr: &str) -> Vec<String> {
    if expr.is_empty() {
        return Vec::new();
    }
    let mut refs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for caps in place_ref_re().captures_iter(expr) {
        let ref_ = ref_from_captures(&caps);
        if ref_.is_empty() || seen.contains(ref_) {
            continue;
        }
        seen.insert(ref_.to_string());
        refs.push(ref_.to_string());
    }
    refs
}

fn ref_from_captures<'a>(caps: &'a Captures<'a>) -> &'a str {
    match caps.get(2) {
        Some(m) if !m.as_str().is_empty() => m.as_str(),
        _ => caps.get(3).map(|m| m.as_str()).unwrap_or(""),
    }
}

/// Rewrites quoted place references in a guard, invariant or objective
/// expression so it keeps meaning after flattening.
///
/// `exact` maps a subnet-local place ID to its flat ID and takes priority.
/// Anything not in `exact` is prefixed instead, matching sum/count's
/// prefix-match semantics on the Go side.
pub fn rewrite_place_refs(expr: &str, exact: &HashMap<String, String>, prefix: &str) -> String {
    if expr.is_empty() {
        return expr.to_string();
    }
    let re = place_ref_re();
    let mut out = String::with_capacity(expr.len());
    let mut last = 0;
    for caps in re.captures_iter(expr) {
        let m = caps.get(0).unwrap();
        out.push_str(&expr[last..m.start()]);
        last = m.end();

        let fn_name = caps.get(1).unwrap().as_str();
        let (quote, ref_) = match caps.get(2) {
            Some(g) if !g.as_str().is_empty() => ("\"", g.as_str()),
            _ => match caps.get(3) {
                Some(g) if !g.as_str().is_empty() => ("'", g.as_str()),
                _ => {
                    // Empty argument: leave the match untouched.
                    out.push_str(m.as_str());
                    continue;
                }
            },
        };

        let replacement = if let Some(flat) = exact.get(ref_) {
            flat.clone()
        } else if !prefix.is_empty() {
            format!("{prefix}{ref_}")
        } else {
            ref_.to_string()
        };
        out.push_str(fn_name);
        out.push('(');
        out.push_str(quote);
        out.push_str(&replacement);
        out.push_str(quote);
        out.push(')');
    }
    out.push_str(&expr[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_exact_and_prefixed() {
        let mut exact = HashMap::new();
        exact.insert("balances".to_string(), "wire:orders/balances".to_string());
        let out = rewrite_place_refs(r#"sum("balances") + tokens("other") == 5"#, &exact, "orders/");
        assert_eq!(out, r#"sum("wire:orders/balances") + tokens("orders/other") == 5"#);
    }

    #[test]
    fn single_quotes_round_trip() {
        let exact = HashMap::new();
        let out = rewrite_place_refs("min('x') > 0", &exact, "ns/");
        assert_eq!(out, "min('ns/x') > 0");
    }

    #[test]
    fn empty_argument_is_left_alone() {
        let exact = HashMap::new();
        let out = rewrite_place_refs(r#"sum("")"#, &exact, "ns/");
        assert_eq!(out, r#"sum("")"#);
    }

    #[test]
    fn place_refs_dedupes_in_order() {
        let refs = place_refs(r#"tokens("a") + tokens("b") + tokens("a")"#);
        assert_eq!(refs, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn empty_expr_yields_no_refs() {
        assert!(place_refs("").is_empty());
    }
}
