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
| `artifact_html` | Write a self-contained page for a loaded net and return its path: publish it as an artifact. `views` (default `["play"]`: click-to-fire, ODE/SSA charts) picks what it shows; every page carries a footer (document CID, versions, seed), the net as JSON-LD (`#pflow-net`, Download and Copy buttons) and an editor link; engine embedded, no network |
| `ontology_describe`, `ontology_search`, `ontology_classes` | The pflow ontology ([CC0](https://ontology.pflow.xyz)), from a snapshot bundled in the package; `meta` in each reply names its version and commit |
| `chyp_check`, `chyp_part`, `chyp_rule` | String-diagram rewriting and proof checking with [chyp-rs](https://github.com/akissinger/chyp) (an unofficial Rust port of part of Chyp) |

### The pflow.xyz/mcp tools, offline

These carry the hosted [pflow.xyz/mcp](https://pflow.xyz/mcp) tools' names (without
`pflow_`), arguments, refusals and answer shapes, so a call written for the hosted
server works here unchanged. Where a tool takes `net` (a pflow.xyz JSON-LD object),
this server also takes the name of a net loaded with `load_net`, and with no `net`
uses the most recently loaded one (`edit` excepted: no net means an empty one).

| Tool | |
|---|---|
| `check`, `info` | Parse a net and report its size; summarise it (places, transitions, arcs, colours, initial marking, enabled). `state` includes the `info` summary for a JSON-LD net |
| `cid` | The CID a net seals to (URDNA2015, sha256, CIDv1), byte-identical to pflow.xyz |
| `edit` | Build or edit a net with an ordered list of ops, all or none; returns the new net and its CID |
| `export` | The net as normalised JSON-LD with its CID, or as SVG |
| `incidence`, `invariants` | The incidence matrix; minimal P- and T-invariants (Farkas), conservation and structural boundedness |
| `validate`, `verify` | Structural checks (optionally with reachability); properties proved, refuted or unknown, with evidence |
| `sde` | Chemical Langevin SDE, seeded, the same doubles as pflow-native's `sim.sde` |
| `what_if` | One place's tokens changed, both arms on one seed through the scenario SSA |
| `bundle_example`, `bundle_validate`, `bundle_add_link`, `bundle_candidates`, `bundle_flatten`, `bundle_run`, `bundle_incidence` | Compositions (PetriNetBundle): the parts catalog, link checks and candidates, flattening into one net, firing a script on it, its incidence matrix and constraint checks |

How they are held to the hosted answers: `sde`, `what_if` and the `bundle_*` tools
replay pflow.xyz's pflow-native goldens exactly (186 cases); `edit` and the
single-net tools replay answers captured from pflow.xyz/mcp (`cargo test -p
pflow-hosted`); `make wasm-mcp-crosscheck-tools` compares them live. Known
differences: a composition's `cid` is refused locally (and `export` then has no
`cid`), as the local sealer does not seal a JSON-LD set; `export`'s SVG is pflow-rs's
own drawing (`layout` is accepted, not applied); refusals of an unreadable net name
pflow-rs, not go-pflow; the hosted 60 KB result truncation and `what_if`'s mid-run
step cap are not applied. Not ported: `simulate`/`ssa`/`fire` (the local `simulate`,
`stochastic` and `fire` cover them on loaded nets), `rate_scan`, `optimize`,
`game_value`, `game_move`, `save`, `get`, `links`, `canvas`.

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
