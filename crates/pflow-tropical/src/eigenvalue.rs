//! Tropical eigenvalue (max circuit mean) via Karp's algorithm.
//!
//! The tropical eigenvalue lambda of a square matrix A is the maximum cycle mean:
//!   lambda = max over all cycles c of { (sum of weights on c) / (length of c) }
//!
//! This gives the throughput of a timed Petri net directly from topology.

use crate::semiring::{Matrix, NEG_INF, tropical_mul};

/// Computes the tropical eigenvalue (max circuit mean) and critical circuit.
///
/// Uses Karp's algorithm: compute A^k for k = 1..n, track max-weight paths,
/// then lambda = max_j min_k (D_n[j] - D_k[j]) / (n - k).
///
/// Returns `(lambda, circuit)` where circuit is the node indices on the critical cycle.
/// Returns an error if the matrix is not square or has no cycles.
pub fn eigenvalue(a: &Matrix) -> Result<(f64, Vec<usize>), String> {
    if !a.is_square() {
        return Err("matrix must be square".into());
    }
    let n = a.rows;
    if n == 0 {
        return Err("empty matrix".into());
    }

    // D[k][j] = max weight of any path of exactly k edges ending at node j.
    // We compute for k = 0..=n.
    // D[0][j] = 0 for all j (tropical identity: path of length 0).
    let mut d: Vec<Vec<f64>> = vec![vec![NEG_INF; n]; n + 1];
    // predecessor[k][j] = node before j on the best k-edge path
    let mut pred: Vec<Vec<Option<usize>>> = vec![vec![None; n]; n + 1];

    for j in 0..n {
        d[0][j] = 0.0;
    }

    // D[k][j] = max_i { D[k-1][i] + A[i][j] }
    for k in 1..=n {
        for j in 0..n {
            for i in 0..n {
                let candidate = tropical_mul(d[k - 1][i], a.at(i, j));
                if candidate > d[k][j] {
                    d[k][j] = candidate;
                    pred[k][j] = Some(i);
                }
            }
        }
    }

    // Karp's formula: lambda = max_j { min_k { (D[n][j] - D[k][j]) / (n - k) } }
    let mut lambda = NEG_INF;
    let mut best_j = 0;
    let mut best_k = 0;

    for j in 0..n {
        if d[n][j] == NEG_INF {
            continue;
        }
        let mut min_val = f64::INFINITY;
        let mut min_k = 0;
        for k in 0..n {
            if d[k][j] == NEG_INF {
                // (finite - (-inf)) / positive = +inf, so this k won't be the min
                continue;
            }
            let val = (d[n][j] - d[k][j]) / (n - k) as f64;
            if val < min_val {
                min_val = val;
                min_k = k;
            }
        }
        if min_val > lambda {
            lambda = min_val;
            best_j = j;
            best_k = min_k;
        }
    }

    if lambda == NEG_INF {
        return Err("no cycles found (graph is not strongly connected)".into());
    }

    // Trace back the critical circuit from the n-edge path ending at best_j.
    // The cycle has length (n - best_k) but we trace the full n-edge path
    // and find the cycle within it.
    let mut path = Vec::with_capacity(n + 1);
    let mut node = best_j;
    path.push(node);
    for step in (1..=n).rev() {
        if let Some(prev) = pred[step][node] {
            path.push(prev);
            node = prev;
        } else {
            break;
        }
    }
    path.reverse();

    // Find the cycle: look for repeated node in the path
    let cycle_len = n - best_k;
    let circuit: Vec<usize> = if path.len() > cycle_len {
        path[path.len() - cycle_len..].to_vec()
    } else {
        path
    };

    Ok((lambda, circuit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_3_node_cycle() {
        // 3-node cycle: 0 -> 1 (weight 2), 1 -> 2 (weight 1), 2 -> 0 (weight 3)
        // Cycle mean = (2 + 1 + 3) / 3 = 2.0
        let mut a = Matrix::new(3, 3);
        a.set(0, 1, 2.0);
        a.set(1, 2, 1.0);
        a.set(2, 0, 3.0);

        let (lambda, circuit) = eigenvalue(&a).unwrap();
        assert!((lambda - 2.0).abs() < 1e-10, "expected 2.0, got {}", lambda);
        assert_eq!(circuit.len(), 3);
    }

    #[test]
    fn test_petri_net_loop() {
        // The validation net: T0 (P0->P1), T1 (P1->P0)
        // As a tropical matrix (transitions as nodes, arc weights = 1):
        // T0 -> T1 (weight 1), T1 -> T0 (weight 1)
        // Cycle mean = (1+1)/2 = 1.0
        let mut a = Matrix::new(2, 2);
        a.set(0, 1, 1.0);
        a.set(1, 0, 1.0);

        let (lambda, _circuit) = eigenvalue(&a).unwrap();
        assert!((lambda - 1.0).abs() < 1e-10, "expected 1.0, got {}", lambda);
    }

    #[test]
    fn test_no_cycles() {
        // DAG: 0 -> 1 -> 2, no back edges
        let mut a = Matrix::new(3, 3);
        a.set(0, 1, 1.0);
        a.set(1, 2, 1.0);

        let result = eigenvalue(&a);
        assert!(result.is_err());
    }
}
