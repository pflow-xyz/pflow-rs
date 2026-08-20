# pflow-rs

Rust Petri net library with ODE simulation, token model DSL, and ZK proofs.

## Central Thesis

Petri nets are a universal formal backbone. Define your system as a Petri net and you get three capabilities from the same source of truth:

1. **Continuous simulation** — ODE solvers model token flow as differential equations, finding equilibria and dynamics
2. **Discrete execution** — the same net fires transitions step-by-step as a state machine, with a DSL and content-addressed identity (CID) for deterministic specification
3. **Zero-knowledge proofs** — the incidence matrix extracted from the net compiles directly into ZK circuits (Groth16 or STARK), proving "transition T legally transformed state M into M'" without revealing the full state

The papers extend this further — showing the incidence matrix enables algebraic reductions (integer reduction for smaller circuits) and connections to tropical geometry (earned compression).

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
| `pflow-mcp` | MCP server exposing Petri net tools (build, simulate, analyze, fire, equilibrium) |
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

Wraps transition logic in a RISC-V guest program. Two modes:
- **Simulation** (default): executes guest logic natively, no real proofs
- **Real STARK proofs** (`prove` feature): requires `cargo risczero install`

```bash
cargo test -p pflow-zk-risc0                  # Simulation mode
cargo test -p pflow-zk-risc0 --features prove # Real STARK proofs (~10s)
```

### Running benchmarks

```bash
cargo bench -p pflow --features zk-arkworks --bench zk_bench
cargo run --example zk_compare -p pflow --features zk-arkworks,zk-risc0 --release           # Sim mode
cargo run --example zk_compare -p pflow --features zk-arkworks,zk-risc0-prove --release     # Real STARK
cargo run --example zk_compare -p pflow --features zk-arkworks,zk-risc0-prove --release -- 3 5 10  # Custom sizes
```

### Feature flags

```toml
pflow = { features = ["zk-arkworks"] }       # Groth16 prover
pflow = { features = ["zk-risc0"] }          # risc0 prover (simulation)
pflow = { features = ["zk-risc0-prove"] }    # risc0 prover (real STARK)
pflow = { features = ["zk"] }                # Shared traits only
```

## MCP Server (`pflow-mcp`)

An MCP (Model Context Protocol) server that exposes Petri net tools. Configured in `.mcp.json`:

```json
{ "mcpServers": { "pflow": { "command": "cargo", "args": ["run", "-p", "pflow-mcp"] } } }
```

### Tools

| Tool | Purpose |
|------|---------|
| `pflow_validate` | Parse model, return structure summary and content-addressed ID (CID) |
| `pflow_build` | Parse model, return places, transitions, arcs, and initial state |
| `pflow_analyze` | Incidence matrix (input/output/delta per transition), enabled transitions |
| `pflow_fire` | Fire discrete transitions step-by-step, return token state after each step |
| `pflow_simulate` | ODE simulation over time, return downsampled time series |
| `pflow_equilibrium` | Find steady state of ODE system |

### DSL Syntax

Models are passed as S-expression strings. The root keyword is `schema` (not `petri-net`):

```lisp
(schema MyNet
  (version v1.0.0)
  (states
    (state p0 :kind token :initial 5)
    (state p1 :kind token :initial 0)
    (state data0 :type uint256)          ; :kind data is default
  )
  (actions
    (action t0)
    (action t1 :guard {p0 >= 2})
  )
  (arcs
    (arc p0 -> t0)                       ; weight 1 (implicit)
    (arc t0 -> p1)
    (arc p1 -> t1 :keys (from))          ; keyed arc for mappings
    (arc t1 -> p0 :value amount)
  )
  (constraints
    (constraint conserve {sum(p0, p1) == 5})
  )
)
```

**Key DSL rules:**
- `state` — `:kind token` for ODE/discrete places, `:kind data` (default) for metadata
- `action` — transitions; optional `:guard {expr}` for enablement conditions
- `arc` — `source -> target`; optional `:keys (...)` and `:value`
- All arcs have implicit weight 1.0
- Line comments with `;`

## Key Types

- `PetriNet` — places (`HashMap<String, Place>`), transitions, arcs; all use `f64` for ODE compat
- `IncidenceMatrix` — canonical (sorted) integer representation for ZK circuits
- `PetriProver` trait — `setup()`, `prove()`, `verify()`, `verifying_key()`
- `State = HashMap<String, f64>` — place label to token count
- `Marking = Vec<i64>` — integer marking in canonical place order (ZK)

## Papers

Papers live in `papers/<slug>/main.tex`. No local LaTeX install — build on pflow.dev.

### Existing papers

| Slug | Title / Topic | PDF |
|------|---------------|-----|
| `integer-reduction` | Incidence matrix reduction for ZK circuits | `incidence-reduction.pdf` |
| `earned-compression` | Tropical geometry and earned compression | `earned-compression.pdf` |

### Creating a new paper

1. Create the directory and `main.tex`:
   ```bash
   mkdir -p papers/<slug>
   ```
2. Use an existing paper as a template — both share a common LaTeX preamble (amsmath, booktabs, listings, hyperref, geometry). Copy one and replace the content:
   ```bash
   cp papers/earned-compression/main.tex papers/<slug>/main.tex
   ```
3. Edit `papers/<slug>/main.tex` with the new content.
4. Add the paper to the table above in this file.

### Building papers (remote, on pflow.dev)

```bash
# Build any paper (replace <slug> and <output-name>)
ssh pflow.dev "cd ~/Workspace/pflow-rs && git pull && cd papers/<slug> && pdflatex -interaction=nonstopmode main.tex"
scp pflow.dev:~/Workspace/pflow-rs/papers/<slug>/main.pdf papers/<slug>/<output-name>.pdf

# Examples:
ssh pflow.dev "cd ~/Workspace/pflow-rs && git pull && cd papers/integer-reduction && pdflatex -interaction=nonstopmode main.tex"
scp pflow.dev:~/Workspace/pflow-rs/papers/integer-reduction/main.pdf papers/integer-reduction/incidence-reduction.pdf

ssh pflow.dev "cd ~/Workspace/pflow-rs && git pull && cd papers/earned-compression && pdflatex -interaction=nonstopmode main.tex"
scp pflow.dev:~/Workspace/pflow-rs/papers/earned-compression/main.pdf papers/earned-compression/earned-compression.pdf
```

### Conventions

- Each paper directory contains `main.tex` (source) and a named PDF (committed for distribution)
- Papers can cross-reference each other and reference pflow-rs crate code/tests as evidence
- Use `\cite{}` with a local `\begin{thebibliography}` block (no .bib files needed for single papers)
- Figures: prefer TikZ or listings over external image files
- Keep PDFs up to date: rebuild and commit after edits

## Architecture Notes

- Places/transitions sorted alphabetically in `IncidenceMatrix` for deterministic circuit construction
- Arc weights rounded from `f64` to `i64` at the ZK boundary (ODE uses continuous, ZK uses discrete)
- The vectorized ODE solver (`pflow-solver`) groups arcs by transition identically to `IncidenceMatrix`
- Book chapters 12-13 (book.pflow.xyz) cover the gnark (Go) reference implementation

## Decommissioning

Rust port of go-pflow. No service, no host, no data.

See [Archiving, backing up and taking down a project](../stackdump-com/CLAUDE.md#archiving-backing-up-and-taking-down-a-project) for the ecosystem-wide procedure and the ordering. This section records only what **this** project holds, which is the part that differs.

No host checkout and no untracked data — nothing to back up beyond git itself.

**Specific to this project:**

- Published crates cannot be unpublished; `cargo yank` marks a version unusable without removing it.
