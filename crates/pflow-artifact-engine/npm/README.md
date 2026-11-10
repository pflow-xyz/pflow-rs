# @pflow-xyz/engine

pflow-rs's reachability analysis, liveness verdicts with a replayable witness,
P/T-invariants and firing rule, as **one script that embeds its WebAssembly**.
Load it with a `<script>` tag and it defines the global `PflowEngine`; it makes
no network request of its own.

That is the point of the package: some sandboxes (claude.ai artifacts among
them) allow scripts from `cdn.jsdelivr.net/npm`, cdnjs and unpkg but block every
other fetch, including a `.wasm` from those same hosts.

```html
<script src="https://cdn.jsdelivr.net/npm/@pflow-xyz/engine@0.1.0/pflow-engine.js"></script>
<script>
  PflowEngine.load().then((engine) => {
    const a = engine.analyze({ net, max_states: 2000 });
    // a.state_count, a.edge_count, a.deadlock_free, a.live, a.quasi_live,
    // a.witness {kind, trace, marking}, a.dead_markings, a.p_invariants, a.t_invariants
    const s = engine.fire({ net, marking: { p0: 1 }, transition: "t0" });
    // s.marking, s.enabled
  });
</script>
```

`net` is a pflow.xyz JSON-LD net (or pflow-rs's Shape B model). `analyze`
and `fire` throw on an `{"error": ...}` reply; `raw(fn, req)` returns it.
Also loads under `require()`.

Needs WebAssembly compilation (under a CSP, `'wasm-unsafe-eval'`) and
`DecompressionStream` (Chrome 80, Safari 16.4, Firefox 113, Node 18).

`pflow-engine.wasm` is the same module, for hosts that can fetch it. The ABI:
`alloc(len)`, write JSON into `memory`, `analyze(ptr,len)` / `fire(ptr,len)`
return a u64 `(ptr<<32)|len` pointing at the JSON reply, `dealloc(ptr,len)`.

Source: [pflow-rs `crates/pflow-artifact-engine`](https://github.com/pflow-xyz/pflow-rs/tree/main/crates/pflow-artifact-engine).
The tests hold every reply to the goldens of the engine pflow.xyz's
`pflow_artifact` embeds inline. MIT.
