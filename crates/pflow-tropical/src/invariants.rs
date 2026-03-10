//! Structural invariants of Petri nets.
//!
//! P-invariants: vectors y such that y · C = 0 (place conservation laws).
//! T-invariants: vectors x such that C · x = 0 (reproducible firing sequences).
//!
//! Both are computed from the integer null space of the incidence matrix,
//! making them purely topological — they depend on which arcs exist and
//! their weights, but the *support* (which places/transitions participate)
//! is determined by connectivity alone.

use pflow_zk::IncidenceMatrix;

/// Dense incidence matrix C where C[t][p] = output_weight - input_weight.
pub fn dense_incidence(im: &IncidenceMatrix) -> Vec<Vec<i64>> {
    let mut c = vec![vec![0i64; im.num_places]; im.num_transitions];
    for t in 0..im.num_transitions {
        for &(p, w) in &im.inputs[t] {
            c[t][p] -= w;
        }
        for &(p, w) in &im.outputs[t] {
            c[t][p] += w;
        }
    }
    c
}

/// Transpose a dense matrix.
fn transpose(m: &[Vec<i64>]) -> Vec<Vec<i64>> {
    if m.is_empty() {
        return vec![];
    }
    let rows = m.len();
    let cols = m[0].len();
    let mut t = vec![vec![0i64; rows]; cols];
    for i in 0..rows {
        for j in 0..cols {
            t[j][i] = m[i][j];
        }
    }
    t
}

