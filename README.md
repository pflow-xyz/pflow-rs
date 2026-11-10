# pflow-rs

Explanations you can run, in Rust. Describe a system once as a Petri net and
the same net answers several questions: how its tokens flow over time (ODE
simulation, and a seeded Gillespie SSA that is byte-exact with go-pflow and
pflow.xyz), which markings it can reach and what it conserves (reachability,
coverability, P/T invariants), what its max-plus structure says (tropical
analysis), and, with a zero-knowledge proof, that one transition legally moved
it from one marking to the next.

pflow-rs is the Rust port of [go-pflow](https://github.com/pflow-xyz/go-pflow),
held to go-pflow's goldens, and runs in the browser as
[`@pflow-xyz/pflow-wasm`](https://www.npmjs.com/package/@pflow-xyz/pflow-wasm).

## Crates

| Crate | Description |
|-------|-------------|
| `pflow-core` | Petri net types (Place, Transition, Arc), fluent Builder API, state map utilities, Shape A JSON and color expansion |
| `pflow-metamodel` | Shape B model (places, transitions, arcs, stages, schedules, parameters) and the one shared firing rule |
| `pflow-parser` | Shape A to Shape B converter (the pflow.xyz editor document to a `pflow-metamodel` model) |
| `pflow-solver` | ODE solvers (Tsit5, RK45, RK4, Euler, Heun, Midpoint, BS32), implicit methods, equilibrium detection; portable Gillespie SSA, chemical Langevin SDE, and the scheduled/staged stochastic engine |
| `pflow-learn` | Parameter fitting, sensitivities, gradient and derivative-free optimizers for Petri net ODEs |
| `pflow-tokenmodel` | Token model schema, snapshot, runtime execution, validation, content-addressed identity |
| `pflow-dsl` | S-expression DSL: lexer, parser, interpreter, builder, codegen |
| `pflow-macros` | `schema!` proc macro — compile-time DSL parsing with zero runtime overhead |
| `pflow-reachability` | Reachability graph, coverability, Farkas P/T invariants, spectral analysis |
| `pflow-validation` | Structural validation checks for `pflow-metamodel` nets |
| `pflow-verify` | Property verification (deadlock-free, bounded, live, invariants, reachability) |
| `pflow-compose` | `Bundle`/`Flatten` composition, templates, derive, model-extension operations |
| `pflow-eventlog` | Process event log types plus CSV/JSONL readers |
| `pflow-mining` | Process discovery (Alpha, Heuristic Miner) and token-replay conformance checking |
| `pflow-results` | Structured simulation results (types, analysis, builder, io) |
| `pflow-visualization` | SVG rendering for Petri nets and ODE solutions |
| `pflow-tropical` | Tropical semiring (max-plus algebra) for Petri net analysis; Rust-only |
| `pflow-zk` | ZK proof traits (`PetriProver`), `IncidenceMatrix` extraction, `fire_transition()` |
| `pflow-zk-arkworks` | Groth16 prover over BN254 with Poseidon hashing (structural R1CS) |
| `pflow-cli` | `pflow` command-line binary, modelled on go-pflow's `cmd/pflow` |
| `pflow-mcp` | MCP server exposing Petri net tools (`petri_*`, plus `pflow_*` extras) |
| `pflow-wasm` | The browser build, on npm as [`@pflow-xyz/pflow-wasm`](crates/pflow-wasm/npm/README.md): fire a net, run the ODE and the portable SSA, explore its reachable states, from one self-contained ES module. The same engine is a local MCP server, [`@pflow-xyz/pflow-mcp`](crates/pflow-wasm/mcp/README.md) (`npx -y @pflow-xyz/pflow-mcp`), which also answers the pflow.xyz/mcp tools offline |
| `pflow-hosted` | The pflow.xyz/mcp tools (check, cid, edit, export, info, incidence, invariants, validate, verify, sde, what_if, bundle_*) with the hosted names, arguments and answer shapes, over pflow-rs engines; built into `@pflow-xyz/pflow-mcp`, not on crates.io |
| `pflow` | Umbrella crate: re-exports `pflow-core`, `pflow-dsl`, `pflow-solver` and `pflow-tokenmodel` as modules, a few common items (`PetriNet`, `solve`, `simulate_ssa`, `schema!`, ...), and `pflow-zk`/`pflow-zk-arkworks` behind the `zk`/`zk-arkworks` features. The other crates are used directly |

How far each crate is from go-pflow parity is tracked in `ROADMAP.md`'s
Status table.

## Quick Start

### Rust

```rust
use pflow::*;

// Build an SIR epidemic model
let (net, rates) = PetriNet::build()
    .sir(999.0, 1.0, 0.0)
    .with_rates(1.0);

// Solve to equilibrium (Problem::new refuses read and inhibitor arcs)
let state = net.set_state(None);
let prob = Problem::new(net, state, [0.0, 100.0], rates).unwrap();
let (final_state, reached) = find_equilibrium(&prob);

assert!(reached);
// S + I + R = 1000 (conserved)
```

### Browser (npm)

[`@pflow-xyz/pflow-wasm`](crates/pflow-wasm/npm/README.md) 0.3.0 is the same
engine as one self-contained ES module: fire a net, run the mass-action ODE and
the portable SSA. This is a complete page; the `.wasm` is embedded, so `init()`
makes no further network request (the page's CSP must still allow
`'wasm-unsafe-eval'`).

```html
<pre id="out"></pre>
<script type="module">
  import init, { Net, version } from "https://cdn.jsdelivr.net/npm/@pflow-xyz/pflow-wasm@0.3.0/pflow-wasm.inline.js";

  await init();

  const net = new Net({
    name: "coffee",
    places: [
      { id: "beans", initial: 3 },
      { id: "cups" },
    ],
    transitions: [{ id: "brew", rate: 0.5 }],
    arcs: [
      { from: "beans", to: "brew" },
      { from: "brew", to: "cups" },
    ],
  });

  const lines = [`pflow-wasm ${version()}`];
  lines.push(`enabled: ${net.enabled()}`);
  lines.push(`after brew: ${JSON.stringify(net.fire("brew"))}`);

  const ode = net.simulate({ tEnd: 10, maxPoints: 20 });
  lines.push(`ODE at t=10: ${JSON.stringify(ode.final)}`);

  const ssa = net.stochastic({ horizon: 10, samples: 11, realizations: 100, seed: 42 });
  lines.push(`SSA mean at t=10: ${JSON.stringify(ssa.final)}`);

  document.getElementById("out").textContent = lines.join("\n");
  console.log(lines.join("\n"));
</script>
```

Or `npm install @pflow-xyz/pflow-wasm`. The package README covers the split
entry, the full API, and what it refuses (guards, stages, rate schedules).

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

let prob = Problem::new(net, state, [0.0, 100.0], rates)?;

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

One backend proves the statement "transition T is enabled and transforms marking M into M'":

- **arkworks (structural R1CS)**: compiles net topology into Groth16 constraints over BN254 with Poseidon hashing. Constant 128-byte proofs, sub-millisecond verification.

A second, risc0 zkVM STARK backend (`pflow-zk-risc0`) and the `zk_compare` head-to-head benchmark were removed on 2026-10-21 for want of a consumer; the last version is at commit `98bfd83` (see ROADMAP.md, *Not ported, on purpose*).

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

### Feature Flags

```toml
pflow = { features = ["zk-arkworks"] }       # Groth16 prover
```

## Dependencies

The `pflow` umbrella with default features (`pflow-core`, `pflow-solver`,
`pflow-tokenmodel`, `pflow-dsl`, `pflow-macros`) needs few external crates:

- `serde`, `serde_json` — serialization
- `sha2`, `hex` — content-addressed identity
- `thiserror` — error types
- `proc-macro2`, `quote`, `syn` — the `schema!` macro

`pflow-solver` itself has no external runtime dependencies. The ZK and MCP
crates bring in more (arkworks; `rmcp`, `tokio`, `schemars`).

## Papers

- **Draft:** [Incidence Reduction: Extracting Exact Strategic Values from Game Topologies via Petri Net ODE Equilibrium](papers/integer-reduction/incidence-reduction.pdf) (source: `papers/integer-reduction/main.tex`)

## Check it yourself

The engine, the goldens it is held to and the nets they come from are all in
public repositories, so a result from pflow-rs can be re-run rather than taken
on trust. What can be reproduced, and how:

| Claim | How to re-run it |
|---|---|
| A pflow.xyz net's CID | URDNA2015 canonicalisation, SHA-256, CIDv1 base58btc. `pflow-tokenmodel`'s `canonical_cid` recomputes it, and `cargo test -p pflow-tokenmodel matches_pflow_xyz_golden_cids` holds it to the CIDs in pflow-xyz's `parity/golden.json` (vendored under `crates/pflow-tokenmodel/tests/fixtures/cid/parity/`, sha256-pinned in `pflow-xyz.lock`). The same CID is computed in Go (go-pflow, pflow-xyz `internal/seal`) and in the browser (pflow-xyz `public/seal-cid.mjs`) |
| A seeded SSA run | `pflow_solver::ssa` in portable mode gives the same doubles as go-pflow and pflow.xyz at the same seed. `cargo test -p pflow-solver --test ssa_parity` compares every parsed double with go-pflow's goldens (`crates/pflow-solver/tests/fixtures/ssa/`, pinned in `go-pflow.lock`); the ODE, SDE and scheduled engines have their own parity tests beside it |
| How an editor document is read | `cargo test -p pflow-parser --test parity` against go-pflow's editor-shape goldens |
| A pflow.xyz/mcp answer | `pflow-hosted` answers the hosted tools with the hosted names and shapes; `cargo test -p pflow-hosted every_hosted_answer_is_reproduced` replays answers captured from `https://pflow.xyz/mcp` (`crates/pflow-hosted/tests/fixtures/hosted/`, with the server commit they were captured at) |
| A deadlock or liveness verdict | `pflow-verify` and `pflow-reachability` return `true`, `false` or `unknown`. A `false` comes with a witness trace that replays with the firing rule, and a search that hit its budget says so instead of answering |
| A Groth16 transition proof | `pflow-zk-arkworks` runs setup, prove and verify locally. Setup is per run: no proving or verifying keys are published and there was no ceremony, so a proof is checked by someone who runs the setup, not by a third party holding a published key |

`./scripts/go-pflow-goldens.sh check` and `./scripts/pflow-xyz-fixtures.sh check`
confirm offline that the vendored goldens are byte-identical to what their locks
record. A golden is never regenerated to make a test pass; a mismatch is a bug
here or a deliberate change upstream (ROADMAP.md, ground rules).

The npm packages are built from this repository (`make wasm-npm`, `make
wasm-mcp`, `make engine-npm`, each with a `-test` target that runs its tests).
Byte-identical rebuilds of the published tarballs are not claimed.

## Contributing

We don't merge outside code, so pull requests won't be merged as-is.
Feature requests and bug reports are welcome: please open an
[issue](https://github.com/pflow-xyz/pflow-rs/issues).

## License

MIT (see [LICENSE](LICENSE)) for every crate, `@pflow-xyz/pflow-wasm` and
`@pflow-xyz/engine`. `@pflow-xyz/pflow-mcp` is `MIT AND Apache-2.0 AND
CC0-1.0`: the engine is MIT, the bundled [chyp-rs](https://github.com/stackdump/chyp-rs)
build is Apache-2.0 (its LICENSE and NOTICE ship in the package), and the bundled
ontology snapshot is CC0 data from [ontology.pflow.xyz](https://ontology.pflow.xyz).
