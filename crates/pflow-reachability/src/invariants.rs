//! Minimal-support P/T invariants, ported from go-pflow's
//! `reachability/farkas.go` and `reachability/invariants.go`.
//!
//! `pflow-tropical::invariants` already computes P/T invariants from a
//! `pflow-zk::IncidenceMatrix` via Gaussian elimination over the rationals —
//! a generic integer null-space basis, used by
//! `pflow-solver::stochastic::classify_supply`. That is a different
//! algorithm answering a different question: a null-space basis is *some*
//! spanning set (its vectors can be negative, and are not unique), while the
//! Farkas/Martinez-Silva elimination here finds the *minimal-support
//! semi-positive* basis go-pflow's `validation` and `verify` packages
//! depend on — every semi-positive invariant is a non-negative combination
//! of this basis, which is exactly what `verify`'s structural proofs and
//! `validation`'s "place P is covered by a conservation law" checks need
//! and a signed null-space vector cannot give them. The two stay separate
//! per ROADMAP.md's Phase 2 note ("tropical keeps the extraction and
//! eigenvalue work that is Rust-only"); this module is the one new home for
//! the Farkas algorithm itself.

use std::collections::HashMap;

use pflow_metamodel::{ArcType, Model};

use crate::marking::Marking;

/// Caps the number of intermediate rows kept during elimination. Farkas
/// elimination is worst-case exponential in the number of transitions; a
/// limit keeps a pathological net from exhausting memory. Results found
/// before the limit is hit are still sound — see [`FarkasResult::truncated`].
pub const DEFAULT_FARKAS_LIMIT: usize = 20_000;

/// The outcome of a minimal-support invariant computation.
#[derive(Debug, Clone)]
pub struct FarkasResult {
    /// Minimal-support semi-positive invariants, each a coefficient vector
    /// indexed the same as `labels`.
    pub basis: Vec<Vec<i64>>,
    /// Names the dimension of each vector: places for P-invariants,
    /// transitions for T-invariants.
    pub labels: Vec<String>,
    /// True if the row limit was reached and the basis may be incomplete.
    /// Every returned invariant is still valid; some may be missing.
    pub truncated: bool,
}

/// A conservation law: `sum(coefficients[p] * marking[p]) == value` at every
/// reachable marking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invariant {
    /// Places with a non-zero coefficient, in the basis's label order.
    pub places: Vec<String>,
    pub coefficients: HashMap<String, i64>,
    pub value: i64,
}

impl Invariant {
    /// Renders as an equation, e.g. `"p1 + 2*p2 - p3 == 10"`. Zero
    /// coefficients (there should be none in `places`, but a caller could
    /// construct one) are skipped; an invariant with none renders as
    /// `"0 == <value>"`.
    pub fn render(&self) -> String {
        let mut order = self.places.clone();
        if order.is_empty() {
            order = self.coefficients.keys().cloned().collect();
            order.sort();
        }

        let mut expr = String::new();
        for p in &order {
            let c = *self.coefficients.get(p).unwrap_or(&0);
            if c == 0 {
                continue;
            }
            if expr.is_empty() && c < 0 {
                expr.push('-');
            } else if !expr.is_empty() {
                expr.push_str(if c < 0 { " - " } else { " + " });
            }
            let mag = c.abs();
            if mag != 1 {
                expr.push_str(&mag.to_string());
                expr.push('*');
            }
            expr.push_str(p);
        }
        if expr.is_empty() {
            expr.push('0');
        }
        format!("{expr} == {}", self.value)
    }

    /// Whether the invariant holds at `marking`.
    pub fn check(&self, marking: &Marking) -> bool {
        let sum: i64 = self
            .coefficients
            .iter()
            .map(|(p, c)| c * marking.get(p).copied().unwrap_or(0))
            .sum();
        sum == self.value
    }
}

/// A firing-count vector `x` with `C * x = 0`: firing each transition the
/// given number of times returns the net to its starting marking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TInvariant {
    pub transitions: Vec<String>,
    pub counts: HashMap<String, i64>,
}

impl TInvariant {
    /// Renders as a firing multiset, e.g. `"{produce, 2*consume}"`.
    pub fn render(&self) -> String {
        let terms: Vec<String> = self
            .transitions
            .iter()
            .filter_map(|t| {
                let c = *self.counts.get(t).unwrap_or(&0);
                if c == 0 {
                    None
                } else if c == 1 {
                    Some(t.clone())
                } else {
                    Some(format!("{c}*{t}"))
                }
            })
            .collect();
        format!("{{{}}}", terms.join(", "))
    }
}

