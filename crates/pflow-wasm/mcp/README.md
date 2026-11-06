# @pflow-xyz/pflow-mcp

**The simplest way to adopt pflow.** One command, no account, no network, no toolchain:

```bash
claude mcp add pflow -- npx -y @pflow-xyz/pflow-mcp
```

Claude builds the model, the engine (the [pflow-rs](https://github.com/pflow-xyz/pflow-rs)
Petri net engine, compiled to WebAssembly and embedded in the package) runs it, and
Claude publishes the result as an interactive page you can share, as a **Claude
artifact**. Every page carries the net, its CID and a link to the public editor at
[pflow.xyz](https://pflow.xyz), so whoever receives it can read the net back out, load it
and carry on.

> **Version note.** `@pflow-xyz/pflow-mcp` 0.4.0 is the latest on npm. Everything marked
> *(0.5.0)* below is on `main` and ships with 0.5.0, which is not published yet.

## Quickstart

1. **Add the server.** Claude Code: the command above. Claude Desktop or any stdio client:

   ```json
   { "mcpServers": { "pflow": { "command": "npx", "args": ["-y", "@pflow-xyz/pflow-mcp"] } } }
   ```

   Needs Node 18+. Nothing else.
2. **Ask for a model.** For example: *"Model a coffee shop with a grinder and two
   baristas, and tell me whether it can deadlock."* Or start from a public net:
   *"Load the producer-consumer example and show me what happens if I double the buffer."*
3. **Publish and share.** Claude calls `artifact_html`, which writes one self-contained
   HTML file and returns its path; Claude then publishes that file with its own Artifact
   tool and hands you the link. The server only writes the local file. It does not host,
   serve or publish anything.

## What the artifacts look like

A page is a single file: the engine is embedded, there are no network requests and no
external scripts, so it runs inside a sandboxed artifact. It has light and dark themes and
is usable at phone width. Every page has a footer (document CID, package, engine and
ontology versions, seed), the net as JSON-LD (`#pflow-net`, with Download net and Copy as
text buttons) and a link to the same net in the public editor.

`artifact_html` takes `views`, a list; Claude picks them from what you asked:

| View | What it shows | Claude picks it when you say |
|---|---|---|
| `play` (default) | The net drawn, click-to-fire token game, ODE and SSA charts | show it, play with it, how does it behave |
| `analysis` *(0.5.0)* | P- and T-invariants, conservation, structural boundedness, the incidence matrix (from the engine), and reachability verdicts with a witness trace you can step through | is it safe, can it deadlock, what is conserved |
| `proof` *(0.5.0)* | Standard properties (deadlock-free, bounded, live, l4-live, terminating, conserves) as proved, refuted or unknown with method and evidence; counterexample traces replayed on the page; a Copy proof as text record | prove it, always holds |
| `whatif` *(0.5.0)* | The net as is beside one changed arm (tokens added or removed on a place, or a transition switched off), same seed, SSA and ODE charts on a shared scale, per-place differences | what if, compare, remove, double |

Views combine on one page (`["play", "analysis", "proof"]`). For a composition of parts
joined by typed links, `compose_canvas` writes the snap-together canvas page, which
re-checks proof records itself with no engine.

Verdicts on a page and in a reply come from the engine, not from Claude's reading of the
net. An `unknown` stays `unknown` and says the search was bounded.

## Examples

*(0.5.0)* `list_examples` and `load_example` load, with no network, the six public
pflow.xyz nets (coffee-shop, tic-tac-toe, enzyme-kinetics, predator-prey,
producer-consumer, usdc-bridge) and the `bundle_example` composition flattened into one
net (`order-flow-composition`). Pages built from them link to the net at pflow.xyz. Caveat:
only 4 of the 6 recompute locally to the CID pflow.xyz publishes them under (coffee-shop
and usdc-bridge do not, cause not found). For those two, `cid` is the local value and the
published one is in `pflow_xyz_cid`; the editor link on a page built from them uses the
local CID and will not resolve at pflow.xyz. The composition's CID is local only.

## Offline behaviour and known limits

Everything runs on your machine after install. The ontology is a snapshot bundled in the
package; update the package to refresh it. Nets live in memory for the life of the process.

- **Refused, not approximated:** nets with guards, stages or rate schedules, and arcs with
  a fractional weight, are refused rather than run as a different net.
- **Compositions:** a composition's `cid` is refused locally (the local sealer does not
  seal a JSON-LD set). `bundle_net` writes it as an ordinary net, which has a CID.
- **No SPARQL.** `ontology_describe`, `ontology_search` and `ontology_classes` read the
  snapshot; for SPARQL use the separate ontology server (ontology.pflow.xyz, its
  `sparql_query` tool), which is not part of pflow.xyz/mcp.
- **Bounded searches:** `reachability`, `validate` and `verify` stop at a state budget and
  say so. `timeout_ms` (default 10000, max 120000) *(0.5.0)* stops a long search in a
  worker thread; the answer is `unknown`, with `timed_out: true`.
- `export`'s SVG is pflow-rs's own drawing. Not ported from the hosted server:
  `rate_scan`, `optimize`, `game_value`, `game_move`, `save`, `get`, `links`, `canvas`.

## Compared with the hosted pflow.xyz tools

| | This package (offline) | Hosted [pflow.xyz/mcp](https://pflow.xyz/mcp) |
|---|---|---|
| Setup | one command | add a remote server; `save` needs a pflow.xyz sign-in |
| Network | none after install | every call |
| Shareable pages | `artifact_html` with the engine embedded, published by Claude | `pflow_artifact` |
| Tool names and answers | the same names (without `pflow_`) and answer shapes, held to captured hosted answers | the reference |
| `rate_scan`, `optimize`, game tools, `save` | no | yes |
| SPARQL over the ontology | no | not in pflow.xyz/mcp: the separate ontology server's `sparql_query` |

The hosted server is a demo of the same tools; this package is the one to adopt. Its
answers are checked against the hosted ones (`make wasm-mcp-crosscheck-tools`).

## Tool reference

| Tool | |
|---|---|
| `load_net` | Load a net (pflow.xyz JSON) under a name |
| `list_nets`, `state` | What is loaded; places, marking, enabled transitions |
| `fire`, `reset` | Token game |
| `simulate` | Mass-action ODE (Tsit5) |
| `stochastic` | Gillespie SSA, byte-exact with go-pflow at the same seed |
| `reachability` | Bounded state-space search, as the hosted pflow.xyz/mcp `pflow_reachability`: state and edge count, bounded, dead markings, and the `deadlock_free`, `quasi_live` (L1) and `live` (L4) verdicts, each `true`, `false` or `"unknown"`, with a witness trace that replays with `fire`. Takes a loaded net or an inline `model`, `max_states` (2000, ≤ 20000), `max_tokens`, and `from` (`initial` or `current`) |
| `artifact_html` | Write a self-contained page for a loaded net and return its path: publish it as an artifact. `views` (default `["play"]`: click-to-fire, ODE/SSA charts) picks what it shows; every page carries a footer (document CID, versions, seed), the net as JSON-LD (`#pflow-net`, Download and Copy buttons) and an editor link; engine embedded, no network |
| `compose_canvas` | Write the snap-together composition canvas as one self-contained page (engine embedded) |
| `list_examples`, `load_example` | *(0.5.0)* The built-in public example nets; `load_example` loads one like `load_net` |
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
| `bundle_net` | The flattened composition written as an ordinary pflow.xyz net, so `simulate`, `reachability`, `verify`, `cid` and the rest read it (and it has a CID) |
| `bundle_prove`, `bundle_check_proof` | Judge what each link does to the parts it joins (conservation laws preserved, extended or changed, with replayable witnesses), and re-check such a record against its bundle by arithmetic and replay |
| `bundle_parts` | The catalog of thirteen parts to snap together: parameters, ports, the conservation law each carries and the joins that are legal |

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
refresh it, or use the ontology server (ontology.pflow.xyz, `sparql_query`) for SPARQL.

Nets live in memory for the life of the process. Nets with guards, stages or rate
schedules are refused rather than run as a different net. Needs Node 18+.

Licences: engine MIT, chyp-rs Apache-2.0 (notices in `chyp/`), ontology data CC0.
