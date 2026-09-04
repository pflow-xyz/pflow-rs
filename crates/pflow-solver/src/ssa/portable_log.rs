//! Portable natural logarithm for the byte-exact SSA.
//!
//! This is the FreeBSD/fdlibm `e_log.c` algorithm in the form Go's pure-Go
//! `math.log` uses: `frexp` reduction, `s = f/(2+f)`, the seven-coefficient
//! rational polynomial and one final combination, with no special-case
//! branches. It is ported from the SSA spec text, not from fdlibm's C file
//! (they differ by an ulp on some inputs).
//!
//! `f64::ln` is the platform libm and differs from this function on ~7 % of
//! uniform inputs (`ln(3.0)` is the canonical example), so it must never be
//! used on the SSA path. Every line below is one binary64 operation per
//! operator in the written order; Rust does not contract to FMA by default.

const LN2_HI: f64 = f64::from_bits(0x3FE6_2E42_FEE0_0000); // 6.93147180369123816490e-01
const LN2_LO: f64 = f64::from_bits(0x3DEA_39EF_3579_3C76); // 1.90821492927058770002e-10
const L1: f64 = f64::from_bits(0x3FE5_5555_5555_5593); // 6.666666666666735130e-01
const L2: f64 = f64::from_bits(0x3FD9_9999_9997_FA04); // 3.999999999940941908e-01
const L3: f64 = f64::from_bits(0x3FD2_4924_9422_9359); // 2.857142874366239149e-01
const L4: f64 = f64::from_bits(0x3FCC_71C5_1D8E_78AF); // 2.222219843214978396e-01
const L5: f64 = f64::from_bits(0x3FC7_4664_96CB_03DE); // 1.818357216161805012e-01
const L6: f64 = f64::from_bits(0x3FC3_9A09_D078_C69F); // 1.531383769920937332e-01
const L7: f64 = f64::from_bits(0x3FC2_F112_DF3E_5244); // 1.479819860511658591e-01
const SQRT2_OVER_2: f64 = f64::from_bits(0x3FE6_A09E_667F_3BCD); // 0.7071067811865476

const EXP_MASK: u64 = 0x7FF << 52;

/// `frexp` on the bit pattern of a positive, finite, non-zero `x`:
/// returns `(frac, exp)` with `x = frac * 2^exp` and `frac` in `[0.5, 1)`.
pub fn frexp(x: f64) -> (f64, i32) {
    let mut x = x;
    let mut b = x.to_bits();
    let mut e = ((b >> 52) & 0x7FF) as i32;
    if e == 0 {
        // Subnormal: normalise first. Multiplying by 2^52 is exact.
        x *= f64::from_bits(0x4330_0000_0000_0000);
        b = x.to_bits();
        e = ((b >> 52) & 0x7FF) as i32 - 52;
    }
    let exp = e - 1022;
    let frac = f64::from_bits((b & !EXP_MASK) | (1022u64 << 52));
    (frac, exp)
}

/// Portable natural logarithm (spec §2.4).
pub fn plog(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }

    let (mut f1, mut ki) = frexp(x);
    if f1 < SQRT2_OVER_2 {
        f1 *= 2.0; // exact
        ki -= 1;
    }
    let f = f1 - 1.0;
    let k = ki as f64;

    let s = f / (2.0 + f);
    let s2 = s * s;
    let s4 = s2 * s2;
    let t1 = s2 * (L1 + (s4 * (L3 + (s4 * (L5 + (s4 * L7))))));
    let t2 = s4 * (L2 + (s4 * (L4 + (s4 * L6))));
    let r = t1 + t2;
    let hfsq = (0.5 * f) * f;
    (k * LN2_HI) - ((hfsq - ((s * (hfsq + r)) + (k * LN2_LO))) - f)
}

