//! Portable PRNG for the byte-exact SSA: SplitMix64 seeding into xoshiro256**.
//!
//! Every operation is `u64` wrapping arithmetic, a logical shift or a rotate,
//! so the stream is identical on every platform and in every language that
//! ports the same text (Go, JS BigInt, Julia). No `rand` crate is involved.

/// SplitMix64: expand one 64-bit seed into the four xoshiro256** state words.
///
/// `s[0]` is the first SplitMix64 output.
pub fn splitmix64(seed: u64) -> [u64; 4] {
    let mut s = [0u64; 4];
    let mut x = seed;
    for word in s.iter_mut() {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        *word = z;
    }
    s
}

/// xoshiro256** generator seeded through [`splitmix64`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xoshiro256 {
    s: [u64; 4],
}

/// 2^-53, exactly. `(next() >> 11) as f64 * TWO_POW_M53` is exact for every
/// 53-bit integer, so `uniform()` is a plain bit rearrangement.
const TWO_POW_M53: f64 = 1.0 / 9_007_199_254_740_992.0;

impl Xoshiro256 {
    /// Seed the generator: `state = splitmix64(seed)`.
    pub fn new(seed: u64) -> Self {
        Self {
            s: splitmix64(seed),
        }
    }

    /// Construct from explicit state words (unit tests and cross-checks).
    pub fn from_state(s: [u64; 4]) -> Self {
        Self { s }
    }

    /// The current state words.
    pub fn state(&self) -> [u64; 4] {
        self.s
    }

    /// One raw 64-bit output. `result` is computed from the *old* `s[1]`.
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Uniform double in `[0, 1)` with granularity 2^-53: `(next() >> 11) * 2^-53`.
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * TWO_POW_M53
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_state_seed_42() {
        assert_eq!(
            splitmix64(42),
            [
                0xBDD7_3226_2FEB_6E95,
                0x28EF_E333_B266_F103,
                0x4752_6757_130F_9F52,
                0x581C_E1FF_0E4A_E394,
            ]
        );
    }

    #[test]
    fn splitmix_state_seed_0() {
        assert_eq!(
            splitmix64(0),
            [
                0xE220_A839_7B1D_CDAF,
                0x6E78_9E6A_A1B9_65F4,
                0x06C4_5D18_8009_454F,
                0xF88B_B8A8_724C_81EC,
            ]
        );
    }

    #[test]
    fn xoshiro_seed_42_first_five_raw_and_uniform() {
        // Raw outputs and the corresponding uniform() bit patterns, spec §1.5.
        let raw: [u64; 5] = [
            0x1578_0B2E_0C2E_C716,
            0x6104_D986_6D11_3A7E,
            0xAE17_5332_39E4_99A1,
            0xECB8_AD47_03B3_60A1,
            0xFDE6_DC7F_E2EC_5E64,
        ];
        let dec: [u64; 5] = [
            1_546_998_764_402_558_742,
            6_990_951_692_964_543_102,
            12_544_586_762_248_559_009,
            17_057_574_109_182_124_193,
            18_295_552_978_065_317_476,
        ];
        let uni_bits: [u64; 5] = [
            0x3FB5_780B_2E0C_2EC0,
            0x3FD8_4136_619B_444E,
            0x3FE5_C2EA_6647_3C93,
            0x3FED_9715_A8E0_766C,
            0x3FEF_BCDB_8FFC_5D8B,
        ];
        let uni_dec: [f64; 5] = [
            0.08386297105988216,
            0.3789802506626686,
            0.6800434110281394,
            0.9246929453253876,
            0.9918039142821028,
        ];
        let mut g = Xoshiro256::new(42);
        for i in 0..5 {
            let r = g.next_u64();
            assert_eq!(r, raw[i], "raw output {i}");
            assert_eq!(r, dec[i], "raw output {i} (decimal)");
        }
        let mut g = Xoshiro256::new(42);
        for i in 0..5 {
            let u = g.uniform();
            assert_eq!(u.to_bits(), uni_bits[i], "uniform bits {i}");
            assert_eq!(u, uni_dec[i], "uniform decimal {i}");
        }
    }

    #[test]
    fn xoshiro_single_output_vectors() {
        assert_eq!(Xoshiro256::new(0).next_u64(), 0x99EC_5F36_CB75_F2B4);
        assert_eq!(Xoshiro256::new(1).next_u64(), 0xB3F2_AF6D_0FC7_10C5);
        // 2^64-1 exercises wrapping in SplitMix64's first addition.
        assert_eq!(Xoshiro256::new(u64::MAX).next_u64(), 0x8F55_20D5_2A7E_AD08);
    }

    #[test]
    fn uniform_complement_is_exact_and_positive() {
        // u = 1 - x1 is exact for every x1 = k * 2^-53 and never zero.
        let mut g = Xoshiro256::new(42);
        for _ in 0..10_000 {
            let x1 = g.uniform();
            assert!((0.0..1.0).contains(&x1));
            let u = 1.0 - x1;
            assert!(u > 0.0 && u <= 1.0);
        }
    }
}
