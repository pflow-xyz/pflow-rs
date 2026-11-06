# GitHub presence for pflow-mcp (DRAFT, not applied)

Status: a draft for the owner. Nothing here has been applied, and no `gh` command has been run. Review, edit, then run the commands in the last section yourself. `@pflow-xyz/pflow-mcp` 0.5.0 is not published (npm has 0.4.0); do not announce it until it is.

The message: the offline MCP is the simplest way to adopt pflow. One command, no account, no network, no toolchain. Claude builds the model, the engine runs it, and Claude publishes the interactive page as a Claude artifact you can share. Every page carries the net, its CID and a link to the public editor.

## Repo description (pflow-xyz/pflow-rs)

> Petri nets for Claude in one command: `claude mcp add pflow -- npx -y @pflow-xyz/pflow-mcp`. Offline MCP server, shareable artifacts. Rust engine: ODE, SSA, reachability, ZK.

(Under 350 characters. The current description was not read; check it before replacing.)

## Topics

`petri-net`, `mcp`, `model-context-protocol`, `claude`, `claude-artifacts`, `webassembly`, `rust`, `formal-verification`, `simulation`, `zero-knowledge`

## Pinned README banner

Put this at the top of the root `README.md` (the repo's own, not the package's):

```markdown
> **Try it in one command.** No account, no network, no toolchain:
>
> ```bash
> claude mcp add pflow -- npx -y @pflow-xyz/pflow-mcp
> ```
>
> Ask Claude to model something. It builds the net, the engine checks and simulates it, and
> Claude publishes the interactive page as an artifact you can share. Every page carries the
> net, its CID and a link to the [public editor](https://pflow.xyz).
> [Package README](crates/pflow-wasm/mcp/README.md)
```

If 0.5.0 is not out when this lands, add "(views `analysis`, `whatif`, `proof` and the example gallery arrive in 0.5.0)".

## Release notes draft: pflow-mcp 0.5.0

Title: `pflow-mcp 0.5.0: publish what Claude builds`

> The offline MCP is now the way to adopt pflow. Install is one command and needs no account, no network and no toolchain:
>
> ```bash
> claude mcp add pflow -- npx -y @pflow-xyz/pflow-mcp
> ```
>
> **Shareable pages.** `artifact_html` writes one self-contained HTML file (engine embedded, no network requests, light and dark, phone width) and Claude publishes it as an artifact. Views: `play`, `analysis` (invariants, conservation, incidence, reachability verdicts with a replayable witness), `whatif` (two scenarios on one seed, per-place differences) and `proof` (properties proved, refuted or unknown, with evidence). Every page carries the net as JSON-LD, its CID and an editor link.
>
> **Examples.** `list_examples` and `load_example`: the six public pflow.xyz nets and one composition, offline. Four of the six recompute to their published CID; coffee-shop and usdc-bridge do not, so their pages link with the local CID.
>
> **Bounded searches.** `timeout_ms` on `reachability`, `validate` and `verify` stops a long search and answers `unknown`.
>
> **Fixes** from the 0.4.0 test report (B3, B4, B5), and a smaller ontology snapshot.
>
> **Limits.** Guards, stages and rate schedules are refused, not approximated. A composition's CID is refused locally. No SPARQL (use the hosted server).

## CHANGELOG entry

Added as `crates/pflow-wasm/mcp/CHANGELOG.md` (0.5.0 marked unreleased, 0.4.0 below it). Move the "unreleased" tag when 0.5.0 is published.

## Commands (not run)

Run from any checkout, after reviewing the text above. Replace `<tag>` and confirm the target commit first.

```bash
# description and topics
gh repo edit pflow-xyz/pflow-rs \
  --description 'Petri nets for Claude in one command: claude mcp add pflow -- npx -y @pflow-xyz/pflow-mcp. Offline MCP server, shareable artifacts. Rust engine: ODE, SSA, reachability, ZK.'
gh repo edit pflow-xyz/pflow-rs \
  --add-topic petri-net --add-topic mcp --add-topic model-context-protocol \
  --add-topic claude --add-topic claude-artifacts --add-topic webassembly \
  --add-topic rust --add-topic formal-verification --add-topic simulation \
  --add-topic zero-knowledge

# release, only after 0.5.0 is on npm (check: npm view @pflow-xyz/pflow-mcp version)
gh release create pflow-mcp-v0.5.0 --repo pflow-xyz/pflow-rs \
  --title 'pflow-mcp 0.5.0: publish what Claude builds' \
  --notes-file release-notes-0.5.0.md   # paste the quoted block above into this file first
```

The banner and the CHANGELOG are file edits, landed by a normal commit and PR, not by `gh`. The repo's pinned items on the pflow-xyz organisation page are set in the GitHub UI.
