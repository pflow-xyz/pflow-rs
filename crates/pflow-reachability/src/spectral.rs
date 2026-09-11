//! Eigenvector centrality over a Petri net's bipartite graph, ported from
//! go-pflow's `reachability/spectral.go`.
//!
//! Go repeats the same power-iteration loop in four functions
//! (`EigenvectorCentrality`, `ProjectedCentrality`,
//! `EntityConstraintCentrality`, `MatrixCentrality`); here it is factored
//! into one [`power_iteration`] all four call, which is the one behavioral
//! difference from the source — same numbers, one fewer copy to drift.

use std::collections::HashMap;

use pflow_metamodel::{ArcType, Model};

/// Eigenvector centrality results for a set of labeled nodes.
#[derive(Debug, Clone)]
pub struct SpectralResult {
    /// Node labels in canonical order.
    pub labels: Vec<String>,
    pub centrality: HashMap<String, f64>,
    /// The dominant eigenvalue (spectral radius).
    pub eigenvalue: f64,
    /// Power iteration steps used.
    pub iterations: usize,
    /// Final residual.
    pub convergence: f64,
}

/// Power iteration for the dominant eigenvector of a non-negative symmetric
/// matrix `m` (`n x n`), starting from the uniform vector. Shared by every
/// function in this module.
fn power_iteration(m: &[Vec<f64>], n: usize, max_iter: usize, tol: f64) -> (Vec<f64>, f64, usize, f64) {
    let mut v = vec![1.0 / (n as f64).sqrt(); n];
    let mut eigenvalue = 0.0;
    let mut residual = 0.0;
    let mut iter = 0;

    while iter < max_iter {
        let mut w = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                w[i] += m[i][j] * v[j];
            }
        }

        eigenvalue = w.iter().map(|x| x * x).sum::<f64>().sqrt();
        if eigenvalue < 1e-15 {
            break;
        }
        for x in w.iter_mut() {
            *x /= eigenvalue;
        }

        residual = w.iter().zip(&v).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt();
        v = w;
        iter += 1;

        if residual < tol {
            break;
        }
    }

    (v, eigenvalue, iter, residual)
}

fn to_result(labels: Vec<String>, v: Vec<f64>, eigenvalue: f64, iterations: usize, convergence: f64) -> SpectralResult {
    let centrality = labels.iter().cloned().zip(v).collect();
    SpectralResult { labels, centrality, eigenvalue, iterations, convergence }
}

fn sorted_place_ids(model: &Model) -> Vec<String> {
    let mut ids: Vec<String> = model.places.iter().filter(|p| p.is_token()).map(|p| p.id.clone()).collect();
    ids.sort();
    ids
}

fn sorted_transition_ids(model: &Model) -> Vec<String> {
    let mut ids: Vec<String> = model.transitions.iter().map(|t| t.id.clone()).collect();
    ids.sort();
    ids
}

/// Eigenvector centrality of the model's bipartite (place ↔ transition)
/// adjacency matrix, ignoring arc direction and weighting by arc weight.
/// Inhibitor arcs are excluded (they gate without connecting flow-wise).
/// Go's `EigenvectorCentrality`.
pub fn eigenvector_centrality(model: &Model, max_iter: usize, tol: f64) -> SpectralResult {
    let places = sorted_place_ids(model);
    let transitions = sorted_transition_ids(model);
    let n = places.len() + transitions.len();

    let place_idx: HashMap<&str, usize> = places.iter().enumerate().map(|(i, p)| (p.as_str(), i)).collect();
    let trans_idx: HashMap<&str, usize> =
        transitions.iter().enumerate().map(|(i, t)| (t.as_str(), places.len() + i)).collect();

    let mut adj = vec![vec![0.0; n]; n];
    for arc in &model.arcs {
        if arc.typ == ArcType::Inhibitor {
            continue;
        }
        let w = arc.effective_weight() as f64;
        if let (Some(&pi), Some(&ti)) = (place_idx.get(arc.from.as_str()), trans_idx.get(arc.to.as_str())) {
            adj[pi][ti] += w;
            adj[ti][pi] += w;
        }
        if let (Some(&ti), Some(&pi)) = (trans_idx.get(arc.from.as_str()), place_idx.get(arc.to.as_str())) {
            adj[ti][pi] += w;
            adj[pi][ti] += w;
        }
    }

    let mut labels = places;
    labels.extend(transitions);
    let (v, eigenvalue, iterations, convergence) = power_iteration(&adj, n, max_iter, tol);
    to_result(labels, v, eigenvalue, iterations, convergence)
}

/// Eigenvector centrality of the entity projection `M = B * B^T` of a
/// bipartite entity-constraint graph, where `B` is the biadjacency matrix
/// between places matching `entity_prefix` (empty selects all) and
/// transitions accepted by `constraint_filter`.
///
/// Go's `ProjectedCentrality` filters constraints by `Transition.Role`, a
/// field of go-pflow's Shape A `petri.Transition`. This crate builds on
/// `pflow-metamodel`'s Shape B `Model` (ROADMAP.md ground rule), whose
/// `Transition` has no `role` — roles and access rules are Phase 3 scope,
/// declared at the model level (`AccessRule`) rather than per-transition.
/// A closure predicate over the transition id is the honest equivalent
/// here: a caller with access rules in hand filters by them; one without
/// passes `|_| true`.
pub fn projected_centrality(
    model: &Model,
    entity_prefix: &str,
    constraint_filter: impl Fn(&str) -> bool,
    max_iter: usize,
    tol: f64,
) -> SpectralResult {
    let entities: Vec<String> = sorted_place_ids(model)
        .into_iter()
        .filter(|p| entity_prefix.is_empty() || p.starts_with(entity_prefix))
        .collect();
    let constraints: Vec<String> =
        sorted_transition_ids(model).into_iter().filter(|t| constraint_filter(t)).collect();

    let biadjacency = build_biadjacency(model, &entities, &constraints);
    projected_from_biadjacency(entities, &biadjacency, max_iter, tol)
}

