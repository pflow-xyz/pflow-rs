# Snap-together nets: composites as nets, provable glue, and an in-browser canvas

Design and status. Branch `val/run-520d030a` (local, not pushed). Written 2026-11-03.

## The problem

pflow.xyz/mcp has a composition canvas and `pflow_bundle_*` tools: parts (subnets with ports), typed links (token, data, event, guard), and a flatten that turns the lot into one net. Four gaps:

1. A flattened bundle is **not a pflow.xyz net**. `pflow_check` refuses it ("this looks like the flat net pflow_bundle_flatten returns, which the net tools cannot read"), so fire, simulate, stochastic, reachability, invariants and verify never see a composite.
2. A link is judged only by its **ports** (schema, direction, kind, net type). Nothing asks what the glue does to the nets it joins.
3. The parts catalog is six instances of three patterns (queue, pool, saga); the limit is 32 subnets.
4. The canvas needs a server.

## What now exists

| Goal | What | Where |
|---|---|---|
| 1. Composite is a net | `bundle_net`: the flat net in the editor's JSON-LD (+ provenance). Every net tool reads it; its CID computes. | `pflow-hosted/src/bundle/flatnet.rs` |
| 2. Glue is proved | `bundle_prove` (per-link verdicts, composite claims, a proof record) and `bundle_check_proof` | `pflow-hosted/src/bundle/proof.rs` |
| 3. Browser assembly | one HTML file, engine embedded, no network; 13-part catalog | `pflow-wasm/compose/`, `pflow-hosted/src/bundle/parts.rs` |
| 4. Chyp | not done (see below) | |

