//! Disjoint-set over element IDs whose representative is always the
//! lexicographically smallest member of its set. Ported from go-pflow's
//! `metamodel/unionfind.go`.
//!
//! Composition needs that property specifically: linking A->B and B->C must
//! produce one three-element class regardless of the order the links appear
//! in, so choosing the smallest member as the canonical name makes `Flatten`
//! associative.

use std::collections::BTreeMap;

pub struct UnionFind {
    parent: BTreeMap<String, String>,
}

impl UnionFind {
    pub fn new() -> Self {
        UnionFind {
            parent: BTreeMap::new(),
        }
    }

    /// Registers an element as its own singleton set. Idempotent.
    pub fn add(&mut self, x: &str) {
        self.parent
            .entry(x.to_string())
            .or_insert_with(|| x.to_string());
    }

    /// Returns the canonical (smallest) member of x's set, compressing the path.
    pub fn find(&mut self, x: &str) -> String {
        self.add(x);
        let mut root = x.to_string();
        while self.parent[&root] != root {
            root = self.parent[&root].clone();
        }
        // Path compression.
        let mut cur = x.to_string();
        while self.parent[&cur] != root {
            let next = self.parent[&cur].clone();
            self.parent.insert(cur.clone(), root.clone());
            cur = next;
        }
        root
    }

    /// Merges the sets containing a and b. The smaller root wins, which is
    /// what keeps `find` returning the lexicographic minimum.
    pub fn union(&mut self, a: &str, b: &str) {
        let mut ra = self.find(a);
        let mut rb = self.find(b);
        if ra == rb {
            return;
        }
        if rb < ra {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent.insert(rb, ra);
    }

    /// Returns canonical representative -> sorted members, for every set with
    /// at least one element.
    pub fn groups(&mut self) -> BTreeMap<String, Vec<String>> {
        let keys: Vec<String> = self.parent.keys().cloned().collect();
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for x in keys {
            let root = self.find(&x);
            out.entry(root).or_default().push(x);
        }
        for members in out.values_mut() {
            members.sort();
        }
        out
    }
}

impl Default for UnionFind {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_is_transitive_and_canonical_smallest_wins() {
        let mut uf = UnionFind::new();
        for x in ["a/x", "b/y", "c/z"] {
            uf.add(x);
        }
        uf.union("a/x", "b/y");
        uf.union("b/y", "c/z");
        let groups = uf.groups();
        assert_eq!(groups.len(), 1);
        let (root, members) = groups.iter().next().unwrap();
        assert_eq!(root, "a/x");
        assert_eq!(members, &vec!["a/x".to_string(), "b/y".to_string(), "c/z".to_string()]);
    }

    #[test]
    fn order_of_union_calls_does_not_change_result() {
        let mut uf1 = UnionFind::new();
        for x in ["a", "b", "c"] {
            uf1.add(x);
        }
        uf1.union("a", "b");
        uf1.union("b", "c");

        let mut uf2 = UnionFind::new();
        for x in ["a", "b", "c"] {
            uf2.add(x);
        }
        uf2.union("b", "c");
        uf2.union("a", "b");

        assert_eq!(uf1.groups(), uf2.groups());
    }
}