/// Computes P/T invariants over a model's incidence matrix.
pub struct InvariantAnalyzer<'m> {
    model: &'m Model,
}

impl<'m> InvariantAnalyzer<'m> {
    pub fn new(model: &'m Model) -> Self {
        InvariantAnalyzer { model }
    }

    /// The incidence matrix `C[place][transition] = output_weight -
    /// input_weight`, with places and transitions in the token-place-only,
    /// declaration order the model already has (sorted for determinism, as
    /// Go's map-backed net requires but this crate's ordered `Vec`s do not
    /// strictly need — kept anyway so the row/column order matches Go's
    /// output exactly). Inhibitor and read arcs move no tokens and are
    /// excluded, matching `Model::inputs`/`outputs`.
    pub fn incidence_matrix(&self) -> (Vec<Vec<i64>>, Vec<String>, Vec<String>) {
        let mut places: Vec<String> = self
            .model
            .places
            .iter()
            .filter(|p| p.is_token())
            .map(|p| p.id.clone())
            .collect();
        places.sort();

        let mut transitions: Vec<String> =
            self.model.transitions.iter().map(|t| t.id.clone()).collect();
        transitions.sort();

        let place_idx: HashMap<&str, usize> =
            places.iter().enumerate().map(|(i, p)| (p.as_str(), i)).collect();
        let trans_idx: HashMap<&str, usize> =
            transitions.iter().enumerate().map(|(i, t)| (t.as_str(), i)).collect();

        let mut matrix = vec![vec![0i64; transitions.len()]; places.len()];
        for arc in &self.model.arcs {
            if arc.typ != ArcType::Normal {
                continue;
            }
            let w = arc.effective_weight();
            if let (Some(&pi), Some(&ti)) = (place_idx.get(arc.from.as_str()), trans_idx.get(arc.to.as_str())) {
                matrix[pi][ti] -= w;
            } else if let (Some(&ti), Some(&pi)) =
                (trans_idx.get(arc.from.as_str()), place_idx.get(arc.to.as_str()))
            {
                matrix[pi][ti] += w;
            }
        }

        (matrix, places, transitions)
    }

    /// The raw minimal-support P-invariant basis.
    pub fn p_invariant_basis(&self) -> FarkasResult {
        let (matrix, places, transitions) = self.incidence_matrix();
        let (basis, truncated) = farkas(&matrix, places.len(), transitions.len(), DEFAULT_FARKAS_LIMIT);
        FarkasResult { basis, labels: places, truncated }
    }

    /// The raw minimal-support T-invariant basis: runs Farkas elimination
    /// over the transposed incidence matrix, since `C * x = 0` over
    /// transitions is `y * C^T = 0` with `y` indexed by transitions.
    pub fn t_invariant_basis(&self) -> FarkasResult {
        let (matrix, places, transitions) = self.incidence_matrix();
        let transposed = transpose(&matrix, places.len(), transitions.len());
        let (basis, truncated) =
            farkas(&transposed, transitions.len(), places.len(), DEFAULT_FARKAS_LIMIT);
        FarkasResult { basis, labels: transitions, truncated }
    }

    /// The net's P-invariants, each carrying the constant evaluated at
    /// `initial`.
    pub fn find_p_invariants(&self, initial: &Marking) -> Vec<Invariant> {
        let res = self.p_invariant_basis();
        res.basis
            .iter()
            .filter_map(|vec| {
                let mut coefficients = HashMap::new();
                let mut places = Vec::new();
                let mut value = 0;
                for (i, &c) in vec.iter().enumerate() {
                    if c == 0 {
                        continue;
                    }
                    let place = res.labels[i].clone();
                    value += c * initial.get(&place).copied().unwrap_or(0);
                    coefficients.insert(place.clone(), c);
                    places.push(place);
                }
                if places.is_empty() {
                    None
                } else {
                    Some(Invariant { places, coefficients, value })
                }
            })
            .collect()
    }

