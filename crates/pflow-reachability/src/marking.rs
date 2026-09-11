//! Marking helpers, ported from go-pflow's `reachability/marking.go`.
//!
//! The marking type itself is [`pflow_metamodel::firing::Marking`]
//! (`HashMap<String, i64>`) — reused rather than re-declared, since
//! [`pflow_metamodel::Model::enabled`]/`fire` already operate on it and a
//! second marking type would need conversions at every call into the firing
//! rule. Everything here is free functions rather than inherent methods,
//! since Rust does not allow adding methods to a type alias for a foreign
//! type.

use std::collections::BTreeSet;

pub use pflow_metamodel::firing::Marking;

/// Place names present in the marking, sorted. Go's `Marking.SortedKeys`.
pub fn sorted_keys(m: &Marking) -> Vec<String> {
    let mut keys: Vec<String> = m.keys().cloned().collect();
    keys.sort();
    keys
}

/// A deterministic string key for a marking, used to identify graph states.
/// Not sha256 like Go's `Marking.Hash` (that exists only to keep Go's map
/// keys short); any injective, deterministic rendering works as a Rust
/// `HashMap` key, and a plain sorted `"place:value"` join is cheaper and
/// exposes the same information under debugging.
pub fn key(m: &Marking) -> String {
    let mut s = String::new();
    for k in sorted_keys(m) {
        s.push_str(&k);
        s.push(':');
        s.push_str(&m[&k].to_string());
        s.push(',');
    }
    s
}

/// A human-readable rendering, e.g. `"p0:2, p1:1"`. Places with zero tokens
/// are omitted; an all-zero marking renders as `"(empty)"`. Go's
/// `Marking.String`.
pub fn render(m: &Marking) -> String {
    let parts: Vec<String> = sorted_keys(m)
        .into_iter()
        .filter(|k| m[k] != 0)
        .map(|k| format!("{}:{}", k, m[&k]))
        .collect();
    if parts.is_empty() {
        "(empty)".to_string()
    } else {
        parts.join(", ")
    }
}

/// The sum of all tokens. Go's `Marking.Total`.
pub fn total(m: &Marking) -> i64 {
    m.values().sum()
}

/// The largest token count in any place (0 for an empty marking). Go's
/// `Marking.Max`.
pub fn max(m: &Marking) -> i64 {
    m.values().copied().max().unwrap_or(0)
}

/// True when every place holds zero tokens. Go's `Marking.IsZero`.
pub fn is_zero(m: &Marking) -> bool {
    m.values().all(|&v| v == 0)
}

/// `m` covers `other`: at least as many tokens everywhere `other` names.
/// Places `other` does not name are unconstrained. Go's `Marking.Covers`.
pub fn covers(m: &Marking, other: &Marking) -> bool {
    other.iter().all(|(k, &v)| *m.get(k).unwrap_or(&0) >= v)
}

/// `m` covers `other` and is strictly greater somewhere `other` names. Go's
/// `Marking.StrictlyCovers`.
pub fn strictly_covers(m: &Marking, other: &Marking) -> bool {
    covers(m, other) && other.iter().any(|(k, &v)| *m.get(k).unwrap_or(&0) > v)
}

/// The places whose count strictly increases from `from` to `to`, sorted.
/// Go's (private) `growingPlaces`, used by the coverability witness.
pub fn growing_places(from: &Marking, to: &Marking) -> Vec<String> {
    let mut keys: BTreeSet<String> = BTreeSet::new();
    keys.extend(from.keys().cloned());
    keys.extend(to.keys().cloned());
    keys.into_iter()
        .filter(|k| to.get(k).copied().unwrap_or(0) > from.get(k).copied().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, i64)]) -> Marking {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn key_is_order_independent_and_deterministic() {
        let a = m(&[("b", 1), ("a", 2)]);
        let b = m(&[("a", 2), ("b", 1)]);
        assert_eq!(key(&a), key(&b));
        assert_eq!(key(&a), "a:2,b:1,");
    }

    #[test]
    fn render_omits_zero_places() {
        let a = m(&[("a", 0), ("b", 3)]);
        assert_eq!(render(&a), "b:3");
        let empty = m(&[("a", 0)]);
        assert_eq!(render(&empty), "(empty)");
    }

    #[test]
    fn covers_and_strictly_covers() {
        let a = m(&[("a", 2), ("b", 1)]);
        let b = m(&[("a", 1), ("b", 1)]);
        assert!(covers(&a, &b));
        assert!(strictly_covers(&a, &b));
        assert!(!strictly_covers(&a, &a));
        assert!(covers(&a, &a));
    }

    #[test]
    fn growing_places_reports_strict_increases_only() {
        let from = m(&[("a", 1), ("b", 2)]);
        let to = m(&[("a", 1), ("b", 3), ("c", 1)]);
        assert_eq!(growing_places(&from, &to), vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn total_max_is_zero() {
        let a = m(&[("a", 2), ("b", 3)]);
        assert_eq!(total(&a), 5);
        assert_eq!(max(&a), 3);
        assert!(!is_zero(&a));
        let z = m(&[("a", 0)]);
        assert!(is_zero(&z));
    }
}
