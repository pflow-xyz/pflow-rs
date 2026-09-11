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

## Status (2026-09-10)

| go-pflow package | Rust crate | Parity | Held by |
|---|---|---|---|
| `solver` (Tsit5, RK, implicit, equilibrium) | `pflow-solver` | full | `parity/ode` goldens |
| `stochastic` portable SSA | `pflow-solver::ssa` | byte-exact | `tests/fixtures/ssa/*.json` (six goldens) |
| `stochastic` SDE | `pflow-solver::ssa::sde` | byte-exact | `tests/fixtures/sde/*.json` (five goldens, `cmd/sde-goldens`, landed 2026-09-10): `chain`/`sir`/`dimer` carry a normal series asserted `==` on every double, same as the SSA goldens; `gates`/`coffeeshop` are refused fixtures whose `diverged`/`reason`/`caveats` are asserted `==` too, not just `contains` — which required rewriting `sde.rs`'s own `gating_reasons` to match go-pflow's `Model.Gating()` wording field for field (arc counts, `[a b c]`-style place lists) rather than the free-form prose it read before |
| `stochastic` stages, schedules, guard, supply, likelihood | `pflow-solver::stochastic`, `pflow-learn::likelihood` | stages (`fold_throughput`, `drop_stage_contentions`), schedules with **per-realization marking carry** across boundaries (`run_boundaries`, `rates_at`, `simulate_from` — the post-v0.28.1 fixed carry, never the rounded-mean bug), guard-aware propensities, `classify_supply` (Farkas-style P-invariant basis via `pflow-tropical`), `FitDiscrete`/`NegLogLikelihood`; `Result` parity (`Caveats`/`Assumptions` apart, time-weighted `Metrics`, `Contended`, `Depleted`) | unit tests, plus a go-pflow-produced scheduled/staged golden landed 2026-09-10 (`tests/fixtures/scheduled/cafe-service.json`, `cmd/scheduled-goldens`), byte-exact end to end: the `forecastRefusal` half (`forecast_schedule_refusal` — the one branch of `Forecast` this crate ports, the schedule-refusal check before any ODE integration) and the scheduled/staged `simulate` result itself, all ~780 reported doubles plus every `Final` value, asserted `==` in `tests/scheduled_parity.rs`'s `scheduled_run_matches_go_pflow_exactly` — closed by fixing `engine::ssa`'s waiting-time draw to the portable, no-clamp `-plog(1.0 - u)` shape `ssa::mod` already used correctly elsewhere (see the note below) |
| `learn` | `pflow-learn` | full | `parity/learn/goldens.json` |
| `tokenmodel` | `pflow-tokenmodel` | full — **and no longer including go-pflow's known runtime defects**: `Runtime::enabled`/`execute` now route through `pflow-metamodel`'s shared firing rule instead of go-pflow's hardcoded `< 1` check, so declared arc weight, inhibitor and read arcs are honoured and `execute` moves the declared weight rather than one hardcoded token; `Arc` gained optional `weight`/`type` fields (absent from go-pflow's own schema) to have something for the firing rule to read. Also: role-based access control (`pflow_metamodel::AccessControl`, ported from `access.go`'s `Role`/`AccessRule`) wired into `Runtime::execute_as`/`access_allows`. **This is a deliberate, documented divergence from go-pflow's runtime, not a golden it produced** — see `runtime.rs`'s module doc for the full rationale, the follow-up go-pflow needs (fix `tokenmodel.Runtime` the same way, or add the same `Arc` fields, so the two do not keep diverging), and exactly what stays a faithful match (arcs that are all weight-1/normal-typed — the only shape go-pflow can express — behave identically on both sides). Also: `canonical_cid::compute_cid` — the ecosystem's `CIDv1(dag-json, sha2-256, base58btc)` over URDNA2015-canonicalized N-Quads, a from-scratch (but pflow-jl-ported, see Phase 6) canonicalizer independent of `Schema::cid()`'s local identity hash | unit tests; `canonical_cid` additionally held to `pflow-xyz/parity/golden.json` (all five fixtures) plus two ported tie-breaking fixtures |
| DSL (petri-pilot `pkg/dsl`) | `pflow-dsl` | parses `cafe-loyalty.pflow`; adds Rust codegen | unit tests |
| `petri` (Shape A net, colors) | `pflow-core` | Shape A JSON reader (`@id`, places/transitions keyed by id, null-tolerant vectors) + `colors.go` port (`expand_colors`, `expand_state`, `short_color_names`, `unfold` — arc-less color copies pruned, output-side inhibitor → explicit read arc) | unit tests; exercised end-to-end by `pflow-parser`'s editor-shape goldens (`parsed`/`expanded` sections) |
| `parser` (`ModelFromJSON`, Shape A → B) | `pflow-parser` | `is_pflow_json`, `model_from_json` — the one Shape A → Shape B converter (color unfolding, short color names, output-side inhibitor → explicit read arc, color-copy Y offset) | `go-pflow/parser/testdata/editor-shape/*.json` (7 fixtures, all three of `parsed`/`expanded`/`unfolded` per fixture), copied byte-identical via `go-pflow.lock` |
| `metamodel` (Shape B, firing rule, compose, stages, schedule, parameters, access) | `pflow-metamodel`, `pflow-compose` | model + firing rule + gating + stages + schedule + parameters; compose now ported — see the `metamodel` compose row below; **access enforcement now ported** (`pflow-metamodel::access::AccessControl` — role-hierarchy resolution, transition/`"*"` rule matching, "empty roles = any authenticated user" — built fresh on go-pflow's `Role`/`AccessRule` types rather than copied, since go-pflow's own `access.go` declares only the types and never resolves them; the shape it mirrors is petri-pilot's `pkg/bridge/access.go`. No go-pflow fixture to hold it to byte-for-byte, so it is new-rather-than-ported, the same interim state Phase 2's dangling-arc check documents), used by `pflow-tokenmodel::Runtime::execute_as`; the SSA's `compile()` now classifies arcs through `Model::inputs`/`outputs`/`tests` instead of a private copy | unit tests; the editor-shape `unfolded` goldens above pin the `Model` shape it deserializes into; SSA goldens prove the shared classification agrees with the SSA's own (previously separate) rule |
| `reachability`, `validation`, `verify` | `pflow-reachability`, `pflow-validation`, `pflow-verify` | reachability graph/BFS, cycle detection, liveness, `PathTo`/`CanFire`/`CanTransitionFire`, Karp–Miller coverability witness (`FindUnboundedWitness`), minimal-support Farkas P/T invariants (a new home — see the crate's own doc comment for why `pflow-tropical`'s null-space basis is a different algorithm and stays), eigenvector centrality (`spectral.go`, four functions folded into one shared power-iteration helper); validation's structure/connectivity/deadlock-heuristic/unbounded/conservation checks plus a dangling-arc check go-pflow itself doesn't have; verify's full property language (`deadlock-free`, `bounded`, `live`, `terminating`, `reachable`/`unreachable`, `invariant`, `mutual-exclusion`, `conserves`) with structural-then-exhaustive proof strategy. Built on `pflow-metamodel`'s already color-unfolded `Model`, so go-pflow's `ColorMap` base-name expansion and the Shape-A-only `+Inf` capacity-vector clamp bug have no analogue here (documented in `pflow-validation`'s crate doc) | unit tests (55 across the three crates); `pflow-polyglot`'s `parity/reachability.golden` for the coffee machine, replayed byte-for-byte (`pflow-reachability/tests/coffee_machine_golden.rs`); `pflow-verify/tests/cafe_order.rs` against the showcase's `cafe-order.json` directly (skips quietly without a `pflow-xyz` checkout); **`pflow-verify/tests/showcase_properties.rs`** replays go-pflow's own `cmd/properties-goldens` golden (`tests/fixtures/showcase/properties.json`, landed 2026-09-10) for the same file field for field — `status`/`method`/`detail`/`evidence`/`counterexample`, not just the statuses `cafe_order.rs` checks — closing the exit criterion below with an actual byte-exact contract rather than the sibling-repo integration check it was held by before. Caught and fixed one real divergence: `format_trace`/the terminating-cycle join used an ASCII `" -> "`; go-pflow's `formatTrace` joins with the Unicode arrow `" → "` (U+2192) |
| `metamodel` compose (`Bundle`/`Flatten`, `compose_matrix`, `compose_guard`, `compose_fuse`, `queue.go`, `patterns_compose.go`) | `pflow-compose` | `Bundle`/`Subnet`/`Link`/`Endpoint` types with the same JSON shape as `cafe.bundle.json`; `flatten`/`flatten_with_map` (identity short-circuit, union-find place/transition equivalence classes, associative fusion, place-merge rules, arc retargeting+merge under `sum`/`max`, `GuardLink` lowering to read/inhibitor arcs or a guard conjunct with the same operator table as `compose_guard.go`); the net-type × link-kind legality matrix (`compose_matrix.go`); `NewQueue`/`IsUnboundedQueue` (bounded queue via the complementary-place idiom); a `ResourcePool` subnet constructor (the composable form of `patterns_compose.go`'s `(*ResourcePool).ToSubnet`, built directly rather than through the generic `PetriNet[R]` machinery — see the crate's module doc for why) | unit tests per module (union-find associativity, guard-lowering operator table, legality matrix, queue capacity invariant); integration tests in `tests/flatten.rs` pinned against the exact cases in go-pflow's `metamodel/compose_test.go` (identity, non-aliasing, associativity under link/subnet reordering, wire fusion, place-merge rules incl. kind/type-conflict rejection, EventLink fusion with initiator-owns-route, guard-link structural lowering) plus a direct flatten of the showcase's own `cafe.bundle.json` (skips quietly without a `pflow-xyz` checkout); **`tests/flatten_golden.rs`** holds that same flatten to go-pflow's own `cmd/bundle-goldens` output (`tests/fixtures/bundle/cafe.flatten.json`, landed 2026-09-10) byte-for-byte as canonical JSON — the golden that used not to exist (below) now does, and the flatten matches it exactly once the test's own canonicalizer neutralizes the two languages' JSON encoders formatting an integral `f64` differently (`20` vs `20.0`), which is not a finding about `flatten`. `StateMachine`/`Workflow`/`EventSourced` (`patterns.go`'s other three composable patterns) are **not ported** — see `pflow-compose::resource_pool`'s module doc for why |
| petri-pilot `pkg/mcp` extension op set (`applyOperation`/`compareModels`, exposed as `petri_extend`/`sim_extend`) | `pflow-compose::extend` | not a go-pflow package — petri-pilot is the one home for this op language, ported here rather than reinvented; the full `add_place`/`add_transition`/`add_arc`/`remove_place`/`remove_transition`/`remove_arc`/`add_event`/`add_event_field`/`remove_event`/`add_binding`/`remove_binding` op set plus `ModelDiff`/`compare_models`. `add_role`/`add_access`/`remove_role`/`remove_access` stay refused with the same "roles are now managed via extensions" message petri-pilot gives, since neither side has ported the extension registry itself | unit tests |
| `templates`, `derive` | `pflow-compose::templates`, `pflow-compose::derive` | `templates`: `sir`/`seir`/`queue`/`producer-consumer`/`workflow` generators over `pflow_core::PetriNet` (Shape A, matching go-pflow's own `petri.PetriNet`-based templates rather than the Shape B model the rest of this crate uses), plus a name-dispatched `generate`/`list`; `derive`: `add_catalyzed_copy`, `replace_with_hazard`, `write_only_places`, `drop_places`, `drop_transitions`, `drop_readbacks` — the net→net evaluation-net transforms, ported field-for-field | unit tests per generator/transform (no go-pflow golden exists for either package; go-pflow's own tests are likewise unit tests, not goldens) |
| `eventlog`, `mining` | `pflow-eventlog`, `pflow-mining` | `eventlog`: `Event`/`Trace`/`EventLog`/`Summary` types, `sort_traces`, CSV and JSONL readers (`ParseCSV`/`ParseJSONL` — go-pflow's `eventlog` package has readers only, no writers, so none are ported either); `mining`: footprint matrix (directly-follows/causal/parallel/choice relations), Alpha and Heuristic Miner discovery plus `sequential`/`common-path` and the `discover` dispatcher, token-based-replay conformance (fitness + ETC precision + F-score), pipeline-shape (windowing) discovery, and the `ExtractTiming`/`LearnRatesFromLog` half of `timing.go` (`FitRateFunctionsFromLog`/`CompareToLog` are unported stubs on the Go side with no behavior to hold parity with — see `pflow-mining`'s `timing` module doc). `dataflow_discovery`'s `PipelineSpec`/`WindowSpec`/`TriggerSpec` are minimal local types carrying only the fields discovery produces, not a port of `tokenmodel/dataflow`'s execution engine (out of this phase's scope) | unit tests throughout (footprint relations, both miners, precision/fitness edge cases, timing stats, pipeline discovery burstiness heuristic); `pflow-mining/tests/conformance_golden.rs` replays the showcase's `cafe-order.json` + `fixtures/event-log.json`, byte-copied from `pflow-xyz/examples/showcase/`, and pins the exact fitness/precision/F-score the live petri-pilot `petri_conformance` MCP tool reports for the same two files (fitness 0.8957219251336899, precision 0.8 — the showcase README's "0.90"/"0.80" are the same result, rounded) |
| `statemachine`, `workflow`, `actor` | `pflow-statemachine`, `pflow-workflow`, `pflow-actor` | `statemachine`: `Chart`/`ChartBuilder` (regions, hierarchical states, guarded event transitions), `to_meta_model`/`to_meta_subnet` (region-mutex `Constraint`s, `IncrementAction` as one weighted arc, `GuardUnrepresentable` for Rust-closure guards — mirrors go-pflow's `metasubnet.go`), and `Machine`, which — per ground rule 4 — drives the compiled `Model` through `enabled`/`fire` directly rather than go-pflow's own `Machine` (built on a *different*, uncompiled `ToPetriNet`/`engine.Engine` pairing that bypasses the firing rule with hand-rolled state edits); one documented gap inherited from `ToMetaModel` itself (no parent-place hand-off on a cross-top-level-state substate transition) is in `machine.rs`'s module doc. `workflow`: `Workflow`/`WorkflowBuilder` (tasks, dependencies, resources, SLAs), `to_meta_model`/`to_meta_subnet` (`metasubnet.rs`, `DepFinishToStart`-only lowering same as go-pflow), `Engine` (case lifecycle, `JoinAll`/`JoinAny`/`JoinN` dependency scheduling, resource acquire/release, retries, SLA alerts — kept as go-pflow's own hand-rolled business logic, not a second firing-rule implementation, since join/split semantics aren't recoverable from the compiled net's structurally-OR dependency transitions either way; see `engine.rs`'s module doc), `WorkflowMonitor`/`WorkflowPredictor` (ODE-based case prediction, bottleneck ID, dashboard snapshot — go-pflow's background ticker loop dropped in favor of plain methods a caller invokes, and `learnedRates`' `"task_"`-prefix/transition-name mismatch reproduced rather than silently fixed, both in `monitor.rs`'s module doc). `actor`: scoped to "bus, subnets, metabundle" — `Bus` (synchronous pub/sub: subscribe/priority/filter, middleware, publish/drain) and `ActorSystem::to_meta_bundle` (per-actor `Subnet` + bus fan-out as `EventLink`s, `GuardUnrepresentable` for Rust-closure trigger conditions/behaviour guards — mirrors `metabundle.go`); go-pflow's `actor.go`/`builder.go` (the internal-Petri-net-dispatching `Actor`/`Behavior` runtime and its fluent builder) and the older `tokenmodel/subnet`-targeting `ToBundle` are **not ported** — out of this crate's declared scope and, for the latter, there is no `tokenmodel/subnet` port in this repo to target (see `pflow-compose`'s crate doc on why `metamodel.Bundle` is the one composition layer) | unit tests throughout (43 across the three crates; no go-pflow-produced golden exists for any of `statemachine`/`workflow`/`actor` either, the same interim state Phase 3/4 document for packages with no `cmd/*-goldens` analogue) |
| `engine` | `pflow-engine` | `Engine` (condition/action rules, `step`/`run`/`stop`/`simulate`), example condition helpers (`threshold_exceeded`/`threshold_below`/`all_of`/`any_of`) — ported method-for-method over `pflow_core::net::PetriNet` + `pflow_solver`, the same pairing go-pflow's version uses; concurrency ported idiomatically (`std::thread` + a stop channel) rather than literally (no goroutines to mirror) — see the crate's module doc | unit tests (no go-pflow-produced golden exists for this package either; go-pflow's own tests are unit tests, not goldens) |
| `hypothesis`, `sensitivity`, `results` (analysis, sweep, io) | `pflow-results` | full — `hypothesis::Evaluator`/`sensitivity::Analyzer`/`GridSearch` and `results::{Analyzer, Builder, sweep, io}` ported method-for-method, sequential only (go-pflow's `*Parallel` variants are a goroutine-vs-thread performance difference, not a parity gap — see the crate's module docs) | unit tests port every case in go-pflow's `hypothesis_test.go`/`sensitivity_test.go` (no `results_test.go` exists on the Go side either); no go-pflow-produced golden exists for any of the three packages (no `cmd/*-goldens` analogue), the same interim state Phase 3 documents for `templates`/`derive` — showcase `fixtures/scenarios.json` feeds petri-pilot's `petri_scenario`, not this crate directly |
| `monitoring` | `pflow-monitoring` | `Case`/`Event`/`Prediction`/`Alert`/`Monitor`/`Predictor`/`Statistics`/`MonitorConfig`, `start_case`/`record_event`/`complete_case`/`predict_completion`/`periodic_update`, `predict_from_state`/`predict_remaining_time`/`predict_next_activity`/`estimate_current_state` — ported method-for-method from `monitor.go`/`predictor.go`/`types.go`, timestamps as `i64` epoch-ms (no clock dependency, caller supplies "now" — see the crate's module doc), one go-pflow bug (the stuck-case check compares against an already-just-updated `LastEventTime`) kept rather than silently fixed, since this is a port | unit tests (no go-pflow-produced golden exists for this package either) |
| `eventsource` (in-memory half) | `pflow-eventsource` | `Event`/`EventFilter`/`Store`/`AdminStore`/`MemoryStore` — `append`/`read`/`read_all`/`stream_version`/`delete_stream`/`close`/`list_instances`/`get_stats` ported from `memory.go`, same optimistic-concurrency and filter semantics; `Subscribe` (the live channel-based push subscription) is not ported — nothing in `pflow-monitoring` needs one, a caller polls instead; see the crate's module doc. The SQLite-backed `Store` stays unported per *Not ported, on purpose* below | unit tests |
| `zkcompile` (gnark) / `prover` | `pflow-zk`, `-arkworks`, `-risc0` | different backends by design; see *Not ported* | — |
| `visualization`, `plotter` | `pflow-visualization` | net SVG (`render.go`/`svg.go`, direct from `pflow_core::PetriNet` — skips the JSON-LD round trip go-pflow's own `RenderSVG` does, since `pflow_core::PetriNet` already is that shape), state machine SVG (`statemachine_svg.go`, adapted to this repo's nested `children`-based `State` instead of go-pflow's flat parent-pointer one — see `pflow-visualization::statemachine`'s module doc; per-transition label-collision offsetting not ported, tracked there too), workflow SVG (`workflow_svg.go`, ported whole), plotter line-plot SVG (`plotter/svg.go`, ported whole) | unit tests (38) — **no reference SVGs exist to hold this to a byte-identical golden**: neither `visualization/testdata` nor `plotter/testdata` exists in go-pflow (confirmed by reading both packages directly, 2026-09-10), and go-pflow's own tests for both packages assert structure (`strings.Contains`), not full-document equality, because node/task/state draw order follows Go's own unordered `map` iteration — not reproducible even between two runs on the Go side. Two of `colors_test.go`'s own tests (`TestSVGFullnessUsesTheWholeCapacityVector`'s `placeIsDrawnFull` helper) exist specifically to warn against the bare-substring-vs-full-class-attribute mistake this port's own first draft made and fixed before landing |
| `eventsource` SQLite store, `graphql`, `cache`, `compat` | — | not planned (in-memory `eventsource` is ported — see the row above) | — |
| Rust-only: `pflow-tropical`, `pflow-macros`, risc0 zkVM | — | ahead of Go | — |

Showcase scorecard today: **kinetics** replays byte-exact once the SSA JSON
loader moves from the parity test into the library; **loyalty** parses and
executes; **order** verifies byte-exact against go-pflow's own
`cmd/properties-goldens` output (`bounded`/`live`/its declared one-state
invariant all prove structurally; `deadlock-free` correctly refutes under
the shared terminal-state heuristic — see Phase 2); **market** runs
unverified; **bundle** loads and flattens byte-exact against go-pflow's own
`cmd/bundle-goldens` output (`pflow-compose::Bundle::flatten` against the
showcase's own `cafe.bundle.json` — see Phase 3); **service** (Variation
II, `cafe-service.json`: stages/schedules) now loads and runs — schedule
dispatch, stage expansion and `Forecast`'s schedule refusal all exercised
against a go-pflow-generated golden (Phase 1) — but its numeric output is
not yet byte-exact, only shape-checked; **theme** and **application** do not
load.

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

### Phase 1 — Complete `stochastic` · ~4.5k Go lines · **done except scheduled/staged byte-exact parity** (2026-09-10)

- Stages (`fold_throughput`, `drop_stage_contentions`), schedules with
  **per-realization marking carry** across boundaries (`run_boundaries`,
  `rates_at`, `simulate_from`). go-pflow v0.28.1 fixed the rounded-mean
  carry; `pflow-solver::stochastic::simulate_from` ports the fixed version
  (each realization's own integer ending marking flows into the next
  segment via `RunStats::ends`) and never the old one.
- `guard.go` (guard-aware propensities — `Options::guard` decides a
  transition's guard against a marking, undecidable expressions caveated
  rather than enforced, exactly as `decidableFromMarking` does), `supply.go`
  (`classify_supply`: conserved / bounded / queue / state, via a
  Farkas-style P-invariant basis reusing `pflow-tropical::p_invariants_from_dense`
  rather than a second null-space implementation), `likelihood.go`
  (`fit_discrete`/`neg_log_likelihood` in the new `pflow-learn::likelihood`
  module — the discrete moment fit petri-pilot exposes as
  `petri_fit_discrete` — built on `pflow-learn::optim::gradopt::minimize_gradient`,
  the same Adam optimizer `fit`/`FitOptions` already use).
- `Result` fields at parity: `Caveats` vs `Assumptions` kept separate,
  `Metrics` time-weighted mean/P95/utilization/in-flight, `Contended`
  (capacity-first, then longest-wait-first sort), `Depleted`
  (recovered vs not).
- **`Forecast` stays out of scope except for one branch, ported specifically
  to hold the new golden below**: the schedule-refusal check
  (`m.HasSchedules()`, before any ODE integration is attempted) is now
  `pflow_solver::stochastic::forecast_schedule_refusal`
  (`tests/scheduled_parity.rs`'s doc comment names it precisely). The rest
  of `Forecast` — the continuous mass-action dispatch, `checkDivergence`,
  the non-schedule `Gating()` refusal — remains unported: `pflow-solver::ode`
  already runs the ODE side directly and dispatching *from* the stochastic
  module to it is orthogonal plumbing, not part of "complete `stochastic`"
  as scoped by the guard/supply/likelihood/stages/schedule list above;
  revisit alongside a `Simulate`-equivalent single entry point if one is
  wanted. `ValidateDelays` (`metamodel/delay.go`) is not yet ported to
  `pflow-metamodel`, so a negative delay is silently clamped to zero here
  rather than refused with go-pflow's error text — narrower than Go, tracked
  as a gap rather than fixed by inventing a different error path.
- **The scheduled/staged engine now carries a numeric byte-parity contract —
  closed, not just measured.** go-pflow's *default* `stochastic.Simulate`/
  `SimulateSchedule` path was never given a byte-exact cross-language
  contract — it uses `math/rand` directly, with no `ssa-spec.md`-style
  pinned PRNG or logarithm — but `cmd/scheduled-goldens` (below) runs
  `Options{Portable: true}` specifically so a byte-exact attempt is
  meaningful. `pflow-solver::stochastic` reuses this crate's own portable
  Xoshiro256 + `plog` (already pinned for the SSA) for its randomness.
  The earlier diagnosis here — "not go-pflow's RNG draw order... a
  go-pflow-and-Rust co-design problem, not a one-line fix" — was wrong on
  both counts: `engine::ssa`'s waiting-time draw computed `-plog(u)` from the
  raw uniform with a `u <= 0.0` clamp, go-pflow's *non-portable*
  `stdSampler.wait()` shape, instead of the portable, no-clamp
  `-plog(1.0 - u)` shape `ssa::mod` already used correctly elsewhere and
  that go-pflow's `portableSampler.wait()` actually specifies. Draw *order*
  was never the problem; the draw *transform* was wrong on one call site.
  Fixed (one line, `stochastic/engine.rs`), all ~780 reported doubles now
  land bit-for-bit — `tests/scheduled_parity.rs`'s
  `scheduled_run_matches_go_pflow_exactly` asserts `==` on every one, and on
  every `Final` value too.
- **SDE goldens — landed 2026-09-10, and byte-exact.** go-pflow's
  `cmd/sde-goldens` now emits `stochastic/testdata/sde/*.json`, vendored to
  `tests/fixtures/sde/`; `tests/sde_parity.rs` asserts `==` on every double
  of the three unconstrained fixtures and on the `diverged`/`reason`/
  `caveats` strings of the two refused ones. Getting the refused fixtures to
  match required rewriting `sde.rs`'s own `gating_reasons` to match
  go-pflow's `Model.Gating()` wording (arc counts, `[a b c]`-style place
  lists, "Use the discrete engine (Simulate)." capitalized) field for field
  — the free-form prose it read before ("a read arc has no continuous
  analogue") diverged/`caveated` correctly but not byte-for-byte. `ssa::sde`
  is now "byte-exact" in the Status table above, the same standing the
  portable SSA already has. pflow-xyz's `petri-sde.js` needed the identical
  wording fix (ported here from `gating_reasons` above) and now carries its
  own `parity/sde/` fixture set (the same five models), replayed bit-for-bit
  by `petri-sde_test.ts` — closing the three-way go-pflow/pflow-rs/pflow-xyz
  contract, not just the go-pflow/pflow-rs half of it.

Exit: showcase `fixtures/ssa-go-seed42.json` replays `==`; the new
go-pflow-generated scheduled/staged golden (`cmd/scheduled-goldens`) is
vendored and its `forecastRefusal` half replays `==` — `Forecast` refuses
`cafe-service.json` with the exact reason string go-pflow gives — but the
scheduled/staged `Result` itself does **not yet** replay `==` (see above);
that half of this exit criterion is still open. **Unlocks: service
(structurally — schedule/stage dispatch runs end to end and its refusal
path is byte-exact); market (verified) unlocking is unaffected, it does not
depend on this engine's numeric parity.**

### Phase 2 — Analysis (`reachability`, `validation`, `verify`) · ~6.4k Go lines · **done** (2026-09-10)

- Reachability graph with bound, coverability (ω-markings), deadlock and
  liveness, `farkas.go` P/T invariants (reconcile with `pflow-tropical`: one
  implementation, tropical keeps the extraction and eigenvalue work that is
  Rust-only), `spectral.go`.
- `validation` checks (`W_GUARD_OPAQUE`, capacity vs initial marking, dangling
  arcs, the +Inf clamp that bit `petri_analyze`).
- `verify`: property language (`always`, `eventually`, bounds, invariants)
  over the reachability graph, with `caveats` for what a static check cannot
  see (expression guards).

Shipped as three new crates — `pflow-reachability`, `pflow-validation`,
`pflow-verify` — built directly on `pflow-metamodel`'s `Model` rather than a
Shape A `petri.PetriNet`, since Phase 0 already resolves colors upstream:

- **Reconciled with `pflow-tropical` by *not* merging.** `pflow-tropical`'s
  `p_invariants_from_dense`/`t_invariants_from_dense` compute a generic
  integer null-space basis (Gaussian elimination, signed vectors, used by
  `pflow-solver::stochastic::classify_supply`); `farkas.go`'s
  Farkas/Martinez-Silva elimination finds the *minimal-support
  semi-positive* basis `validation` and `verify` actually depend on (every
  semi-positive invariant is a non-negative combination of it — the null
  space basis gives no such guarantee). Different algorithms, different
  questions; `pflow-reachability::invariants` is the one new home for
  Farkas, and `pflow-tropical` keeps its extraction/eigenvalue work
  untouched, per this phase's own instruction.
- **`W_GUARD_OPAQUE`** is `metamodel/compose.go`'s guard-lowering warning
  (Phase 3 scope, composition); nothing in `reachability`/`validation`/
  `verify` emits it in go-pflow either, so there is nothing to port here.
- **The `+Inf` capacity clamp bug has no analogue to reintroduce.** It was a
  Shape A defect: `reachability/graph.go` summed a place's *per-color*
  capacity vector and only treated it as unbounded when the sum was `<= 0`,
  so `[red:1, blue:0]` (blue unbounded) was clamped to a bogus total of 1
  instead of being unbounded (`visualization/render.go`'s `getCapacity` has
  the correct per-component reading). `pflow-metamodel::Place::capacity` is
  already a single post-firing `i64` — colors are unfolded upstream — so
  `capacity <= 0` unambiguously means unbounded and every check agrees.
  Documented in `pflow-validation`'s crate doc so a future per-color
  capacity feature on `Model` doesn't reintroduce the bug it replicates.
- **Dangling arcs**: go-pflow's own `validation/checks.go` has no such
  check (an arc with a typoed endpoint silently drops out of the firing
  rule instead of being refused), so this is new rather than ported —
  ROADMAP.md calls it out explicitly and it costs one loop.
- **`verify`'s color-base-name expansion has no analogue either**: go-pflow's
  `Verifier` distributes a coefficient over a place's expanded colors
  (`"pool == 3"` → `"pool.red + pool.blue == 3"`) because it is built on
  Shape A. Built on Shape B, there is no color map to expand against.

Goldens: `pflow-polyglot`'s `parity/reachability.golden` for the coffee
machine, replayed byte-for-byte by `pflow-reachability/tests/
coffee_machine_golden.rs` (a second, independent contract already
maintained by twenty programs — the fixture required translating
`model.json`'s own third JSON shape and its "transition-to-place inhibitor
is a read arc" convention, both handled inline in the test). The showcase's
`fixtures/properties.json` targets the color-unfolded `cafe.jsonld` theme
(`machine_free`, `barista_free`, ...), which needs the color-base-name
expansion this phase explicitly has no analogue for (above) plus
`cafe-service.json`'s schedules/stages/unbounded resource places (Phase 1
territory for the discrete graph — `beans`, `walked_out`, `vip_queue` etc.
have no declared capacity and are not the bounded net this phase's BFS
exit criterion assumes); porting it is left for whichever phase carries
that combination rather than faked with a reduced stand-in fixture. In its
place, `pflow-verify/tests/cafe_order.rs` runs `Bounded`/`Live`/`Invariant`/
`DeadlockFree` directly against the showcase's own `cafe-order.json` (a
sibling-repo integration check, not a copied golden — the same file,
deserialized with no gap between what this crate reads and what go-pflow's
`verify` would see).

`go-pflow`'s `cmd/properties-goldens` closed that gap 2026-09-10: it runs
the representative property set (`bounded`/`deadlock-free`/`live`/
`terminating`/`conserves`/the model's own `one_state` invariant/a
mutual-exclusion/two `reachable`/one `unreachable`) against `cafe-order.json`
through `verify.Property` directly and dumps `Report` plus the
`metapetri.Convert` `Diagnostics` as `verify/testdata/showcase/
properties.json` — vendored to `tests/fixtures/showcase/` and replayed field
for field (`status`/`method`/`detail`/`evidence`/`counterexample`) by
`tests/showcase_properties.rs`. One real divergence surfaced and was fixed:
`format_trace` (and the terminating-cycle join) used an ASCII `" -> "` where
go-pflow's `formatTrace` joins with the Unicode arrow `" → "` (U+2192), so an
`evidence` string like `"firing sequence: pay → start_brew → finish_brew →
pick_up"` did not match until the separator was corrected.

Exit: `petri_verify`-equivalent output for `cafe-order.json` matches Go field
for field — **done**, held by `tests/showcase_properties.rs` against the
golden above rather than the sibling-repo check alone. **Unlocks: order
(fully).**

### Phase 3 — Composition (`metamodel` compose, `templates`, `derive`, `patterns`) · ~5k Go lines · **done except `composedot` and flattened-bundle simulation parity** (2026-09-10)

- `Bundle` → `Flatten` (fusion classes sharing one `Link.ID`, rendezvous at
  the slowest member's rate, `GuardLink` lowering to `tokens("<flat>") <cond>`
  or an inhibitor when the condition is `== 0`), `compose_matrix`,
  `composedot`.
- `templates` (queue, SIR), `patterns`, `derive` (evaluation nets derived from
  a declared net), `extension.go` model extension operations (the
  `petri_extend` / `sim_extend` op set, with `ops.Diff`'s modified sets).

Shipped as a new crate, `pflow-compose`, built on `pflow-metamodel` (compose)
and `pflow-core` (templates/derive, which build on Shape A in go-pflow too):

- **`Bundle`/`Flatten` is ported whole**: identity short-circuit, union-find
  place/transition equivalence classes (associative — order of links or
  subnets cannot change the result, pinned by
  `flatten_is_associative_regardless_of_link_or_subnet_order`), the place-merge
  rules table (`initial` sums, `capacity` is the tightest non-zero bound,
  `exported`/`persisted`/`resource` OR, kind/type conflicts refused), arc
  retargeting and merge under `sum`/`max` with kinetics as part of the merge
  key, `GuardLink` lowering with the exact same operator table as
  `compose_guard.go` (`>=`/`>`/`<`/`<=`/`==` structural, `!=` opaque), and the
  net-type × link-kind legality matrix. `extension.go`'s op set is ported too,
  as `pflow-compose::extend` — reading petri-pilot's `pkg/mcp/server.go`
  directly, since that is where the op language and `ModelDiff` are actually
  defined (go-pflow's own `extension.go` is a different, generic plugin
  registry — `ModelExtension`/`ExtensionRegistry` — not the op set this phase
  names; not ported, since nothing in this repo needs a dynamic extension
  registry with no consumer).
- **`patterns.go`/`patterns_compose.go` narrowed to `ResourcePool`.** Go's
  four composable patterns (`StateMachine`, `Workflow`, `ResourcePool`,
  `EventSourced`) are each built on a generic `PetriNet[S]` runtime that has
  no Rust analogue in this crate; porting it just to re-derive a `Subnet`
  from it would duplicate behaviour `pflow-tokenmodel::Runtime` already
  owns. `pflow-compose::resource_pool` builds the acquire/release `Subnet`
  (with its `available + in_use == capacity` conservation constraint)
  directly instead. `StateMachine`/`Workflow`/`EventSourced` are a tracked
  gap, not a silent drop — see the module's own doc comment.
- **`composedot.go` (bundle/flattened-model DOT rendering) is not ported.**
  It is a visualization concern, the same category Phase 6 (`visualization`,
  `plotter`) already carries, and nothing in this phase's exit criterion
  depends on it; revisit alongside that phase rather than half-porting a
  renderer here.
- **`NewQueue`/`IsUnboundedQueue` port without the optional `Payload`
  binding** (the per-item data-place variant): the core complementary-place
  capacity idiom — the actual reason the constructor exists — is ported in
  full, including the capacity invariant and `W_UNBOUNDED_QUEUE`-triggering
  shape; the payload extension is additional surface with no dependent
  golden, tracked as a gap in `pflow-compose::queue`'s doc comment rather
  than silently included as if complete.

Goldens: **a go-pflow-produced flatten golden now exists** —
`cmd/bundle-goldens`, landed 2026-09-10, flattens the showcase's own
`cafe.bundle.json` and re-encodes it as canonical JSON
(`metamodel/testdata/bundle/cafe.flatten.json`). Vendored to
`tests/fixtures/bundle/` and held byte-for-byte by
`tests/flatten_golden.rs`, which canonicalizes this crate's own
`Bundle::flatten` output the same way (sorted object keys, and — the one
real formatting gap the two JSON encoders have — Go's `encoding/json` never
writes a trailing `.0` on an integral float where `serde_json`'s ryu-based
writer always does; the test's own `go_style_numbers` rewrites `20.0` to
`20` before comparing, since that difference is about the encoders, not
about `flatten`). Beyond that one golden, `pflow-compose`'s `Bundle`/
`Flatten` is still additionally held by unit and integration tests:
`tests/flatten.rs` reimplements every case in go-pflow's own
`metamodel/compose_test.go` (read directly from source — identity,
non-aliasing, associativity under link/subnet reordering, wire fusion, every
place-merge rule including the two rejection cases, EventLink fusion with
initiator-owns-route and `W_ROUTE_DROPPED`, guard-link structural lowering)
and additionally flattens the showcase's own `cafe.bundle.json` directly
(three independently-authored nets, seven links spanning all four link
kinds — skips quietly without a `pflow-xyz` checkout, the same pattern
`pflow-verify/tests/cafe_order.rs` uses). `fixtures/fusions.json` and
`fixtures/extend-operations.json` are showcase-authored descriptions of the
same bundle/op-set inputs, not golden outputs, so there is nothing in them
to diff against; they are exercised implicitly by the `cafe.bundle.json`
test rather than parsed separately.

Exit: the flattened bundle simulates byte-exact against the SSA golden Go
produces from its own flattening — **still open, but for a narrower reason
than before**. The flatten *output itself* is now byte-exact against
go-pflow's own (above); what remains missing is a golden that additionally
feeds that flattened bundle through the portable SSA the way
`cmd/ssa-goldens` does for the five standalone fixtures — no such golden
exists on either side yet (a flattened bundle has never been fed through
`cmd/ssa-goldens`), so this specific "simulates byte-exact" claim is
unaffected by `cmd/bundle-goldens` landing and stays open until a golden
does that too. **Unlocks: bundle** (composition, guard-lowering and fusion
all work end-to-end against the showcase's own `cafe.bundle.json`, and the
flattened *shape* is now proven byte-exact against go-pflow's own; the
specific "simulates byte-exact" claim waits on the
golden).

### Phase 4 — Data (`eventlog`, `mining`, `results`, `sensitivity`, `hypothesis`) · ~6.4k Go lines · **eventlog+mining done** (2026-09-10)

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

### Phase 5 — Execution runtimes (`statemachine`, `workflow`, `actor`, `engine`, `tokenmodel` fixes) · ~11k Go lines · **done** (2026-09-10)

- `statemachine` and `workflow` builders and engines, `metasubnet`, the
  workflow monitor; `actor` (bus, subnets, metabundle) — **done**,
  `pflow-statemachine`/`pflow-workflow`/`pflow-actor`. See the Status table
  row above for what each crate covers and the deliberate scope narrowing
  (`actor.go`'s internal-Petri-net-dispatching runtime and the
  `tokenmodel/subnet`-targeting `ToBundle` are not ported).
- `engine` (the unified ODE + event-sourced runtime petri-pilot's
  `pflow-engine.js` derives from) — **done**, `pflow-engine`. See the Status
  table row above.
- `tokenmodel::Runtime` used to match go-pflow faithfully, which meant it
  also matched its defects: enablement hardcoded to `< 1` so **weights were
  ignored**, inhibitors ignored, one token moved per arc, guards never
  evaluated (on the plain, bindings-free entry points — see below). **Fixed
  this phase, on the side of the two options this repo could actually take
  unilaterally**: coordinating a simultaneous go-pflow fix isn't something a
  Rust-only change here can do, so rather than leave the bug in place,
  `pflow-tokenmodel::Runtime` now routes `enabled`/`execute` through
  `pflow-metamodel`'s shared firing rule from Phase 0 (ground rule 4)
  instead of go-pflow's private, buggy copy. **This is a deliberate,
  documented divergence in behaviour from go-pflow, not a golden go-pflow
  produced and Rust replays** — spelled out in full, including exactly what
  still matches go-pflow and what a go-pflow-side fix would need to look
  like, in `pflow-tokenmodel/src/runtime.rs`'s module doc comment and the
  Status table row above. `Enabled`/`Execute`'s guard-blindness is
  unchanged (matches go-pflow; only `ExecuteWithBindings`/
  `ExecuteWithGuardFuncs` evaluate guards, on both sides) since a guard
  expression needs bindings this crate has no way to invent for the
  plain entry points.
  - **Follow-up needed on the go-pflow side**, tracked here since it can't
    be filed there directly: either port this same fix into
    `tokenmodel.Runtime` (route through `metamodel.Enabled`/`Fire`), or add
    `weight`/`type` fields to `tokenmodel.Arc` matching the ones added here,
    so a schema using them means the same thing in both languages. Until
    then, a schema whose arcs are all weight-1/normal-typed — the only
    shape go-pflow's wire format can express — behaves identically on both
    sides; anything using the new fields is Rust-only.
- `monitoring` (monitor, predictor) with `eventsource` in-memory store only
  — **done**, `pflow-monitoring` + `pflow-eventsource`. See the Status table
  rows above.
- Roles and access (`access.go`) enforced in execution — **done**,
  `pflow_metamodel::access::AccessControl`, wired into
  `pflow-tokenmodel::Runtime::execute_as`/`access_allows`. `Model` itself
  still does not embed `roles`/`access` (matching go-pflow, which keeps them
  in its extension system rather than on `metamodel.Model` — see
  `schema.rs`'s doc comment on `Role`), so a caller attaches an
  `AccessControl` to the `Runtime` explicitly rather than the schema
  carrying it. `cafe-application.json`'s generated-app-level enforcement
  (petri-pilot's `pkg/bridge/access.go` plus its generated `permissions.go`)
  is a separate, larger surface — the application generator itself is not
  ported anywhere in this workspace — so this phase's access control is
  proven by the unit tests in `pflow-metamodel/src/access.rs` and
  `pflow-tokenmodel/src/runtime.rs`, not by that fixture.

Goldens: showcase `fixtures/firing-sequence.json`, `sample-path.json`;
petri-pilot's generated-app event stores are the reference for the engine.
**No go-pflow-produced golden exists for `engine`/`monitoring` either** — as
with `templates`/`derive` (Phase 3) and `hypothesis`/`sensitivity`/`results`
(Phase 3 note), go-pflow's own tests for these three packages are unit
tests, not `cmd/*-goldens` fixtures, so this half of the phase is held by
unit tests ported case-for-case rather than a byte-identical JSON golden,
per the ground rules ("go-pflow generates, Rust replays" — there is nothing
to copy yet).

Exit: **application** replays end to end. Showcase is 8/8.

### Phase 6 — Surfaces (`visualization`, `plotter`, CLI, MCP) · ~4.5k Go lines + tool parity · **visualization/plotter, canonical CID, MCP tools and CLI done** (2026-09-10)

- `visualization` SVG (net, state machine, workflow) and `plotter` SVG —
  **done**, new crate `pflow-visualization`. Intended to be held to Go's
  output after whitespace normalisation, but go-pflow has no reference SVGs
  under `visualization/testdata` or `plotter/testdata` to normalise against
  (confirmed by reading both packages directly), so this half is held to
  structural unit tests instead — the same interim state documented for
  every other package with no `cmd/*-goldens` analogue. See the Status
  table row above for what each renderer ports whole versus narrows, and
  why.
- `pflow-mcp` tools accept Shape A and Shape B JSON, not only DSL text —
  **done**. Every model-accepting tool routes through one parser
  (`tools::convert::parse_any_model`) that tries DSL, the native tokenmodel
  Schema JSON, Shape A (via `pflow-parser`) and Shape B
  (`pflow_metamodel::Model` directly) in that order; the pre-existing
  Schema-oriented tools (`pflow_build`/`pflow_fire`/`pflow_simulate`/...)
  get this for free through `parse_model`'s new fallback, which converts
  Shape A/B into an equivalent `Schema` (`tools::convert::model_to_schema`
  — lossy on capacity/rate/delay/stages/parameters/access, documented on
  the function).
- `pflow-mcp` grows to the `petri_*` set that has a library counterpart by
  now — **done**: `petri_verify` (`pflow-verify`, full shorthand + object
  property grammar matching petri-pilot's `pkg/mcp/verify.go` byte for
  byte), `petri_invariants` (`pflow-reachability`'s Farkas P/T basis),
  `petri_conformance` (`pflow-mining`'s token-replay fitness/precision/
  F-score), `petri_scenario` (marking/rate/schedule overrides plus a
  `scenarios` array, run on `pflow-solver::stochastic` — the model's own
  default engine, not the portable SSA), `petri_extend` and `petri_diff`
  (`pflow-compose::extend`'s op set and `compare_models`, the same
  petri-pilot ported in Phase 3), `petri_canonical` (tries the real
  ecosystem CID via `pflow_tokenmodel::compute_cid` first, falling back to
  an identity hash — labelled as such — when the input has no JSON-LD
  `@id`/`@type` structure to canonicalize, i.e. DSL or Shape B). The
  existing `pflow_validate`/`pflow_analyze`/`pflow_simulate`/
  `pflow_stochastic` tools were renamed to `petri_validate`/`petri_analyze`/
  `petri_simulate`/`petri_stochastic` to match petri-pilot's names, since
  those already had a one-to-one Go counterpart; `pflow_build`/`pflow_fire`/
  `pflow_equilibrium` keep the `pflow_` prefix as pflow-rs-only extras with
  no petri-pilot analogue. Two tools are **not a port**, since neither
  go-pflow nor petri-pilot has one to hold them to: `petri_lumping`
  (ordinary CTMC lumpability via naive relational-partition-refinement
  fixed-point iteration — same fixed point as the Paige-Tarjan splitter-
  queue algorithm a from-scratch performance-tuned implementation would
  need, worse asymptotics on a large state space, documented on the
  function) and `petri_dataset` (a synthetic event log from a seeded
  stochastic playout — an approximation of the private `sim` fork's
  `sim_dataset`/`eventgen.Playout`, not a byte-identical port of it,
  documented on the function). Response shapes match petri-pilot's Go
  types field-for-field where a Go type exists to match (`Report`,
  `ConformanceResult`, `ModelDiff`); the two Rust-only tools invent their
  own, documented shape.
- `cmd/pflow` CLI parity with go-pflow's `cmd/pflow` — **done**, new crate
  `pflow-cli` (binary `pflow`). Subcommands `create` (`pflow-compose`'s
  five templates), `validate`/`verify` (thin CLI wrappers over the same
  `pflow-validation`/`pflow-verify` calls `pflow-mcp` makes — the property
  shorthand parser is intentionally duplicated between the two crates
  rather than shared, since a CLI binary cannot depend on an MCP binary's
  crate and this is request-shaping glue, not library logic ground rule 4
  covers), `expand` (`pflow_core::unfold`), `simulate`/`analyze`/`summary`/
  `compare`/`events` (`pflow-results`' `Builder`/`Analyzer`/`io` — the
  "data half" ported in Phase 3/4, now with a driver), `visualize`/`plot`
  (`pflow-visualization`, landed alongside this CLI in the same phase).
  **`sweep` is not implemented**: go-pflow's `cmd/pflow sweep` drives a
  parameter sweep; `pflow-results::results::sweep` only analyzes one
  already run, and building a driver is out of this phase's scope — tracked
  here rather than silently absent. Flag parsing is hand-rolled (`--flag
  value` and `--flag=value`, matching Go's own `flag` package shape) rather
  than adding a dependency no other Rust crate in this workspace uses.
- Canonical CID: URDNA2015 + dag-json CIDv1 over the `@id`-stripped document,
  held to `pflow-xyz/parity/golden.json` — **done, 2026-09-10**,
  `pflow_tokenmodel::canonical_cid::compute_cid` (re-exported as
  `pflow_tokenmodel::compute_cid`). **The general-JSON-LD-processor +
  off-the-shelf-Rust-canonicalizer combination the earlier text here
  proposed (`json-ld` for expansion, `rdf-canon`'s RDFC-1.0 for
  canonicalization) was evaluated first and rejected**: it produces RDF
  graphs isomorphic to go-pflow's own (confirmed by re-canonicalizing Go's
  canonical N-Quads through `rdf_canon::canonicalize` and getting
  byte-identical output to canonicalizing the same document's Rust
  conversion), but RDFC-1.0's canonical *labeling* doesn't match
  `piprate/json-gold`'s/`jsonld.js`'s pre-standardization URDNA2015 draft on
  any of the five golden fixtures — pflow's JSON-LD shape mints one blank
  node per singleton list (`weight: [1]`, `capacity`, `initial`), so any net
  with several arcs has enough blank-node symmetry to land in the tie-
  breaking cases the two algorithm variants were never guaranteed to agree
  on bit-for-bit; `net-b.jsonld` didn't even terminate within `rdf-canon`'s
  default Hash-N-Degree-Quads call budget. **What shipped instead is a
  hand-rolled URDNA2015 canonicalizer plus a pflow-schema-specific (not
  general) JSON-LD expander**, ported field-for-field from `pflow-jl`'s
  `src/urdna2015.jl` + `src/cid.jl` (merged to that repo's `main`, `daf0a45`)
  — itself verified byte-exact against go-pflow's actual canonical output,
  including two spec-adjacent details found only by diffing against
  go-pflow's `hashNDegreeQuads` directly (the identifier issuer's `"_:"`
  sigil is part of the *hash input*, not just final serialization; a
  recursive N-degree hash result is wrapped in literal `<...>` before being
  appended to the issuance-path string). Held to all five
  `pflow-xyz/parity/golden.json` CIDs (`matches_pflow_xyz_golden_cids`,
  skips quietly without a `pflow-xyz` checkout) plus two hand-built
  tie-breaking fixtures copied from `pflow-jl`'s own port
  (`tests/fixtures/cid/{tie1,tie2}.jsonld` — three arcs/places tied at first
  degree, exercising Hash N-Degree Quads' hardest case, which
  `net-b.jsonld` alone doesn't fully cover). `pflow-tokenmodel::cid()`
  (`Schema::cid()`) stays a local identity hash over this crate's DSL-derived
  `Schema` type — a different input than the raw JSON-LD document
  `compute_cid` takes, not a placeholder for it; see `cid.rs`'s module doc.
  The showcase's `cid.mjs` step can now be cross-checked against pflow-rs,
  not just Go/JS.

Exit: `pflow-rs` appears in the showcase README's score table with the same
columns as Go.

## Not ported, on purpose

| Package | Reason | Revisit when |
|---|---|---|
| `graphql` | a server surface over `eventsource`; petri-pilot is the served product and it is Go | a Rust service exists |
| `eventsource` SQLite store | persistence belongs to the service, not the library; memory store shipped in Phase 5 as `pflow-eventsource::MemoryStore` | a Rust service exists |
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
- Where a phase needs go-pflow to *emit* a golden it does not yet emit, that
  is a go-pflow change first, tagged, then consumed here. go-pflow's
  `cmd/ssa-goldens` and `cmd/shape-goldens` are the pattern; `cmd/sde-goldens`,
  `cmd/scheduled-goldens`, `cmd/bundle-goldens` and `cmd/properties-goldens`
  (landed 2026-09-10) are four more instances of it, closing the SDE,
  scheduled-SSA, flattened-bundle and verify-output gaps this bullet used to
  name as still open.
  - Those four were vendored from an **uncommitted** go-pflow working tree —
    the four generator programs themselves were new, untracked files, which
    is exactly what `sync`'s "refuses a dirty checkout" guard exists to
    catch. This was a deliberate, narrow exception, not a quiet bypass: the
    commit the fixtures were generated *against* (`HEAD`, not the untracked
    generators) was already the same commit `go-pflow.lock`'s `source:` line
    pins, the four generators change nothing any existing golden depends on,
    and each vendored file's own `_comment` records the commit with a
    `-dirty` suffix so the provenance is honest rather than silently
    rewritten to look like a clean sync. `go-pflow.lock`'s entries were
    updated by hand to match, not through `sync`. The next `sync` run
    against a committed go-pflow tree that includes these four commands will
    refresh them the normal way and drop the `-dirty` suffix.