    /// The net's T-invariants.
    pub fn find_t_invariants(&self) -> Vec<TInvariant> {
        let res = self.t_invariant_basis();
        res.basis
            .iter()
            .filter_map(|vec| {
                let mut counts = HashMap::new();
                let mut transitions = Vec::new();
                for (i, &c) in vec.iter().enumerate() {
                    if c == 0 {
                        continue;
                    }
                    counts.insert(res.labels[i].clone(), c);
                    transitions.push(res.labels[i].clone());
                }
                if transitions.is_empty() {
                    None
                } else {
                    Some(TInvariant { transitions, counts })
                }
            })
            .collect()
    }

    /// True if every place is covered (positive coefficient) by some
    /// semi-positive P-invariant — a sufficient (not necessary) condition
    /// for structural boundedness, for *any* initial marking. Go's
    /// `InvariantAnalyzer.StructuralBoundedness`.
    pub fn structural_boundedness(&self) -> bool {
        let res = self.p_invariant_basis();
        if res.labels.is_empty() {
            return true;
        }
        let mut covered = vec![false; res.labels.len()];
        for vec in &res.basis {
            for (i, &c) in vec.iter().enumerate() {
                if c > 0 {
                    covered[i] = true;
                }
            }
        }
        covered.into_iter().all(|c| c)
    }
}

fn transpose(matrix: &[Vec<i64>], rows: usize, cols: usize) -> Vec<Vec<i64>> {
    let mut t = vec![vec![0i64; rows]; cols];
    for (i, row) in matrix.iter().enumerate().take(rows) {
        for (j, &v) in row.iter().enumerate().take(cols) {
            t[j][i] = v;
        }
    }
    t
}

/// One row of the augmented matrix `[C | I]` during elimination. `coef` is
/// the partially eliminated matrix row; `annot` accumulates which linear
/// combination of the original rows produced it. When `coef` reaches the
/// zero vector, `annot` is an invariant.
#[derive(Clone)]
struct FarkasRow {
    coef: Vec<i64>,
    annot: Vec<i64>,
}

/// Computes the minimal-support semi-positive solutions of `y*M = 0`
/// (`y >= 0`) via Farkas/Martinez-Silva elimination. `matrix` is
/// `row_count` by `col_count`; returned vectors are indexed over rows.
/// Ported from Go's `farkas`.
fn farkas(matrix: &[Vec<i64>], row_count: usize, col_count: usize, limit: usize) -> (Vec<Vec<i64>>, bool) {
    if row_count == 0 {
        return (Vec::new(), false);
    }
    let limit = if limit == 0 { DEFAULT_FARKAS_LIMIT } else { limit };

    let mut rows: Vec<FarkasRow> = (0..row_count)
        .map(|i| {
            let mut annot = vec![0i64; row_count];
            annot[i] = 1;
            FarkasRow { coef: matrix[i].clone(), annot }
        })
        .collect();

    let mut truncated = false;

    for j in 0..col_count {
        let mut zero = Vec::new();
        let mut pos = Vec::new();
        let mut neg = Vec::new();
        for r in rows {
            match r.coef[j].cmp(&0) {
                std::cmp::Ordering::Greater => pos.push(r),
                std::cmp::Ordering::Less => neg.push(r),
                std::cmp::Ordering::Equal => zero.push(r),
            }
        }

        let mut next = zero;
        'outer: for p in &pos {
            for n in &neg {
                if next.len() >= limit {
                    truncated = true;
                    break 'outer;
                }
                // Scale so the column-j entries cancel exactly:
                //   a = -n.coef[j] > 0, b = p.coef[j] > 0
                let a = -n.coef[j];
                let b = p.coef[j];
                next.push(combine_rows(p, n, a, b));
            }
        }

        rows = minimal_rows(next);
    }

    let mut basis: Vec<Vec<i64>> = rows
        .into_iter()
        .map(|r| r.annot)
        .filter(|a| a.iter().any(|&v| v != 0))
        .collect();
    sort_vectors(&mut basis);
    (basis, truncated)
}

/// `a*p + b*n`, reduced by the gcd of all its entries so invariants come
/// back in lowest terms.
fn combine_rows(p: &FarkasRow, n: &FarkasRow, a: i64, b: i64) -> FarkasRow {
    let coef: Vec<i64> = p.coef.iter().zip(&n.coef).map(|(&pc, &nc)| a * pc + b * nc).collect();
    let annot: Vec<i64> = p.annot.iter().zip(&n.annot).map(|(&pa, &na)| a * pa + b * na).collect();

    let mut g = 0i64;
    for &v in coef.iter().chain(annot.iter()) {
        g = gcd(g, v.abs());
    }
    if g > 1 {
        let coef = coef.iter().map(|v| v / g).collect();
        let annot = annot.iter().map(|v| v / g).collect();
        return FarkasRow { coef, annot };
    }
    FarkasRow { coef, annot }
}