#[cfg(test)]
// The decimal literals below are the spec's own constants and test vectors;
// they are deliberately written out in full and deliberately near LN_2 etc.
#[allow(clippy::excessive_precision, clippy::approx_constant)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_decimal_literals() {
        assert_eq!(LN2_HI, 6.93147180369123816490e-01);
        assert_eq!(LN2_LO, 1.90821492927058770002e-10);
        assert_eq!(L1, 6.666666666666735130e-01);
        assert_eq!(L2, 3.999999999940941908e-01);
        assert_eq!(L3, 2.857142874366239149e-01);
        assert_eq!(L4, 2.222219843214978396e-01);
        assert_eq!(L5, 1.818357216161805012e-01);
        assert_eq!(L6, 1.531383769920937332e-01);
        assert_eq!(L7, 1.479819860511658591e-01);
        assert_eq!(SQRT2_OVER_2, 0.7071067811865476);
    }

    #[test]
    fn frexp_basic() {
        assert_eq!(frexp(1.0), (0.5, 1));
        assert_eq!(frexp(0.5), (0.5, 0));
        assert_eq!(frexp(3.0), (0.75, 2));
        assert_eq!(frexp(1e-300).0 * 2f64.powi(frexp(1e-300).1), 1e-300);
        // Subnormal path.
        let (fr, e) = frexp(5e-324);
        assert!((0.5..1.0).contains(&fr));
        // powi(-1074) alone underflows; scale in two exact steps.
        assert_eq!(fr * 2f64.powi(e + 60) * 2f64.powi(-60), 5e-324);
    }

    #[test]
    fn plog_thirteen_vectors_bit_exact() {
        // Spec §2.5: (x, plog(x) decimal, bits).
        let vectors: [(f64, f64, u64); 13] = [
            (0.5, -0.6931471805599453, 0xBFE6_2E42_FEFA_39EF),
            (2.0, 0.6931471805599453, 0x3FE6_2E42_FEFA_39EF),
            (0.1, -2.3025850929940455, 0xC002_6BB1_BBB5_5515),
            (1e-300, -690.7755278982137, 0xC085_9634_47F8_7FB5),
            (0.999999, -1.000000500029089e-06, 0xBEB0_C6F8_2D74_D230),
            // glibc gives 1.0986122886681098 here: proves the port is in use.
            (3.0, 1.0986122886681096, 0x3FF1_93EA_7AAD_030A),
            (10.0, 2.302585092994046, 0x4002_6BB1_BBB5_5516),
            (1.0, 0.0, 0x0000_0000_0000_0000),
            // = SQRT2_OVER_2, takes the no-doubling branch.
            (
                0.7071067811865476,
                -0.3465735902799726,
                0xBFD6_2E42_FEFA_39EE,
            ),
            // one ulp below, takes the doubling branch.
            (
                0.7071067811865475,
                -0.34657359027997275,
                0xBFD6_2E42_FEFA_39F1,
            ),
            (
                1.0000000000000002,
                2.2204460492503128e-16,
                0x3CAF_FFFF_FFFF_FFFF,
            ),
            (
                0.9999999999999999,
                -1.1102230246251565e-16,
                0xBCA0_0000_0000_0000,
            ),
            (1e-09, -20.72326583694641, 0xC034_B927_F32B_FFB8),
        ];
        for (x, want, bits) in vectors {
            let got = plog(x);
            assert_eq!(got.to_bits(), bits, "plog({x}) = {got:?}, want {want:?}");
            assert_eq!(got, want, "plog({x}) decimal");
        }
    }

    #[test]
    fn plog_special_values() {
        assert!(plog(f64::NAN).is_nan());
        assert_eq!(plog(f64::INFINITY), f64::INFINITY);
        assert!(plog(-1.0).is_nan());
        assert_eq!(plog(0.0), f64::NEG_INFINITY);
        assert_eq!(plog(-0.0), f64::NEG_INFINITY);
    }

    #[test]
    fn plog_smallest_ssa_input_is_finite() {
        // u = 2^-53 is the smallest value 1 - uniform() can take.
        let v = plog(f64::from_bits(0x3CA0_0000_0000_0000));
        assert!(v.is_finite());
        assert!((v - -36.7368005696771).abs() < 1e-12);
    }
}