/// Compute the integer null space of a matrix using Gaussian elimination.
///
/// Returns a list of basis vectors x such that M · x = 0.
/// Works over rationals (using i64 with explicit GCD reduction) to find
/// integer solutions.
fn integer_null_space(matrix: &[Vec<i64>]) -> Vec<Vec<i64>> {
    if matrix.is_empty() {
        return vec![];
    }
    let rows = matrix.len();
    let cols = matrix[0].len();

    // Work with rational arithmetic using (numerator, denominator) pairs
    // to avoid floating point. We'll use i128 to avoid overflow.
    let mut m: Vec<Vec<i128>> = matrix
        .iter()
        .map(|row| row.iter().map(|&v| v as i128).collect())
        .collect();

    // Track pivot columns
    let mut pivot_cols: Vec<usize> = Vec::new();
    let mut pivot_row = 0;

    for col in 0..cols {
        // Find pivot in this column
        let mut found = None;
        for row in pivot_row..rows {
            if m[row][col] != 0 {
                found = Some(row);
                break;
            }
        }
        let Some(pr) = found else {
            continue;
        };

        // Swap rows
        m.swap(pivot_row, pr);
        pivot_cols.push(col);

        // Eliminate below (and above for reduced form)
        let pivot_val = m[pivot_row][col];
        for row in 0..rows {
            if row == pivot_row || m[row][col] == 0 {
                continue;
            }
            let factor = m[row][col];
            for c in 0..cols {
                m[row][c] = m[row][c] * pivot_val - factor * m[pivot_row][c];
            }
            // Reduce row by GCD to prevent overflow
            let g = row_gcd(&m[row]);
            if g > 1 {
                for c in 0..cols {
                    m[row][c] /= g;
                }
            }
        }
        pivot_row += 1;
    }

    // Free variables are columns not in pivot_cols
    let free_cols: Vec<usize> = (0..cols).filter(|c| !pivot_cols.contains(c)).collect();

    // For each free variable, construct a null space vector
    let mut basis = Vec::new();
    for &free in &free_cols {
        let mut x = vec![0i128; cols];
        x[free] = 1;

        // Back-substitute in reverse order (last pivot first) so that
        // scaling free variables doesn't invalidate already-computed pivots.
        for (i, &pc) in pivot_cols.iter().enumerate().rev() {
            let pivot_val = m[i][pc];
            if pivot_val == 0 {
                continue;
            }
            // pivot_val * x[pc] + sum = 0 => x[pc] = -sum / pivot_val
            // Scale free variables by |pivot_val| to keep integer arithmetic
            x[free] *= pivot_val.abs();
            for &other_free in &free_cols {
                if other_free != free {
                    x[other_free] *= pivot_val.abs();
                }
            }
            // Compute sum of non-pivot contributions with scaled values
            let mut sum = 0i128;
            for c in 0..cols {
                if c != pc {
                    sum += m[i][c] * x[c];
                }
            }
            x[pc] = -sum / pivot_val;
        }

        // Reduce by GCD
        let g = vec_gcd(&x);
        if g > 1 {
            for v in &mut x {
                *v /= g;
            }
        }

        basis.push(x.into_iter().map(|v| v as i64).collect());
    }

    basis
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn row_gcd(row: &[i128]) -> i128 {
    row.iter().fold(0, |acc, &v| gcd(acc, v)).max(1)
}

fn vec_gcd(v: &[i128]) -> i128 {
    v.iter().fold(0, |acc, &val| gcd(acc, val)).max(1)
}

/// Compute P-invariants: vectors y such that C · y = 0.
///
/// With pflow's convention (C is transitions x places), this finds
/// place vectors whose weighted token sum is constant under all firings.
pub fn p_invariants(im: &IncidenceMatrix) -> Vec<Vec<i64>> {
    let c = dense_incidence(im);
    integer_null_space(&c)
}

/// Compute T-invariants: vectors x such that C^T · x = 0.
///
/// Each invariant represents a firing sequence that returns the net
/// to its original marking (a reproducible cycle).
pub fn t_invariants(im: &IncidenceMatrix) -> Vec<Vec<i64>> {
    let c = dense_incidence(im);
    let ct = transpose(&c);
    integer_null_space(&ct)
}

/// The support of an invariant: indices of non-zero entries.
pub fn support(invariant: &[i64]) -> Vec<usize> {
    invariant
        .iter()
        .enumerate()
        .filter(|(_, &v)| v != 0)
        .map(|(i, _)| i)
        .collect()
}

/// The sign pattern of a vector: -1, 0, or +1 for each entry.
pub fn sign_pattern(v: &[i64]) -> Vec<i8> {
    v.iter().map(|&x| x.signum() as i8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_core::PetriNet;

    #[test]
    fn test_simple_loop_p_invariant() {
        // P0 <-> P1 loop: tokens are conserved (P0 + P1 = const)
        let net = PetriNet::build()
            .place("P0", 1.0)
            .place("P1", 0.0)
            .transition("T0")
            .transition("T1")
            .arc("P0", "T0", 1.0)
            .arc("T0", "P1", 1.0)
            .arc("P1", "T1", 1.0)
            .arc("T1", "P0", 1.0)
            .done();

        let im = IncidenceMatrix::from_petri_net(&net);
        let pinv = p_invariants(&im);

        // Should have exactly 1 P-invariant: [1, 1] (P0 + P1 = const)
        assert_eq!(pinv.len(), 1, "expected 1 P-invariant, got {}", pinv.len());
        // Both places participate (support = {0, 1})
        let sup = support(&pinv[0]);
        assert_eq!(sup.len(), 2, "invariant should span both places");
    }

    #[test]
    fn test_simple_loop_t_invariant() {
        let net = PetriNet::build()
            .place("P0", 1.0)
            .place("P1", 0.0)
            .transition("T0")
            .transition("T1")
            .arc("P0", "T0", 1.0)
            .arc("T0", "P1", 1.0)
            .arc("P1", "T1", 1.0)
            .arc("T1", "P0", 1.0)
            .done();

        let im = IncidenceMatrix::from_petri_net(&net);
        let tinv = t_invariants(&im);

        // Should have 1 T-invariant: [1, 1] (fire T0 then T1 returns to start)
        assert_eq!(tinv.len(), 1, "expected 1 T-invariant, got {}", tinv.len());
        let sup = support(&tinv[0]);
        assert_eq!(sup.len(), 2, "both transitions participate in the cycle");
    }

    #[test]
    fn test_sir_p_invariant() {
        // SIR: infect (S+I -> 2I), recover (I -> R)
        // Conservation: S + I + R = const
        let net = PetriNet::build().sir(10.0, 1.0, 0.0).done();
        let im = IncidenceMatrix::from_petri_net(&net);
        let pinv = p_invariants(&im);

        assert!(!pinv.is_empty(), "SIR should have at least 1 P-invariant");
        // The invariant should span all 3 places (total population conserved)
        let sup = support(&pinv[0]);
        assert_eq!(sup.len(), 3, "all places in SIR are conserved together");
    }

    #[test]
    fn test_dense_incidence() {
        let net = PetriNet::build()
            .place("P0", 1.0)
            .place("P1", 0.0)
            .transition("T0")
            .transition("T1")
            .arc("P0", "T0", 1.0)
            .arc("T0", "P1", 1.0)
            .arc("P1", "T1", 1.0)
            .arc("T1", "P0", 1.0)
            .done();

        let im = IncidenceMatrix::from_petri_net(&net);
        let c = dense_incidence(&im);

        // T0: consumes P0, produces P1 -> [-1, 1]
        // T1: consumes P1, produces P0 -> [1, -1]
        assert_eq!(c[0], vec![-1, 1]);
        assert_eq!(c[1], vec![1, -1]);
    }
}
