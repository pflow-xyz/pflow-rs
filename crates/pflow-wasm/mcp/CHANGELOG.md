# Changelog

## 0.5.0 (unreleased)

The offline MCP as the adoption path: one command, and Claude publishes the pages as artifacts.

- `artifact_html` takes `views`: `play` (default), `analysis` (invariants, conservation, incidence, reachability verdicts with a replayable witness), `whatif` (two scenarios on one seed, per-place differences) and `proof` (standard properties as proved, refuted or unknown with evidence, Copy proof as text). Every page is self-contained (no network, no external scripts), has light and dark themes, works at phone width, and carries a footer with the CID and versions, the net as JSON-LD (`#pflow-net`) and an editor link.
- `list_examples` and `load_example`: the six public pflow.xyz nets plus the `bundle_example` composition as one net, offline. Four of the six recompute to their published CID; coffee-shop and usdc-bridge do not (the local CID is reported, the published one is kept in `pflow_xyz_cid`).
- `timeout_ms` (default 10000, max 120000) on `reachability`, `validate` and `verify`: the search runs in a worker thread and is stopped at the limit; the answer is `unknown` with `timed_out: true`.
- `load_net` writes every net as JSON-LD, so `info` and `incidence` work on array-shape nets; rate 0 switches a transition off; the eager CID is capped (bug fixes B3, B4, B5 from the 0.4.0 test report).
- The bundled ontology snapshot ships only `core.ttl` and `symbols.ttl` (about 180 KB smaller); a test compares the ontology's pattern entries with `bundle_parts`.
- Server instructions steer Claude to build pages with `artifact_html` and publish them with its own Artifact tool.
- `make wasm-mcp-test` runs every `test-*.mjs`, a corpus check and `npm pack`.

Unchanged limits: guards, stages and rate schedules are refused; a composition's `cid` is refused; no SPARQL.

## 0.4.0

First published version: the engine as a local MCP server, the pflow.xyz/mcp tools offline, ontology snapshot, chyp-rs tools.
