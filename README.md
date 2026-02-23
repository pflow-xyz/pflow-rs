# pflow-rs

Rust port of [go-pflow](https://github.com/pflow-xyz/go-pflow) — Petri net modeling with ODE simulation and token model DSL.

**Paper:** [Integer Reduction: Extracting Exact Strategic Values from Game Topologies via Petri Net ODE Equilibrium](https://github.com/pflow-xyz/pflow-rs/releases/latest/download/integer-reduction-draft.pdf)

## Crates

| Crate | Description |
|-------|-------------|
| `pflow-core` | Petri net types (Place, Transition, Arc), fluent Builder API, state map utilities |
| `pflow-solver` | ODE solvers (Tsit5, RK45, RK4, Euler, Heun, Midpoint, BS32), implicit methods, equilibrium detection |
| `pflow-tokenmodel` | Token model schema, snapshot, runtime execution, validation, content-addressed identity |
| `pflow-dsl` | S-expression DSL: lexer, parser, interpreter, builder, codegen |
| `pflow-macros` | `schema!` proc macro — compile-time DSL parsing with zero runtime overhead |
| `pflow-zk` | ZK proof traits (`PetriProver`), `IncidenceMatrix` extraction, `fire_transition()` |
| `pflow-zk-arkworks` | Groth16 prover over BN254 with Poseidon hashing (structural R1CS) |
| `pflow-zk-risc0` | risc0 zkVM STARK prover (simulation mode default; real proofs with `prove` feature) |
| `pflow` | Umbrella crate re-exporting all of the above |

## Quick Start

```rust
use pflow::*;

// Build an SIR epidemic model
let (net, rates) = PetriNet::build()
    .sir(999.0, 1.0, 0.0)
    .with_rates(1.0);

// Solve to equilibrium
let state = net.set_state(None);
let prob = Problem::new(net, state, [0.0, 100.0], rates);
let (final_state, reached) = find_equilibrium(&prob);

assert!(reached);
// S + I + R = 1000 (conserved)
```

## Petri Net Builder

```rust
use pflow_core::PetriNet;

let net = PetriNet::build()
    .place("A", 10.0)
    .place("B", 0.0)
    .transition("t1")
    .arc("A", "t1", 1.0)
    .arc("t1", "B", 1.0)
    .done();
```

Chain helper for linear sequences:

```rust
let net = PetriNet::build()
    .chain(1.0, &["start", "t1", "middle", "t2", "end"])
    .done();
```

## ODE Solver

Seven explicit Runge-Kutta methods plus implicit solvers for stiff systems:

```rust
use pflow_solver::*;

let prob = Problem::new(net, state, [0.0, 100.0], rates);

// Explicit (adaptive step size)
let sol = solve(&prob, &methods::tsit5(), &Options::default_opts());

// Implicit (stiff systems)
let sol = implicit::implicit_euler(&prob, &Options::stiff());
let sol = implicit::trbdf2(&prob, &Options::stiff());

// Auto-detect stiffness
let sol = implicit::solve_implicit(&prob, &Options::default_opts());
```

**Solver presets:**

| Preset | Use Case |
|--------|----------|
| `Options::default_opts()` | General purpose |
| `Options::fast()` | Game AI, interactive (~10x faster) |
| `Options::accurate()` | Research, publishing |
| `Options::game_ai()` | Move evaluation |
| `Options::epidemic()` | SIR/SEIR models |

## Token Model DSL

Define token model schemas using an S-expression DSL. The `schema!` macro parses and validates the DSL at compile time — syntax errors become compiler errors, and the generated code constructs the `Schema` directly with zero runtime parsing.

```rust
use pflow::schema;

let s = schema!(r#"
(schema ERC-020
  (version v1.0.0)
  (states
    (state balances :type map[address]uint256 :exported)
    (state totalSupply :type uint256)
  )
  (actions
    (action transfer :guard {balances[from] >= amount})
  )
  (arcs
    (arc balances -> transfer :keys (from))
    (arc transfer -> balances :keys (to))
  )
  (constraints
    (constraint conservation {sum(balances) == totalSupply})
  )
)
"#);

assert_eq!(s.name, "ERC-020");
assert_eq!(s.actions[0].guard, "balances[from] >= amount");
```

Invalid DSL is caught at compile time:

```rust
// This won't compile:
let s = schema!(r#"(bad input)"#);
// error: DSL parse error: expected symbol "schema", got "bad"
```

### DSL Syntax Reference

```scheme
(schema <name>
  (version <version>)

  (states
    (state <id> :kind token :initial <n>)          ; token state with initial count
    (state <id> :type <type> :exported)             ; data state, exported
  )

  (actions
    (action <id>)                                    ; simple action
    (action <id> :guard {<expr>})                    ; guarded action
  )

  (arcs
    (arc <source> -> <target>)                       ; simple arc
    (arc <source> -> <target> :keys (<k1> <k2>))     ; arc with map keys
    (arc <source> -> <target> :value <binding>)       ; arc with value binding
  )

  (constraints
    (constraint <id> {<expr>})                       ; invariant constraint
  )
)
```

### Alternative: Fluent Builder

For dynamic schema construction, use the builder API directly:

```rust
use pflow_dsl::Builder;

let schema = Builder::new("ERC-020")
    .data("balances", "map[address]uint256").exported()
    .data("totalSupply", "uint256")
    .action("transfer").guard("balances[from] >= amount")
    .flow("balances", "transfer").keys(&["from"])
    .flow("transfer", "balances").keys(&["to"])
    .constraint("conservation", "sum(balances) == totalSupply")
    .must_schema();
```

### Runtime Parsing

For DSL strings loaded at runtime (e.g. from files):

```rust
use pflow_dsl::parse_schema;

let schema = parse_schema(&dsl_string).unwrap();
```

## Content-Addressed Identity

Schemas produce deterministic content identifiers (CIDs) via SHA-256. Insertion order doesn't matter — schemas with the same structure always produce the same hash.

```rust
use pflow::schema;

let s = schema!(r#"(schema counter
  (states (state count :kind token :initial 5))
  (actions (action inc))
  (arcs (arc inc -> count))
)"#);

// Full CID (includes name/version)
let cid = s.cid();           // "cid:a1b2c3..."

// Structural fingerprint (ignores name/version)
let idh = s.identity_hash(); // "idh:d4e5f6..."

// Compare schemas
assert!(s.equal(&s));                // same CID
assert!(s.structurally_equal(&s));   // same structure
```

## Runtime Execution

```rust
use pflow_tokenmodel::{Runtime, Bindings};
use serde_json::Value;

let mut rt = Runtime::new(schema);

// Set initial balance
rt.snapshot.set_data_map_value("balances", "alice", Value::Number(1000.into()));

// Execute a transfer
let mut bindings = Bindings::new();
bindings.insert("from".into(), Value::String("alice".into()));
bindings.insert("to".into(), Value::String("bob".into()));
bindings.insert("amount".into(), Value::Number(250.into()));

rt.execute_with_bindings("transfer", &bindings).unwrap();
// alice: 750, bob: 250
```

## State Utilities

```rust
use pflow_core::stateutil;

let state = /* ... */;
let updated = stateutil::apply(&state, &updates);
let total = stateutil::sum(&state);
let changes = stateutil::diff(&before, &after);
let history = stateutil::filter(&state, |k| k.starts_with('_'));
```

## Code Generation

Generate Rust source from DSL definitions:

```rust
use pflow_dsl::generate_rust_from_dsl;

let code = generate_rust_from_dsl(dsl_input, "mymodule", "make_schema").unwrap();
// Outputs a Rust function that constructs the schema
```

## ZK Proofs

Two contrasting strategies prove the same statement — "transition T is enabled and transforms marking M into M'":

- **arkworks (structural R1CS)**: compiles net topology into Groth16 constraints over BN254 with Poseidon hashing. Constant 128-byte proofs, sub-millisecond verification.
- **risc0 (zkVM STARK)**: wraps transition logic in a RISC-V guest program. No trusted setup, but larger proofs and slower proving.

```rust
use pflow_zk::{IncidenceMatrix, PetriProver, TransitionWitness, fire_transition};
use pflow_zk_arkworks::ArkworksProver;

let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
let matrix = IncidenceMatrix::from_petri_net(&net);

let mut prover = ArkworksProver::new(matrix.clone());
prover.setup().unwrap();

let pre = matrix.initial_marking(&net);
let post = fire_transition(&matrix, &pre, 0).unwrap();
let witness = TransitionWitness { pre_marking: pre, transition_id: 0, post_marking: post };

let proof = prover.prove(&witness).unwrap();
assert!(prover.verify(&proof).unwrap());
assert_eq!(proof.metrics.proof_size_bytes, 128);
```

### Benchmarks

Head-to-head comparison using NxN tic-tac-toe nets (3n² places, 2n² transitions):

```
cargo run --example zk_compare -p pflow --features zk-arkworks,zk-risc0-prove --release
```

| Model | System | Places | Trans | Setup | Prove | Verify | Proof Size |
|-------|--------|--------|-------|-------|-------|--------|------------|
| SIR | groth16-bn254 | 3 | 2 | 26ms | 18ms | 1.0ms | 128B |
| SIR | risc0 | 3 | 2 | 0ms | 3.0s | 9.5ms | 217KB |
| TTT 3x3 | groth16-bn254 | 27 | 18 | 67ms | 78ms | 0.8ms | 128B |
| TTT 3x3 | risc0 | 27 | 18 | 0ms | 6.1s | 10ms | 239KB |
| TTT 5x5 | groth16-bn254 | 75 | 50 | 176ms | 189ms | 0.8ms | 128B |
| TTT 5x5 | risc0 | 75 | 50 | 0ms | 12.1s | 11ms | 250KB |
| TTT 10x10 | groth16-bn254 | 300 | 200 | 654ms | 709ms | 0.9ms | 128B |
| TTT 10x10 | risc0 | 300 | 200 | 0ms | 51.8s | 13ms | 275KB |
| TTT 20x20 | groth16-bn254 | 1200 | 800 | 2.6s | 2.8s | 0.9ms | 128B |
| TTT 20x20 | risc0 | 1200 | 800 | 0ms | 2m47s | 48ms | 1.05MB |
| TTT 40x40 | groth16-bn254 | 4800 | 3200 | 11.1s | 11.4s | 0.9ms | 128B |
| TTT 40x40 | risc0 | 4800 | 3200 | 0ms | 10m55s | 160ms | 3.5MB |

At 4800 places / 3200 transitions: arkworks proves **57x faster**, verifies **178x faster**, with proofs **28,000x smaller**. Groth16 verification and proof size remain constant regardless of net size. risc0 trades performance for no trusted setup and simpler guest programming.

### Feature Flags

```toml
pflow = { features = ["zk-arkworks"] }       # Groth16 prover
pflow = { features = ["zk-risc0"] }          # risc0 simulation mode
pflow = { features = ["zk-risc0-prove"] }    # risc0 real STARK proofs (requires cargo risczero install)
```

## Dependencies

Minimal external dependencies:

- `serde`, `serde_json` — serialization
- `sha2`, `hex` — content-addressed identity
- `thiserror` — error types

## License

MIT
