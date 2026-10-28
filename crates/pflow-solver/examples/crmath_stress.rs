//! `cargo run --release -p pflow-solver --example crmath_stress [n]`: the
//! double-double path against the 320-bit path on n random inputs (default
//! 5,000,000), half of them shaped like the ODE step controller's.
use pflow_solver::crmath::{pow, pow_slow};

fn main() {
    let n: u64 = std::env::args()
        .nth(1)
        .map(|a| a.parse().unwrap())
        .unwrap_or(5_000_000);
    let mut s = 0x2545_F491_4F6C_DD1Du64;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let (mut checked, mut bad) = (0u64, 0u64);
    for i in 0..n {
        let (x, y) = if i % 2 == 0 {
            let err = 10f64.powf((next() >> 11) as f64 / (1u64 << 53) as f64 * 22.0 - 14.0);
            (1.0 / err, 1.0 / (2 + next() % 8) as f64)
        } else {
            (
                f64::from_bits(1 + next() % 0x7FEF_FFFF_FFFF_FFFF),
                (next() >> 11) as f64 / (1u64 << 53) as f64 * 16.0 - 8.0,
            )
        };
        let a = pow(x, y);
        if !a.is_finite() || a == 0.0 {
            continue;
        }
        checked += 1;
        let b = pow_slow(x, y);
        if a.to_bits() != b.to_bits() {
            bad += 1;
            eprintln!("pow({x:e}, {y:e}): fast {a:e}, slow {b:e}");
        }
    }
    println!("{checked} checked, {bad} disagree");
    assert_eq!(bad, 0);
}
