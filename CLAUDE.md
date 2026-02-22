# pflow-rs

Rust Petri net library with ODE simulation, token model DSL, and ZK proofs.

## Workspace Structure

| Crate | Purpose |
|-------|---------|
| `pflow-core` | Core types: `PetriNet`, `Place`, `Transition`, `Arc`, `State`, fluent `Builder` |
| `pflow-solver` | ODE solvers (Tsitouras 5/4, RK45, etc.), equilibrium detection, vectorized fast path |
| `pflow-tokenmodel` | Token model `Schema`, `Runtime`, content-addressed identity (CID) |
| `pflow-dsl` | S-expression DSL parser, code generation |
| `pflow-macros` | `schema!` proc macro for compile-time DSL validation |
| `pflow-zk` | ZK proof traits (`PetriProver`), `IncidenceMatrix` extraction, `fire_transition()` |
| `pflow-zk-arkworks` | Groth16 prover over BN254 with Poseidon hashing (structural R1CS) |
| `pflow-zk-risc0` | risc0 zkVM wrapper prover (simulation mode; full STARK requires toolchain) |
| `pflow` | Umbrella crate re-exporting all of the above |

## Build & Test

```bash
cargo test --workspace              # All tests
cargo test -p pflow-zk              # Foundation crate only
cargo test -p pflow-zk-arkworks     # Arkworks prover
cargo test -p pflow-zk-risc0        # risc0 prover (simulation)
```

## ZK Proofs

Two contrasting strategies prove the same statement: "transition T transforms marking M into M'."

### arkworks (structural)

Compiles net topology into R1CS constraints directly. Circuit structure:
1. Poseidon hash pre-marking -> assert equals public `pre_state_root`
2. Poseidon hash post-marking -> assert equals public `post_state_root`
3. Compute delta from incidence matrix (baked as circuit constants)
4. Assert `post[p] == pre[p] + delta[p]` for all places
5. Assert enabledness via non-negative remainder witnesses

Uses BN254 curve + Groth16 (128-byte proofs, ~2ms verification).

### risc0 (zkVM)

Wraps transition logic in a RISC-V guest. Currently simulation mode only.
Full STARK proofs require: `cargo risczero install`

### Running benchmarks

```bash
cargo bench -p pflow --features zk-arkworks --bench zk_bench
cargo run --example zk_compare -p pflow --features zk-arkworks,zk-risc0
```

### Feature flags

```toml
pflow = { features = ["zk-arkworks"] }  # Groth16 prover
pflow = { features = ["zk-risc0"] }     # risc0 prover
pflow = { features = ["zk"] }           # Shared traits only
```

## Key Types

- `PetriNet` — places (`HashMap<String, Place>`), transitions, arcs; all use `f64` for ODE compat
- `IncidenceMatrix` — canonical (sorted) integer representation for ZK circuits
- `PetriProver` trait — `setup()`, `prove()`, `verify()`, `verifying_key()`
- `State = HashMap<String, f64>` — place label to token count
- `Marking = Vec<i64>` — integer marking in canonical place order (ZK)

## Architecture Notes

- Places/transitions sorted alphabetically in `IncidenceMatrix` for deterministic circuit construction
- Arc weights rounded from `f64` to `i64` at the ZK boundary (ODE uses continuous, ZK uses discrete)
- The vectorized ODE solver (`pflow-solver`) groups arcs by transition identically to `IncidenceMatrix`
- Book chapters 12-13 (book.pflow.xyz) cover the gnark (Go) reference implementation
