# pflow-rs Roadmap — go-pflow parity

pflow-rs is a port of go-pflow, not a sibling: go-pflow is the reference
implementation and every claim of parity here is a golden that go-pflow
generated and Rust replays. That rule already holds for the SSA (byte-exact
across Go, Rust, JS and Julia) and for `pflow-learn`; this roadmap extends it
to the rest of the library.

**Definition of "full parity."** Every go-pflow package that reads, analyses,
simulates, composes or executes a net has a Rust counterpart that produces the
same numbers from the same input, held by a fixture go-pflow produced. Server
plumbing that only makes sense as a long-lived Go process is listed at the end
under *Not ported, on purpose* with the reason for each — those are decisions
to revisit, not gaps to forget.

The acceptance test throughout is the pflow showcase
(`pflow-xyz/examples/showcase/`): one café model in eight variations that
together exercise every feature the suite has. Each phase below names which
variations it unlocks, so "how far is Rust" has a one-line answer at any time.

## Status (2026-09-08)

| go-pflow package | Rust crate | Parity | Held by |
|---|---|---|---|
| `solver` (Tsit5, RK, implicit, equilibrium) | `pflow-solver` | full | `parity/ode` goldens |
| `stochastic` portable SSA | `pflow-solver::ssa` | byte-exact | `tests/fixtures/ssa/*.json` (six goldens) |
| `stochastic` SDE | `pflow-solver::ssa::sde` | algorithm ported | Gaussian sampler vectors only; no SDE goldens exist on either side |
| `stochastic` stages, schedules, guard, supply, likelihood | — | **missing** | — |
| `learn` | `pflow-learn` | full | `parity/learn/goldens.json` |
| `tokenmodel` | `pflow-tokenmodel` | full, including go-pflow's known runtime defects (see Phase 5) | unit tests |
| DSL (petri-pilot `pkg/dsl`) | `pflow-dsl` | parses `cafe-loyalty.pflow`; adds Rust codegen | unit tests |
| `petri` (Shape A net, colors) | `pflow-core` | struct only: **no JSON reader, no unfolding** | — |
| `parser` (`ModelFromJSON`, Shape A → B) | — | **missing** | — |
| `metamodel` (Shape B, firing rule, compose, stages, schedule, parameters, access) | — | **missing** | — |
| `reachability`, `validation`, `verify` | `pflow-tropical` covers P/T invariants only | **missing** | — |
| `eventlog`, `mining` | — | **missing** | — |
| `statemachine`, `workflow`, `actor`, `engine` | — | **missing** | — |
| `templates`, `derive`, `hypothesis`, `sensitivity`, `results`, `monitoring` | — | **missing** | — |
| `zkcompile` (gnark) / `prover` | `pflow-zk`, `-arkworks`, `-risc0` | different backends by design; see *Not ported* | — |
| `visualization`, `plotter` | — | **missing** | — |
| `eventsource`, `graphql`, `cache`, `compat` | — | not planned | — |
| Rust-only: `pflow-tropical`, `pflow-macros`, risc0 zkVM | — | ahead of Go | — |

Showcase scorecard today: **kinetics** replays byte-exact once the SSA JSON
loader moves from the parity test into the library; **loyalty** parses and
executes; **order** invariants are computable; **market** runs unverified;
**theme**, **service** (stages/schedules), **bundle** and **application** do
not load.

## Ground rules

1. **go-pflow generates, Rust replays.** A golden is copied byte-identical and
   its sha256 recorded next to the go-pflow commit that produced it, the way
   `tests/fixtures/ssa/README.md` does now. A failing golden is a bug in Rust
   or a deliberate change in Go; it is never fixed by regenerating.
2. **One lock for all of it.** Phase 0 introduces `go-pflow.lock` (pinned
   go-pflow tag, plus sha256 per golden file) and `scripts/go-pflow-goldens.sh
   check|sync|status`, the same shape as `docs.lock` / `pflow-js.lock`. Every
   later phase adds files to it rather than inventing a second mechanism.