Tools are in pflow-rs (`pflow-hosted`, so also in the wasm `hosted` build and `@pflow-xyz/pflow-mcp` 0.4.1). Their Go ports and the hosted registration are described under [go-pflow and pflow-xyz](#go-pflow-and-pflow-xyz).

### 1. A composite as an ordinary net

`bundle_net({bundle})` returns `{ok, net, members, places, transitions, arcs, notes}`.

- `net` is the editor's JSON-LD: places and transitions keyed by id, `source`/`target` arcs, one colour. It is the inverse of `net::model_of`, the reader the net tools use: a read arc is a transition→place arc with `inhibitTransition`, an inhibitor a place→transition one, capacity is the place's capacity, and an arc with no weight is written weight 1 (every firing rule reads it as 1; the net reader would drop a zero-weight arc).
- One id changes: `:` is written `~`. The fused transitions are called `fused:a+b`, which JSON-LD reads as a compact IRI, and the CID would be refused.
- Provenance (`members`: which `<subnet>/<element>` each flat element came from, and the declared constraints) is returned beside the net, not in it, so the CID depends on the net alone.
- What the editor's format cannot say is **refused, naming it** (`ok:false`, `unrepresentable[]`): a transition guard, a data place. A net that ran differently from the composite would be worse than none.

Held by: `flatnet.rs` unit tests (places, markings, capacities, arcs and a 40-step walk are identical between the flat model and what the net reader makes of the net, with read arcs and inhibitors in play) and `tests/bundle_net.rs` (check, info, invariants, incidence, validate, verify, cid and export all read the composite; its incidence matrix equals `bundle_incidence`'s cell for cell).

### 2. Glue consistency and the proof record

`bundle_prove({bundle, max_states=2000, max_tokens, ablate=true})` returns a `CompositionProof` record. What it decides, and how:

**Part laws.** Each part brings conservation laws: its declared constraints (`tokens("a") + tokens("b") == n`) and the minimal P-invariants of the part alone (Farkas). Each law y is lifted onto the composite's places and tried against the composite's incidence matrix C:

| status | meaning | certificate |
|---|---|---|
| `preserved` | y·C = 0 still | the check |
| `extended` | y·C ≠ 0, but the composite conserves a larger law z that equals y (up to a ratio) on every place holding a member of the part | z: z·C = 0, z ≥ 0 |
| `changed` | the composite moves it; no larger law accounts for it | the residual: exactly which transitions have y·C ≠ 0 |

A *declared* law is also tried on every reachable marking: `proved` (the search finished, none violates it), `refuted` (a replayable trace to a violating marking), `unknown` (the search stopped).

**Composite claims**, each `proved`, `refuted` or `unknown`. Nothing is `proved` by a search that did not finish.

- `bounded`: every place held by a conservation law (z = sum of the minimal invariants, bound ⌊z·M₀/z[p]⌋) or a capacity; else the exhaustive search.
- `deadlock_free`: exhaustive; `refuted` carries the shortest trace and the dead marking.
- `quasi_live`: every transition fires somewhere reachable.

**Per-link verdicts**, by ablation: flatten without the link, search again, compare. A deadlock, unbounded growth or blocked transition that exists with the link and not without is the link's.

| verdict | when |
|---|---|
| `fail` | the link is illegal; or a declared law is false in a reachable marking and no larger law accounts for it; or the link introduces a deadlock neither joined part would reach by finishing, or unboundedness |
| `warn` | a declared law is false as stated but the composite conserves the larger sum (the token link shares the place, so each part's sum is only part of the whole); or the link blocks transitions that fired without it |
| `explained` | derived laws that continue as parts of larger ones; a restrictive guard; a dead end brought by a part that finishes by itself (a saga) |
| `ok` | no finding |

Overall: `inconsistent` (any fail) · `needs_attention` (any warn) · `consistent_with_notes` · `consistent`, or `inconclusive` when the search stopped with open questions.

**Checking.** `bundle_check_proof({bundle, proof})` has two layers:

1. every certificate by arithmetic or replay on the flat net: y·C and z·C, z ≥ 0, bounds, the counterexample and the deadlock witness fired step by step to the stated marking; the composite's CID;
2. the engine's own answers (the exhaustive searches, every link verdict, the overall verdict) recomputed and compared.

`crates/pflow-wasm/compose/verify.mjs` is layer 1 again in 150 lines of JS with no engine: a reader who trusts neither the Rust nor the wasm needs only `bundle_net`'s net and the record. What neither layer re-derives: the flattening itself (held to go-pflow's goldens) and the firing rule. Claims that rest on a *finished* search (`proved` deadlock-freedom) are re-run by the engine, not by the JS.

Held by `tests/bundle_net.rs`: the default composite is consistent and its record checks; two fused queues are `needs_attention` with a counterexample that replays; a bucket sharing tokens with a pool is `fail`; a saga's dead end is `explained`; shared accounts are explained, not failed; **tampered records do not check** (a law relabelled preserved, a trace that does not replay, a bound on a non-invariant, a record for another composition, a witness that stops short); a search cut at 5 states proves nothing.

**What is judgment, not engine output.** The engine decides the statuses and certificates. The *severity tiers* (that a refuted-but-extended law is `warn` and an unexplained one `fail`; that a finishing part explains a dead end) are this module's judgment, written down in the table above and in `proof.rs`. The plain-language sentences are generated from the record by fixed templates, not by a model.

### 3. The browser canvas

`make wasm-compose` builds `target/compose-canvas.html` (~1.0 MB): the hosted engine (wasm embedded), `verify.mjs` and the page, inlined. No import, no fetch. Publish it as an artifact, or `compose_canvas` in `@pflow-xyz/pflow-mcp` writes it out.

- **Parts palette**: 13 parts (below); click to add; per-part parameters.
- **Canvas**: drag a card by its title; drag from a port and the ports it can legally join light up (the rest dim, hover says why); drop to link; click a link to remove. A list-based join (pick a port, pick a legal partner) does the same without a pointer; ports are focusable.
- **What the joins do**: the verdict banner, the three composite claims, one card per link with its findings in plain sentences, the parts' own laws.
- **Check the proof yourself**: runs both checkers and says what each trusts.
- **Play the composite**: the flat net loads in the engine's `Net`; fire transitions; "Show the steps" replays the dead-end witness.
- **Copy net JSON**: the composite as an ordinary net, for the editor or any pflow tool.

Held by `crates/pflow-wasm/compose/test.mjs` (the wasm build answers `bundle_net`/`bundle_prove`/`bundle_parts` **exactly** as the native goldens; the JS checker accepts every golden record and rejects tampered ones; the composite fires in `Net`) and `e2e.mjs` (the built page in Chrome under a sandbox CSP, `script-src 'unsafe-inline' 'wasm-unsafe-eval'`, `connect-src 'none'`: presets change the verdict, a link is dragged between two ports with legal ports lit and illegal ones dimmed, both re-checks pass, the composite fires, a link is removed, list-based joining works, the witness replays, zero console errors). `make wasm-compose-test` runs both.

#### The catalog (`bundle_parts`)

13 parts; `bundle_parts({})` lists parameters, ports (direction, schema), the law each carries and its legal joins. Every suggested join is held by a test that judges it with the link rules (`every_suggested_join_is_legal`), and every join proves and re-checks (`the_catalog_lists_every_part_and_its_joins_prove_and_recheck`).

`queue`, `resource-pool`, `saga` (as `bundle_example`, same constructors) and ten new ones: `mutex`, `token_bucket`, `circuit_breaker`, `batcher`, `approval`, `inventory`, `failover`, `sir`, `kanban`, `traffic_light`. They are textbook nets written for this catalog from public descriptions (a lock, a token bucket, a breaker, an aggregator, a decision step, stock control, repairable machines, Kermack-McKendrick, a WIP limit, a cyclic controller). **sim.pflow.xyz's models were the inspiration for the kind of part** (operational building blocks with a capacity, a gate or a flow), **not a source**: sim-pflow-xyz is a private repo and nothing was copied from it. Sizes, which place is a port and which schema a port speaks are this catalog's judgment (`why` says what each is for). Port schemas decide legality: `resource` joins a pool, lock, bucket and machines; `queue:items` joins a queue, batcher, approval desk and kanban stage. The net-type matrix still bites: a `WorkflowNet` cannot token-link a `ResourceNet`, so `kanban` and `approval` are resource nets.

What the catalog teaches (all in the tests):

- queue → batcher over a token link: the queue's `items + slots == 3` is false as stated (the batcher eats `size` items at a time) and the composite conserves a larger law: `needs_attention`, explained.
- pool + failover over a token link: the pool's law is false because machines fail: explained by `failover/fail`, `failover/repair`.
- token bucket + pool: a source and a sink sharing the pool's place: nothing accounts for the broken law: `fail`.
- queue + saga over an event link: the composite finishes where the queue alone cannot: `explained` by the saga.

### Limits (stated, not fixed)

- **Subnet limit 32** is unchanged: it is `bundle_example`'s hosted limit, held by goldens, and the new tools reuse the reader. Raising it means a local-limits option on the reader; the browser has no 60 KiB transport cut, so it could be much higher.
- **Search bounds.** Verdicts that rest on a search are exact only when it finishes (default 2000 states, at most 20000). Past that they are `unknown`; the page says so.
- **Ablation** costs a search per link and is skipped past 60 links. It judges one link at a time: two links that only deadlock together are not attributed to either.
- **Guards and data places** do not flatten into an editor net (refused); they do flatten (and fire) in `bundle_run`.
- **Laws are linear** (sums of tokens). A guard-opaque or nonlinear property is not a law here.
- **Derived laws** (Farkas) are capped at 24 per part in the record.

### Goal 4, Chyp: not done

chyp-rs is in the ecosystem (typed wires; `rule`, `refl`, `simp`; the Laws view on pflow.xyz uses it). A composite-meets-spec claim could be a theory with one generator per part (boundary = its port schemas as wire types) and the composite as a composed term, with the spec an equation proved by rewriting. Two problems made me stop short of building it: the rules a proof needs (what a link does to the diagram) would be *assumptions* the engine's record supplies, so the Chyp step would check a derivation from them, not the claim; and chyp-rs's own docs say its typed wires are what rule out ill-typed composites, which is what the port-schema check already does. The honest version is a view that shows the wiring as a typed diagram and re-checks "these parts compose" and its negative controls; I left it for when there is a spec worth stating. The record and `verify.mjs` are the proof mechanism here.

## go-pflow and pflow-xyz

Local branches `val/run-520d030a` in both repos, nothing pushed.

- **go-pflow** `composite/` (commits 68eba0f, 894c7cf): `Call(tool, args)` and typed `Net`, `Prove`, `CheckProof`, `Parts`. The three existing patterns come from go-pflow's own constructors; the ten new parts are written from `parts.rs`; search, Farkas bases and laws run on go-pflow's `reachability`. Held to the pflow-rs goldens (`composite/testdata/composite.json`, pflow-rs 16f69e0, sha256 `8bf9ee18…d934`). **Parity is exact**: catalog, bundles, nets and every prove record (traces, `extended_by`, `says` text) are equal in all 9 cases; nothing was relaxed. Cross-checked both ways: Go's checker accepts every Rust record, and the Rust checker (`FOREIGN_RECORDS`) accepts the records Go writes. `go test ./composite` passes (also under Bazel).
- **pflow-xyz** (b82f6c2, plus 3d2886e, a TEMP `replace` to the local go-pflow that must be dropped before merge: bump the go-pflow pin to a release containing `composite/` instead, or b82f6c2 alone does not build): `pflow_bundle_net`, `pflow_bundle_prove`, `pflow_bundle_check_proof`, `pflow_bundle_parts` registered beside the other `pflow_bundle_*` tools (`cmd/webserver/mcp_model_composite.go`). Tests load the net in check/info/invariants/incidence/validate/cid/export/verify/reachability; the record's cid equals `pflow_cid` of the net (sealed with json-gold) and its states equal `pflow_reachability`'s. Two things differ for the hosted transport: `bundle_net` refuses a net over the 60 KiB result cap rather than cut it, and `bundle_prove` answers with a summary (`record_omitted`) when its record would not fit, since a cut record cannot be checked. `go test ./cmd/webserver/...` passes except two tests that run `git push` and are refused by this run's push hook.
- Differences from the Rust reference: Go's `bundle_prove` refuses a flat net with a guard or data place (go-pflow's reachability does not evaluate guards, so the answer would be about another net); Rust analyses it and sets `net_available:false`.
- **Fixed in Rust after the Go port reported it, not yet in Go**: the proof's incidence now reads a weightless arc as weight 1 (it fires as 1; the hosted `bundle_incidence` keeps raw weights), so a law is no longer "preserved" by an arc that moves a token (a lone subnet with weightless arcs); `bundle_check_proof` now also requires the record to be complete (a deleted law or claim fails); a repeated place id keeps its first row. The goldens did not change. The Go port needs the same three edits before the next golden refresh.
- Not touched: `public/llms.txt` / `llms-full.txt` in pflow-xyz still say "the flat net is not a pflow.xyz net".

## Running it

```bash
cargo test -p pflow-hosted                      # incl. composite goldens (tests/fixtures/composite/)
make wasm-compose-test                          # builds the engine + page; goldens, JS checker, Chrome e2e
make wasm-mcp-test                              # @pflow-xyz/pflow-mcp 0.4.1 (needs CHYP_PKG, ONTOLOGY_DIR in a worktree)
```

Regenerating the composite goldens (a reviewed change to the tools, never to make a test pass): `REGEN_COMPOSITE=1 cargo test -p pflow-hosted --test composite_goldens`.
