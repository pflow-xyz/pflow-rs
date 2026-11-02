# @pflow-xyz/pflow-mcp

The [pflow-rs](https://github.com/pflow-xyz/pflow-rs) Petri net engine as a local
[MCP](https://modelcontextprotocol.io) server. The engine is WebAssembly embedded in
the package: one command, no Rust toolchain, no network after install.

```bash
npx -y @pflow-xyz/pflow-mcp
```

Claude Code:

```bash
claude mcp add pflow -- npx -y @pflow-xyz/pflow-mcp
```

Claude Desktop / any stdio client:

```json
{ "mcpServers": { "pflow": { "command": "npx", "args": ["-y", "@pflow-xyz/pflow-mcp"] } } }
```

## Tools

| Tool | |
|---|---|
| `load_net` | Load a net (pflow.xyz JSON) under a name |
| `list_nets`, `state` | What is loaded; places, marking, enabled transitions |
| `fire`, `reset` | Token game |
| `simulate` | Mass-action ODE (Tsit5) |
| `stochastic` | Gillespie SSA, byte-exact with go-pflow at the same seed |
| `reachability` | Bounded state-space search, as the hosted pflow.xyz/mcp `pflow_reachability`: state and edge count, bounded, dead markings, and the `deadlock_free`, `quasi_live` (L1) and `live` (L4) verdicts, each `true`, `false` or `"unknown"`, with a witness trace that replays with `fire`. Takes a loaded net or an inline `model`, `max_states` (2000, ≤ 20000), `max_tokens`, and `from` (`initial` or `current`) |
| `artifact_html` | Write a self-contained interactive page for a loaded net (click-to-fire, ODE/SSA charts, engine embedded, no network) and return its path: publish it as an artifact |
| `ontology_describe`, `ontology_search`, `ontology_classes` | The pflow ontology ([CC0](https://ontology.pflow.xyz)), from a snapshot bundled in the package; `meta` in each reply names its version and commit |
| `chyp_check`, `chyp_part`, `chyp_rule` | String-diagram rewriting and proof checking with [chyp-rs](https://github.com/akissinger/chyp) (an unofficial Rust port of part of Chyp) |

`reachability` gives the hosted tool's answer: its verdicts are held to go-pflow's
on sample nets offline (`cargo test -p pflow-wasm --test reachability_parity`) and
live against pflow.xyz/mcp (`make wasm-mcp-crosscheck`). A verdict is never `true`
when `complete` is `false`: the search stopped at its budget (`truncated` says
which), and only a witness makes a verdict `false`.

Everything runs offline after install. The ontology is a snapshot: update the package to
refresh it, or use the hosted server for SPARQL.

Nets live in memory for the life of the process. Nets with guards, stages or rate
schedules are refused rather than run as a different net. Needs Node 18+.

Licences: engine MIT, chyp-rs Apache-2.0 (notices in `chyp/`), ontology data CC0.