3. **Multiple parsers are fine as long as go-pflow matches.** Rust may read
   the editor's Shape A directly, but only through the editor-shape goldens
   (`go-pflow/parser/testdata/editor-shape/*.json`), which pin `parsed`,
   `expanded` and `unfolded` for every input. Same contract pflow-jl and
   pflow-xyz JS are held to.
4. **The firing rule has one home per language.** go-pflow's
   `metamodel/firing.go` (consume ≥ weight, read arc tests without consuming,
   inhibitor blocks at ≥ weight, capacity is a post-firing bound netting the
   same firing's consumption) is ported once, into `pflow-metamodel`, and every
   engine — SSA, state machine, workflow, token runtime — calls it. Five
   disagreeing copies is how petri-pilot went wrong; the Rust SSA's private
   copy is retired in Phase 0.
5. **Every phase lands with a row in the ecosystem scoreboard**
   (`~/Workspace/CAPABILITIES.md`) and in the Status table above. If it is not
   in the table it is not done.
6. **Cargo, not Bazel** (item 8, resolved in `b615351`): nothing else in the
   ecosystem's Bazel graph targets Rust, so `cargo test --workspace` plus
   clippy is the gate. Revisit only if a Go consumer needs to build Rust
   in-graph.

## Phases

Sizes are go-pflow's line count including tests, as an order-of-magnitude
proxy for effort, not an estimate.

### Phase 0 — The model (metamodel + parser + colors) · ~13k Go lines

Nothing else can be held to a golden until Rust reads what Go reads.

- **New crate `pflow-metamodel`**: Shape B `Model` with serde — places
  (`initial`, `capacity`, `x/y`), transitions (`rate`, `delay`, `role`,
  guards, `kinetic` arcs), arcs (`from`/`to`, `type: read|inhibitor`,
  `weight`, `kinetic`), `parameters`, `stages`, `schedule`, `simulation`
  (`objective`, `solver.rates`), `presentation`, `views`, `tags`, `roles`,
  `access`, `events`. Unknown fields rejected in tests, tolerated in the
  library, exactly as Go does.
- **The firing rule** (`Enabled`/`Fire`/`Inputs`/`Outputs`/`Tests`) and
  `Model::gating()` (capacity is only gating if some transition raises the
  place). The SSA's `compile()` is rewritten over it; the six SSA goldens
  prove nothing moved.
- **`ExpandStages`, `ApplyParameters`, `HasSchedules`** — the pure
  model-to-model rewrites the engines depend on.
- **`pflow-core`**: serde for Shape A (`@id`, `places`/`transitions` keyed by
  id, vector `initial`/`capacity`/`weight`, `inhibitTransition`, `token`,
  `parents`), `null` vector slots read as 0, and a port of `petri/colors.go`
  (`expand_colors`, `expand_state`, short colour names, prune arc-less
  copies, output-side inhibitor → explicit read arc).
- **`pflow-parser`** (or a module in `pflow-metamodel`): `is_pflow_json`,
  `model_from_json` — the one Shape A → Shape B converter, held to the
  editor-shape goldens' `parsed`, `expanded`, `unfolded`.
- **Introduce `go-pflow.lock`** and move the SSA README's sha256 table into
  it.

Exit: `cafe.jsonld` → `unfolded` equals the golden byte-for-byte after
canonical JSON serialisation; every SSA golden still passes through the
shared firing rule. **Unlocks: theme, kinetics, order (structurally).**

### Phase 1 — Complete `stochastic` · ~4.5k Go lines

- Stages (`dropStageContentions`, `foldThroughput`), schedules with
  **per-realization marking carry** across boundaries (`runBoundaries`,
  `ratesAt`, `simulateFrom`), `Forecast` refusing schedules, `Simulate`
  routing model-declared schedules. go-pflow v0.28.1 fixed the rounded-mean
  carry; Rust ports the fixed version and never the old one.
- `guard.go` (guard-aware propensities), `supply.go` (`ClassifySupply`:
  conserved / bounded / queue), `likelihood.go` (`FitDiscrete`, the discrete
  moment fit petri-pilot exposes as `petri_fit_discrete`).
- `Result` fields at parity: `Caveats` vs `Assumptions` kept separate,
  `Metrics` time-weighted mean/P95/utilization, `Contended`, `Depleted`.
- **SDE goldens** — a go-pflow item, not a Rust one: extend `cmd/ssa-goldens`
  to emit `sde/*.json`, then hold `ssa::sde` to them exactly as the SSA is.
  Until then the SDE row stays "algorithm ported".

Exit: showcase `fixtures/ssa-go-seed42.json` and a new go-pflow-generated
scheduled/staged golden replay `==`; `Forecast` refuses `cafe-service.json`
with the same reason string Go gives. **Unlocks: service, market (verified).**

### Phase 2 — Analysis (`reachability`, `validation`, `verify`) · ~6.4k Go lines

- Reachability graph with bound, coverability (ω-markings), deadlock and
  liveness, `farkas.go` P/T invariants (reconcile with `pflow-tropical`: one
  implementation, tropical keeps the extraction and eigenvalue work that is
  Rust-only), `spectral.go`.
- `validation` checks (`W_GUARD_OPAQUE`, capacity vs initial marking, dangling
  arcs, the +Inf clamp that bit `petri_analyze`).
- `verify`: property language (`always`, `eventually`, bounds, invariants)
  over the reachability graph, with `caveats` for what a static check cannot
  see (expression guards).

Goldens: showcase `fixtures/properties.json`; pflow-polyglot's
`parity/reachability.golden` for the coffee machine (a second, independent
contract already maintained by twenty programs).

Exit: `petri_verify`-equivalent output for `cafe-order.json` matches Go field
for field. **Unlocks: order (fully).**

### Phase 3 — Composition (`metamodel` compose, `templates`, `derive`, `patterns`) · ~5k Go lines

- `Bundle` → `Flatten` (fusion classes sharing one `Link.ID`, rendezvous at
  the slowest member's rate, `GuardLink` lowering to `tokens("<flat>") <cond>`
  or an inhibitor when the condition is `== 0`), `compose_matrix`,
  `composedot`.
- `templates` (queue, SIR), `patterns`, `derive` (evaluation nets derived from
  a declared net), `extension.go` model extension operations (the
  `petri_extend` / `sim_extend` op set, with `ops.Diff`'s modified sets).

Goldens: `cafe.bundle.json` flattened by Go and compared as canonical JSON;
showcase `fixtures/{fusions,extend-operations}.json`.

Exit: the flattened bundle simulates byte-exact against the SSA golden Go
produces from its own flattening. **Unlocks: bundle.**

### Phase 4 — Data (`eventlog`, `mining`, `results`, `sensitivity`, `hypothesis`) · ~6.4k Go lines

- `eventlog` types, CSV and JSONL readers/writers, `SortTraces`.
- `mining`: alpha and heuristic discovery, footprint matrix, token-replay
  conformance (fitness, precision), dataflow discovery, timing.
- `results` (analysis, sweep, io), `sensitivity`, `hypothesis` (the
  what-if comparison shape `petri_scenario` / `sim_compare` return).

Goldens: showcase `fixtures/{event-log,scenarios,observations,rates}.json`
and the conformance numbers `petri_conformance` reports for them.

Exit: conformance fitness for the showcase event log equals Go to the last
printed digit. **Unlocks: application's conformance half; finishes the
showcase's analysis columns.**

### Phase 5 — Execution runtimes (`statemachine`, `workflow`, `actor`, `engine`, `tokenmodel` fixes) · ~11k Go lines

- `statemachine` and `workflow` builders and engines, `metasubnet`, the
  workflow monitor; `actor` (bus, subnets, metabundle); `engine` (the
  unified ODE + event-sourced runtime petri-pilot's `pflow-engine.js`
  derives from).
- `tokenmodel::Runtime` today matches go-pflow faithfully, which means it
  also matches its defects: enablement hardcoded to `< 1` so **weights are
  ignored**, inhibitors ignored, one token moved per arc, guards never
  evaluated. Decision for this phase, taken together with go-pflow so the
  two do not diverge: either fix both on the shared firing rule from Phase 0,
  or document the runtime as the DSL/CID identity surface only and route
  execution through `pflow-metamodel`. Do not fix one side alone.
- `monitoring` (monitor, predictor) with `eventsource` in-memory store only.
- Roles and access (`access.go`) enforced in execution, so
  `cafe-application.json`'s access rules refuse what Go refuses.

Goldens: showcase `fixtures/firing-sequence.json`, `sample-path.json`;
petri-pilot's generated-app event stores are the reference for the engine.

Exit: **application** replays end to end. Showcase is 8/8.

### Phase 6 — Surfaces (`visualization`, `plotter`, CLI, MCP) · ~4.5k Go lines + tool parity

- `visualization` SVG (net, state machine, workflow) and `plotter` SVG, held
  to Go's output after whitespace normalisation.
- `pflow-mcp` tools accept Shape A and Shape B JSON, not only DSL text, and
  grow to the `petri_*` set that has a library counterpart by now
  (`analyze`, `verify`, `stochastic`, `scenario`, `compare`, `extend`,
  `conformance`, `invariants`, `canonical`, `lumping`, `dataset`). Tool
  names and response shapes follow petri-pilot exactly so a client cannot
  tell which server answered.
- `cmd/pflow` CLI parity with go-pflow's `cmd/pflow`.
- Canonical CID: URDNA2015 + dag-json CIDv1 over the `@id`-stripped document,
  held to `pflow-xyz/parity/golden.json`. Needs a JSON-LD canonicaliser in
  Rust (`json-ld` + `sophia`/`rdf-canon`, to evaluate); until it lands,
  `pflow-tokenmodel::cid()` is documented as an identity hash, not the
  ecosystem CID, and the showcase's `cid.mjs` step stays JS-only.

Exit: `pflow-rs` appears in the showcase README's score table with the same
columns as Go.

## Not ported, on purpose

| Package | Reason | Revisit when |
|---|---|---|
| `graphql` | a server surface over `eventsource`; petri-pilot is the served product and it is Go | a Rust service exists |
| `eventsource` SQLite store | persistence belongs to the service, not the library; memory store ships in Phase 5 | same |
| `prover`, `zkcompile` (gnark, Solidity) | Rust already has arkworks Groth16 and risc0 STARK over the same incidence matrix; Solidity verifier generation is the only piece worth porting, and only for bitwrap-io | bitwrap-io wants a Rust prover |
| `cache` | Go-specific memoisation of solver runs | never, most likely |
| `compat` | bridges go-pflow's own older types | never |
| `stateutil` | already in `pflow-core::stateutil` | — |

## Tracking

- Status table above is the source of truth for "how far"; update it in the
  same commit as the code.
- `CAPABILITIES.md` at the workspace root gets one row per phase exit.
- Each phase is a branch merged to `main` only with its goldens green in CI;
  goldens arrive via `scripts/go-pflow-goldens.sh sync` from a tagged
  go-pflow, never from a dirty checkout (the SSA vendoring was once done from
  a dirty tree and had to be redone).
- Where a phase needs go-pflow to *emit* a golden it does not yet emit (SDE,
  scheduled SSA, flattened bundle, verify output), that is a go-pflow change
  first, tagged, then consumed here. go-pflow's `cmd/ssa-goldens` and
  `cmd/shape-goldens` are the pattern.
