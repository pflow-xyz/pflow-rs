# Changelog

## 0.4.1

Fixes for the five bugs the 0.4.0 test round found (`docs/pflow-mcp-0.4.0-test-report.md`, B1 to B5), each held by a regression test in `test-fixes-041.mjs` and `test-tools.mjs`.

- **B1, a large search no longer poisons the server.** The reachability cycle search (`pflow-reachability`'s `dfs_cycles`) walks an explicit stack instead of recursing once per state, so `max_states` of 15,000 to 20,000 no longer overflows the wasm stack. `reachability`, `validate` and `verify` run in a worker thread with its own engine and take `timeout_ms` (default 10000, max 120000): past it the answer is `unknown` with `timed_out: true`. A WebAssembly trap on the main engine now restarts it from a fresh instance and reloads every loaded net at its initial marking, instead of leaving every later call failing with "memory access out of bounds".
- **B2, `artifact_html` draws the ODE chart.** The page read `series[p].values || series[p]`, which picks `Array.prototype.values` for an ODE series and threw "function is not iterable". Both ODE and SSA now draw one line per place (checked in headless Chrome).
- **B3, the hosted-name tools read both net shapes.** `load_net` stores every net as pflow.xyz JSON-LD, and an array-shape net given inline (`places: [...]`, `arcs: [{from, to}]`) is converted first, so `check`, `cid`, `edit`, `info`, `validate`, `verify`, `invariants`, `incidence`, `export`, `sde` and `what_if` all accept it. A net that cannot be read says which shape it was taken for and why.
- **B4, rate 0 is a real rate.** In `simulate`, `stochastic` and a model's own transition rates, 0 switches the transition off; `null` means unset (the net's rate, else 1). A negative rate is refused.
- **B5, sealing is quadratic, not cubic.** The URDNA2015 canonicaliser behind every CID cloned its identifier issuer at every step of an RDF list's chain of tied blank nodes. It no longer does, and every CID is unchanged (pinned by `crates/pflow-hosted/tests/seal_cost.rs`). A 200-place net seals in about 2.6 s in wasm (53 s before). `cid`, `export` and `edit` run in the worker with `timeout_ms` (default 60000), so they never block the server, and a seal past the limit is stopped with an error saying so. `load_net` still computes the CID eagerly only up to 50 places and 250 nodes plus arcs; above that `cid` is null with `cid_skipped`, and the `cid` tool computes it on demand. `edit` takes `cid: false` to skip the seal.
- Also: an array-shape net's name is optional; a fractional arc weight is refused, naming the arc; `invariants` answers `[]` on a net that has none.

Also new since 0.4.0, from main: `bundle_net`, `bundle_prove`, `bundle_check_proof`, `bundle_parts` and `compose_canvas`.

## 0.4.0

First published version: the engine as a local MCP server, the pflow.xyz/mcp tools offline, ontology snapshot, chyp-rs tools.