fn build_biadjacency(model: &Model, entities: &[String], constraints: &[String]) -> Vec<Vec<f64>> {
    let entity_idx: HashMap<&str, usize> = entities.iter().enumerate().map(|(i, e)| (e.as_str(), i)).collect();
    let constraint_idx: HashMap<&str, usize> =
        constraints.iter().enumerate().map(|(i, c)| (c.as_str(), i)).collect();

    let mut b = vec![vec![0.0; constraints.len()]; entities.len()];
    for arc in &model.arcs {
        if arc.typ == ArcType::Inhibitor {
            continue;
        }
        let w = arc.effective_weight() as f64;
        if let (Some(&ei), Some(&ci)) = (entity_idx.get(arc.from.as_str()), constraint_idx.get(arc.to.as_str())) {
            b[ei][ci] += w;
        }
        if let (Some(&ci), Some(&ei)) = (constraint_idx.get(arc.from.as_str()), entity_idx.get(arc.to.as_str())) {
            b[ei][ci] += w;
        }
    }
    b
}

fn projected_from_biadjacency(
    entities: Vec<String>,
    b: &[Vec<f64>],
    max_iter: usize,
    tol: f64,
) -> SpectralResult {
    let ne = entities.len();
    let nc = if b.is_empty() { 0 } else { b[0].len() };
    let mut m = vec![vec![0.0; ne]; ne];
    for i in 0..ne {
        for j in 0..ne {
            m[i][j] = b[i].iter().take(nc).zip(b[j].iter().take(nc)).map(|(x, y)| x * y).sum();
        }
    }
    let (v, eigenvalue, iterations, convergence) = power_iteration(&m, ne, max_iter, tol);
    to_result(entities, v, eigenvalue, iterations, convergence)
}

/// Eigenvector centrality from an explicit entity-constraint bipartite
/// graph: `constraints[j]` lists the indices into `entities` it connects.
/// `M[i][i]` is entity `i`'s degree; `M[i][j]` counts constraints shared by
/// `i` and `j`. Go's `EntityConstraintCentrality`.
pub fn entity_constraint_centrality(
    entities: &[String],
    constraints: &[Vec<usize>],
    max_iter: usize,
    tol: f64,
) -> SpectralResult {
    let ne = entities.len();
    let mut b = vec![vec![0.0; constraints.len()]; ne];
    for (j, constraint) in constraints.iter().enumerate() {
        for &i in constraint {
            if i < ne {
                b[i][j] = 1.0;
            }
        }
    }
    projected_from_biadjacency(entities.to_vec(), &b, max_iter, tol)
}

/// Eigenvector centrality of an arbitrary square matrix. Go's
/// `MatrixCentrality`.
pub fn matrix_centrality(labels: &[String], m: &[Vec<f64>], max_iter: usize, tol: f64) -> SpectralResult {
    let n = labels.len();
    let (v, eigenvalue, iterations, convergence) = power_iteration(m, n, max_iter, tol);
    to_result(labels.to_vec(), v, eigenvalue, iterations, convergence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_metamodel::schema::{Arc, Place, Transition};

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
    fn symmetric_cycle_gives_equal_centrality() {
        let model = cycle_model();
        let result = eigenvector_centrality(&model, 200, 1e-10);
        assert_eq!(result.labels.len(), 4);
        let values: Vec<f64> = result.labels.iter().map(|l| result.centrality[l]).collect();
        for v in &values {
            assert!((v - values[0]).abs() < 1e-6, "a symmetric 4-cycle should have uniform centrality");
        }
        assert!(result.eigenvalue > 0.0);
    }

    #[test]
    fn matrix_centrality_on_identity_like_matrix() {
        let labels = vec!["a".to_string(), "b".to_string()];
        let m = vec![vec![2.0, 0.0], vec![0.0, 1.0]];
        let result = matrix_centrality(&labels, &m, 100, 1e-12);
        // Dominant eigenvalue of diag(2,1) is 2, eigenvector along "a".
        assert!((result.eigenvalue - 2.0).abs() < 1e-6);
        assert!(result.centrality["a"].abs() > result.centrality["b"].abs());
    }

    #[test]
    fn entity_constraint_centrality_counts_shared_constraints() {
        let entities = vec!["e0".to_string(), "e1".to_string(), "e2".to_string()];
        // Constraint 0 connects e0,e1; constraint 1 connects e1,e2.
        let constraints = vec![vec![0, 1], vec![1, 2]];
        let result = entity_constraint_centrality(&entities, &constraints, 200, 1e-10);
        // e1 participates in both constraints, so it should have the
        // highest centrality.
        let c = |l: &str| result.centrality[l].abs();
        assert!(c("e1") >= c("e0"));
        assert!(c("e1") >= c("e2"));
    }
}
