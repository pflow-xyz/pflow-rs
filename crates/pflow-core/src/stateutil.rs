//! State map utilities for manipulating Petri net state maps.

use std::collections::HashMap;

use crate::net::State;

/// Creates a deep copy of a state map.
pub fn copy(state: &State) -> State {
    state.clone()
}

/// Creates a new state by copying base and applying updates.
pub fn apply(base: &State, updates: &State) -> State {
    let mut out = base.clone();
    for (k, v) in updates {
        out.insert(k.clone(), *v);
    }
    out
}

/// Combines multiple state maps, with later maps taking precedence.
pub fn merge(states: &[&State]) -> State {
    let size: usize = states.iter().map(|s| s.len()).sum();
    let mut out = HashMap::with_capacity(size);
    for s in states {
        for (k, v) in *s {
            out.insert(k.clone(), *v);
        }
    }
    out
}

/// Returns true if two states have the same keys and values (exact comparison).
pub fn equal(a: &State, b: &State) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (k, v) in a {
        match b.get(k) {
            Some(bv) if *v == *bv => {}
            _ => return false,
        }
    }
    true
}

/// Returns true if two states have the same keys and values within tolerance.
pub fn equal_tol(a: &State, b: &State, tol: f64) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (k, v) in a {
        match b.get(k) {
            Some(bv) if (v - bv).abs() <= tol => {}
            _ => return false,
        }
    }
    true
}

/// Returns the value for a key, or 0 if not found.
pub fn get(state: &State, key: &str) -> f64 {
    state.get(key).copied().unwrap_or(0.0)
}

/// Returns the sum of all values in the state.
pub fn sum(state: &State) -> f64 {
    state.values().sum()
}

/// Returns the sum of values for the specified keys.
pub fn sum_keys(state: &State, keys: &[&str]) -> f64 {
    keys.iter().map(|k| state.get(*k).copied().unwrap_or(0.0)).sum()
}

/// Returns a new state with all values multiplied by factor.
pub fn scale(state: &State, factor: f64) -> State {
    state.iter().map(|(k, v)| (k.clone(), v * factor)).collect()
}

/// Returns a new state containing only keys that pass the predicate.
pub fn filter(state: &State, predicate: impl Fn(&str) -> bool) -> State {
    state
        .iter()
        .filter(|(k, _)| predicate(k))
        .map(|(k, v)| (k.clone(), *v))
        .collect()
}

/// Returns all keys in the state map.
pub fn keys(state: &State) -> Vec<String> {
    state.keys().cloned().collect()
}

/// Returns keys that have non-zero values.
pub fn non_zero(state: &State) -> Vec<String> {
    state
        .iter()
        .filter(|(_, v)| **v != 0.0)
        .map(|(k, _)| k.clone())
        .collect()
}

/// Returns a map of keys where values differ between a and b.
/// Values in the result are from state b.
pub fn diff(a: &State, b: &State) -> State {
    let mut d = State::new();

    // Changed or new keys in b
    for (k, bv) in b {
        match a.get(k) {
            Some(av) if *av == *bv => {}
            _ => {
                d.insert(k.clone(), *bv);
            }
        }
    }

    // Keys removed in b
    for k in a.keys() {
        if !b.contains_key(k) {
            d.insert(k.clone(), 0.0);
        }
    }

    d
}

/// Returns the key with the maximum value.
pub fn max(state: &State) -> Option<(String, f64)> {
    state
        .iter()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(k, v)| (k.clone(), *v))
}

/// Returns the key with the minimum value.
pub fn min(state: &State) -> Option<(String, f64)> {
    state
        .iter()
        .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(k, v)| (k.clone(), *v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(pairs: &[(&str, f64)]) -> State {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn test_copy() {
        let state = make_state(&[("A", 10.0), ("B", 5.0)]);
        let copied = copy(&state);
        assert_eq!(state, copied);
    }

    #[test]
    fn test_apply() {
        let base = make_state(&[("A", 10.0), ("B", 5.0)]);
        let updates = make_state(&[("A", 0.0), ("C", 1.0)]);
        let result = apply(&base, &updates);
        assert_eq!(result["A"], 0.0);
        assert_eq!(result["B"], 5.0);
        assert_eq!(result["C"], 1.0);
    }

    #[test]
    fn test_merge() {
        let s1 = make_state(&[("A", 1.0)]);
        let s2 = make_state(&[("B", 2.0)]);
        let s3 = make_state(&[("A", 3.0)]);
        let result = merge(&[&s1, &s2, &s3]);
        assert_eq!(result["A"], 3.0);
        assert_eq!(result["B"], 2.0);
    }

    #[test]
    fn test_equal() {
        let a = make_state(&[("A", 1.0), ("B", 2.0)]);
        let b = make_state(&[("A", 1.0), ("B", 2.0)]);
        let c = make_state(&[("A", 1.0), ("B", 3.0)]);
        assert!(equal(&a, &b));
        assert!(!equal(&a, &c));
    }

    #[test]
    fn test_equal_tol() {
        let a = make_state(&[("A", 1.0), ("B", 2.0)]);
        let b = make_state(&[("A", 1.0001), ("B", 2.0002)]);
        assert!(equal_tol(&a, &b, 0.001));
        assert!(!equal_tol(&a, &b, 0.0001));
    }

    #[test]
    fn test_sum() {
        let state = make_state(&[("A", 10.0), ("B", 5.0), ("C", 3.0)]);
        assert_eq!(sum(&state), 18.0);
    }

    #[test]
    fn test_sum_keys() {
        let state = make_state(&[("A", 10.0), ("B", 5.0), ("C", 3.0)]);
        assert_eq!(sum_keys(&state, &["A", "C"]), 13.0);
    }

    #[test]
    fn test_scale() {
        let state = make_state(&[("A", 10.0), ("B", 5.0)]);
        let scaled = scale(&state, 2.0);
        assert_eq!(scaled["A"], 20.0);
        assert_eq!(scaled["B"], 10.0);
    }

    #[test]
    fn test_filter() {
        let state = make_state(&[("_X0", 1.0), ("_X1", 1.0), ("pos", 5.0)]);
        let history = filter(&state, |k| k.starts_with('_'));
        assert_eq!(history.len(), 2);
        assert!(!history.contains_key("pos"));
    }

    #[test]
    fn test_diff() {
        let a = make_state(&[("A", 1.0), ("B", 2.0), ("C", 3.0)]);
        let b = make_state(&[("A", 1.0), ("B", 5.0), ("D", 4.0)]);
        let d = diff(&a, &b);
        assert!(!d.contains_key("A")); // unchanged
        assert_eq!(d["B"], 5.0); // changed
        assert_eq!(d["C"], 0.0); // removed
        assert_eq!(d["D"], 4.0); // added
    }

    #[test]
    fn test_max_min() {
        let state = make_state(&[("A", 10.0), ("B", 5.0), ("C", 20.0)]);
        let (mk, mv) = max(&state).unwrap();
        assert_eq!(mk, "C");
        assert_eq!(mv, 20.0);

        let (nk, nv) = min(&state).unwrap();
        assert_eq!(nk, "B");
        assert_eq!(nv, 5.0);
    }

    #[test]
    fn test_non_zero() {
        let state = make_state(&[("A", 0.0), ("B", 5.0), ("C", 0.0)]);
        let nz = non_zero(&state);
        assert_eq!(nz.len(), 1);
        assert!(nz.contains(&"B".to_string()));
    }
}