/// Removes duplicate annotations and any row whose annotation support
/// strictly contains another row's — the minimality criterion that keeps
/// the basis from filling up with sums of simpler invariants.
fn minimal_rows(rows: Vec<FarkasRow>) -> Vec<FarkasRow> {
    if rows.len() <= 1 {
        return rows;
    }

    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();
    for r in rows {
        let key = r.annot.clone();
        if seen.insert(key) {
            unique.push(r);
        }
    }

    let supports: Vec<std::collections::HashSet<usize>> = unique
        .iter()
        .map(|r| r.annot.iter().enumerate().filter(|(_, &v)| v != 0).map(|(i, _)| i).collect())
        .collect();

    let mut result = Vec::new();
    for (i, r) in unique.into_iter().enumerate() {
        let minimal = !(0..supports.len()).any(|k| {
            k != i && supports[k].len() < supports[i].len() && supports[k].is_subset(&supports[i])
        });
        if minimal {
            result.push(r);
        }
    }
    result
}

/// Orders a basis deterministically: fewer non-zero entries first, then
/// lexicographically.
fn sort_vectors(vs: &mut [Vec<i64>]) {
    vs.sort_by(|a, b| {
        let sa = a.iter().filter(|&&x| x != 0).count();
        let sb = b.iter().filter(|&&x| x != 0).count();
        sa.cmp(&sb).then_with(|| a.cmp(b))
    });
}

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_metamodel::schema::{Arc, ArcType as AT, Place, Transition};

    /// A simple two-place cycle: p0 <-> p1, so p0+p1 is conserved.
    fn cycle_model() -> Model {
        Model {
            name: "cycle".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
            ],
            transitions: vec![
                Transition { id: "t0".into(), ..Default::default() },
                Transition { id: "t1".into(), ..Default::default() },
            ],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
                Arc { from: "p1".into(), to: "t1".into(), ..Default::default() },
                Arc { from: "t1".into(), to: "p0".into(), ..Default::default() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn p_invariant_covers_both_places() {
        let model = cycle_model();
        let analyzer = InvariantAnalyzer::new(&model);
        let initial = model.initial_marking();
        let invs = analyzer.find_p_invariants(&initial);
        assert_eq!(invs.len(), 1);
        assert_eq!(invs[0].value, 1);
        assert!(invs[0].check(&initial));
        assert_eq!(invs[0].render(), "p0 + p1 == 1");
    }

    #[test]
    fn t_invariant_fires_the_whole_cycle() {
        let model = cycle_model();
        let analyzer = InvariantAnalyzer::new(&model);
        let invs = analyzer.find_t_invariants();
        assert_eq!(invs.len(), 1);
        assert_eq!(invs[0].transitions.len(), 2);
    }

    #[test]
    fn structural_boundedness_true_for_cycle() {
        let model = cycle_model();
        let analyzer = InvariantAnalyzer::new(&model);
        assert!(analyzer.structural_boundedness());
    }

    #[test]
    fn structural_boundedness_false_for_a_source() {
        // A place with only outputs and no inputs: no P-invariant covers it.
        let model = Model {
            name: "source".into(),
            places: vec![Place { id: "p0".into(), ..Default::default() }],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![Arc { from: "t0".into(), to: "p0".into(), ..Default::default() }],
            ..Default::default()
        };
        let analyzer = InvariantAnalyzer::new(&model);
        assert!(!analyzer.structural_boundedness());
    }

    #[test]
    fn inhibitor_arcs_do_not_enter_the_incidence_matrix() {
        let model = Model {
            name: "inhib".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "guard".into(), ..Default::default() },
            ],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "guard".into(), to: "t0".into(), typ: AT::Inhibitor, weight: 1, ..Default::default() },
            ],
            ..Default::default()
        };
        let analyzer = InvariantAnalyzer::new(&model);
        let (matrix, places, _) = analyzer.incidence_matrix();
        let guard_idx = places.iter().position(|p| p == "guard").unwrap();
        assert!(matrix[guard_idx].iter().all(|&v| v == 0));
    }
}
