//! Core tropical semiring (max-plus algebra) types and operations.
//!
//! In the tropical semiring (R union {-inf}, max, +):
//! - Addition:       a + b = max(a, b)
//! - Multiplication: a * b = a + b (standard addition)
//! - Zero:           -inf (identity for max)
//! - One:            0.0 (identity for +)

/// Tropical zero: identity for tropical addition (max).
pub const NEG_INF: f64 = f64::NEG_INFINITY;

/// Tropical addition: max(a, b).
#[inline]
pub fn tropical_add(a: f64, b: f64) -> f64 {
    a.max(b)
}

/// Tropical multiplication: a + b (with -inf propagation).
#[inline]
pub fn tropical_mul(a: f64, b: f64) -> f64 {
    if a == NEG_INF || b == NEG_INF {
        NEG_INF
    } else {
        a + b
    }
}

/// Row-major tropical matrix. NEG_INF represents structural zeros.
#[derive(Debug, Clone)]
pub struct Matrix {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

impl Matrix {
    /// Creates a new matrix initialized to NEG_INF (tropical zero).
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            data: vec![NEG_INF; rows * cols],
        }
    }

    /// Creates a matrix from existing data (row-major).
    pub fn from_data(rows: usize, cols: usize, data: Vec<f64>) -> Self {
        assert_eq!(data.len(), rows * cols);
        Self { rows, cols, data }
    }

    #[inline]
    pub fn at(&self, i: usize, j: usize) -> f64 {
        self.data[i * self.cols + j]
    }

    #[inline]
    pub fn set(&mut self, i: usize, j: usize, v: f64) {
        self.data[i * self.cols + j] = v;
    }

    /// Returns true if this is a square matrix.
    pub fn is_square(&self) -> bool {
        self.rows == self.cols
    }
}

/// Tropical matrix multiplication: C[i][j] = max_k(A[i][k] + B[k][j]).
pub fn mat_mul(a: &Matrix, b: &Matrix) -> Matrix {
    assert_eq!(a.cols, b.rows);
    let mut c = Matrix::new(a.rows, b.cols);
    for i in 0..a.rows {
        for j in 0..b.cols {
            let mut val = NEG_INF;
            for k in 0..a.cols {
                val = tropical_add(val, tropical_mul(a.at(i, k), b.at(k, j)));
            }
            c.set(i, j, val);
        }
    }
    c
}

/// Tropical matrix power: A^k via repeated tropical multiplication.
pub fn mat_pow(a: &Matrix, k: usize) -> Matrix {
    assert!(a.is_square());
    if k == 0 {
        // Tropical identity: 0 on diagonal, -inf elsewhere
        let n = a.rows;
        let mut id = Matrix::new(n, n);
        for i in 0..n {
            id.set(i, i, 0.0);
        }
        return id;
    }
    let mut result = a.clone();
    for _ in 1..k {
        result = mat_mul(&result, a);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scalar_ops() {
        assert_eq!(tropical_add(3.0, 5.0), 5.0);
        assert_eq!(tropical_add(NEG_INF, 5.0), 5.0);
        assert_eq!(tropical_add(NEG_INF, NEG_INF), NEG_INF);

        assert_eq!(tropical_mul(3.0, 5.0), 8.0);
        assert_eq!(tropical_mul(NEG_INF, 5.0), NEG_INF);
        assert_eq!(tropical_mul(NEG_INF, NEG_INF), NEG_INF);
        assert_eq!(tropical_mul(0.0, 7.0), 7.0); // 0 is multiplicative identity
    }

    #[test]
    fn test_mat_mul_3x3() {
        // Hand-computed tropical product:
        // A = [[0, 1, -inf], [-inf, 0, 2], [3, -inf, 0]]
        // B = [[0, -inf, 1], [2, 0, -inf], [-inf, 1, 0]]
        //
        // C[0][0] = max(0+0, 1+2, -inf) = 3
        // C[0][1] = max(0+-inf, 1+0, -inf) = 1
        // C[0][2] = max(0+1, 1+-inf, -inf) = 1
        // C[1][0] = max(-inf, 0+2, 2+-inf) = 2
        // C[1][1] = max(-inf, 0+0, 2+1) = 3
        // C[1][2] = max(-inf, 0+-inf, 2+0) = 2
        // C[2][0] = max(3+0, -inf, 0+-inf) = 3
        // C[2][1] = max(3+-inf, -inf, 0+1) = 1
        // C[2][2] = max(3+1, -inf, 0+0) = 4
        let a = Matrix::from_data(3, 3, vec![
            0.0, 1.0, NEG_INF,
            NEG_INF, 0.0, 2.0,
            3.0, NEG_INF, 0.0,
        ]);
        let b = Matrix::from_data(3, 3, vec![
            0.0, NEG_INF, 1.0,
            2.0, 0.0, NEG_INF,
            NEG_INF, 1.0, 0.0,
        ]);
        let c = mat_mul(&a, &b);
        assert_eq!(c.at(0, 0), 3.0);
        assert_eq!(c.at(0, 1), 1.0);
        assert_eq!(c.at(0, 2), 1.0);
        assert_eq!(c.at(1, 0), 2.0);
        assert_eq!(c.at(1, 1), 3.0);
        assert_eq!(c.at(1, 2), 2.0);
        assert_eq!(c.at(2, 0), 3.0);
        assert_eq!(c.at(2, 1), 1.0);
        assert_eq!(c.at(2, 2), 4.0);
    }
}
