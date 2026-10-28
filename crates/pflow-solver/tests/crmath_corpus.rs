//! `crmath::pow` against the shared corpus: every case bit for bit.
//!
//! `tests/fixtures/crmath/pow.txt` is `scripts/crmath/corpus.py`'s output:
//! `x y x^y` as hex bit patterns, the result correctly rounded from Python's
//! `decimal` at 120 digits. go-pflow (`crmath/testdata/pow.txt`) and pflow-xyz
//! (`parity/crmath/pow.txt`) carry byte-identical copies and run the same
//! check on their ports (Go, and JS under Node and in the browser), and this
//! test runs natively and on wasm32 (`make wasm-crmath` in pflow-native), so
//! one file holds all of them to the same bits.

use pflow_solver::crmath::{pow, pow_slow};

const CORPUS: &str = include_str!("fixtures/crmath/pow.txt");

fn cases() -> Vec<(f64, f64, f64)> {
    CORPUS
        .lines()
        .map(|l| {
            let w: Vec<f64> = l
                .split(' ')
                .map(|h| f64::from_bits(u64::from_str_radix(h, 16).unwrap()))
                .collect();
            (w[0], w[1], w[2])
        })
        .collect()
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

#[test]
fn pow_matches_the_corpus() {
    let cases = cases();
    assert!(cases.len() > 13_000, "corpus truncated: {}", cases.len());
    let mut bad = Vec::new();
    let mut libm_wrong = 0;
    for &(x, y, want) in &cases {
        let got = pow(x, y);
        if !same(got, want) {
            bad.push(format!(
                "pow({x:e} [{:016x}], {y:e} [{:016x}]) = {got:e}, want {want:e}",
                x.to_bits(),
                y.to_bits()
            ));
        }
        if !same(x.powf(y), want) {
            libm_wrong += 1;
        }
    }
    eprintln!(
        "host powf differs from the correctly rounded result on {libm_wrong} of {} cases",
        cases.len()
    );
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad[..bad.len().min(20)].join("\n")
    );
}

#[test]
fn the_slow_path_matches_the_corpus() {
    for (x, y, want) in cases() {
        if x > 0.0
            && x.is_finite()
            && x != 1.0
            && y.is_finite()
            && y != 0.0
            && want.is_finite()
            && want != 0.0
        {
            // pow_slow assumes y ln x within [-745.2, 709.79], which a finite, non-zero result implies.
            let got = pow_slow(x, y);
            assert!(
                same(got, want),
                "pow_slow({x:e}, {y:e}) = {got:e}, want {want:e}"
            );
        }
    }
}
