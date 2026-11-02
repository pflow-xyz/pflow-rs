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
| `artifact_html` | Write a self-contained interactive page for a loaded net (click-to-fire, ODE/SSA charts, engine embedded, no network) and return its path: publish it as an artifact |
| `ontology_describe`, `ontology_search`, `ontology_classes` | The pflow ontology ([CC0](https://ontology.pflow.xyz)), from a snapshot bundled in the package; `meta` in each reply names its version and commit |
| `chyp_check`, `chyp_part`, `chyp_rule` | String-diagram rewriting and proof checking with [chyp-rs](https://github.com/akissinger/chyp) (an unofficial Rust port of part of Chyp) |

Everything runs offline after install. The ontology is a snapshot: update the package to
refresh it, or use the hosted server for SPARQL.

Nets live in memory for the life of the process. Nets with guards, stages or rate
schedules are refused rather than run as a different net. Needs Node 18+.

Licences: engine MIT, chyp-rs Apache-2.0 (notices in `chyp/`), ontology data CC0.
