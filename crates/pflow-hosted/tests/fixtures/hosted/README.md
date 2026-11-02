# Hosted answers

Answers of the hosted server, `https://pflow.xyz/mcp` (unauthenticated JSON-RPC
`tools/call`), captured 2026-11-02. The server was then at pflow-xyz
`origin/main` `5b91835`, which includes `2e57057` (net-shape refusals).

Each file is `<tool>-<fixture>[-<variant>].json`:

```json
{"tool": "pflow_<tool>", "net": "<file under ../nets/>", "arguments": {...},
 "answer": <structuredContent>}            // or "error": "<the refusal text>"
```

`arguments` is everything but `net`. `tests/net_tools.rs`
(`every_hosted_answer_is_reproduced`) replays each file locally and asserts
the local answer equals `answer` (or the local refusal equals `error`), with
these documented exceptions:

- `validate`: go-pflow's validator ranges over Go maps, so the order of issues
  within `errors`, `warnings` and `info` varies from call to call on the hosted
  server; the lists are compared as sets.
- `export-*-svg`: the hosted server draws with pflow-xyz's `internal/svg`
  (applying `layout`); the local answer is pflow-visualization's drawing at the
  net's own coordinates. Only the shape `{format: "svg", svg}` is compared.
- `cid-bundle-solo`: a composition seals its `subnets` list as a JSON-LD set,
  which the local sealer does not implement; the local tool refuses, naming
  the hosted server, instead of answering a different CID.

Not captured, because the hosted answer is not reproducible even on the
hosted server: a `reachable`/`unreachable` target that several reachable
markings match (go-pflow takes the first in Go map order, so the trace and
marking vary between calls).

To refresh: re-run the capture against the live endpoint and check every
change by hand; a difference is a bug in the port unless it is one of the
above.
