//! Poseidon hash gadget for state commitments.

use ark_bn254::Fr;
use ark_crypto_primitives::sponge::{
    constraints::CryptographicSpongeVar,
    poseidon::{constraints::PoseidonSpongeVar, PoseidonConfig, PoseidonSponge},
    CryptographicSponge, FieldBasedCryptographicSponge,
};
use ark_ff::Field;
use ark_r1cs_std::fields::fp::FpVar;
use ark_relations::r1cs::{ConstraintSystemRef, SynthesisError};

/// Default Poseidon configuration for BN254.
pub fn poseidon_config() -> PoseidonConfig<Fr> {
    let full_rounds = 8;
    let partial_rounds = 57;
    let alpha = 5;
    let rate = 2;
    let capacity = 1;
    let state_size = rate + capacity;

    let num_constants = (full_rounds + partial_rounds) * state_size;
    let mut ark = Vec::with_capacity(num_constants);
    for i in 0..num_constants {
        ark.push(Fr::from((i + 1) as u64));
    }

    // Cauchy MDS matrix construction
    let mut mds = vec![vec![Fr::from(0u64); state_size]; state_size];
    for i in 0..state_size {
        for j in 0..state_size {
            let x = Fr::from((i + 1) as u64);
            let y = Fr::from((state_size + j + 1) as u64);
            mds[i][j] = (x + y).inverse().unwrap_or(Fr::from(1u64));
        }
    }

    let ark_matrix: Vec<Vec<Fr>> = ark.chunks(state_size).map(|c| c.to_vec()).collect();

    PoseidonConfig {
        full_rounds: full_rounds as usize,
        partial_rounds: partial_rounds as usize,
        alpha: alpha as u64,
        ark: ark_matrix,
        mds,
        rate,
        capacity,
    }
}

/// Hash a vector of field elements using Poseidon (native, outside circuit).
pub fn poseidon_hash_native(values: &[Fr]) -> Fr {
    let config = poseidon_config();
    let mut sponge = PoseidonSponge::new(&config);
    sponge.absorb(&values);
    let output: Vec<Fr> = sponge.squeeze_native_field_elements(1);
    output[0]
}

/// Hash a vector of FpVar elements using Poseidon (inside R1CS circuit).
pub fn poseidon_hash_vec(
    cs: ConstraintSystemRef<Fr>,
    values: &[FpVar<Fr>],
) -> Result<FpVar<Fr>, SynthesisError> {
    let config = poseidon_config();
    let mut sponge = PoseidonSpongeVar::new(cs, &config);
    sponge.absorb(&values)?;
    let output = sponge.squeeze_field_elements(1)?;
    Ok(output[0].clone())
}
