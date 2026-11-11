# @pflow-xyz/engine

pflow-rs's reachability analysis, liveness verdicts with a replayable witness,
P/T-invariants and firing rule, as **one script that embeds its WebAssembly**.
Load it with a `<script>` tag and it defines the global `PflowEngine`; it makes
no network request of its own.

That is the point of the package: some sandboxes (claude.ai artifacts among
them) allow scripts from `cdn.jsdelivr.net/npm`, cdnjs and unpkg but block every
other fetch, including a `.wasm` from those same hosts.

```html
<script src="https://cdn.jsdelivr.net/npm/@pflow-xyz/engine@0.2.0/pflow-engine.js"></script>
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

As a dependency (`npm i @pflow-xyz/engine`), the `exports` map sends `import`
to an ES module with named exports and a default, and `require` to the same
script; both carry types (`AnalyzeResult`, `LivenessWitness`, `Verdict`, ...):

```js
import { load } from '@pflow-xyz/engine';        // or: import PflowEngine from '@pflow-xyz/engine'
const { load } = require('@pflow-xyz/engine');   // CommonJS
```

The ES module defines no global; the script does.

`net` is a pflow.xyz JSON-LD net (or pflow-rs's Shape B model). `analyze`
and `fire` throw on an `{"error": ...}` reply; `raw(fn, req)` returns it.

`deadlock_free`, `live`, `quasi_live` and `bounded` are `true`, `false` or
`"unknown"`. A search that stopped at `max_states` (`complete: false`) has not
seen every marking, so a claim about all of them is `"unknown"`; `bounded` is
`false` as soon as a marking passes the token limit. (0.1.0 reported `bounded:
true` for a search cut short by `max_states`; since 0.2.0 that is `"unknown"`.)

Needs WebAssembly compilation (under a CSP, `'wasm-unsafe-eval'`) and
`DecompressionStream` (Chrome 80, Safari 16.4, Firefox 113, Node 18).

`pflow-engine.wasm` is the same module, for hosts that can fetch it. The ABI:
`alloc(len)`, write JSON into `memory`, `analyze(ptr,len)` / `fire(ptr,len)`
return a u64 `(ptr<<32)|len` pointing at the JSON reply, `dealloc(ptr,len)`.

## Check it yourself

Source: [pflow-rs `crates/pflow-artifact-engine`](https://github.com/pflow-xyz/pflow-rs/tree/main/crates/pflow-artifact-engine).
`make engine-npm-test` in a pflow-rs checkout builds the package and holds every
reply to the goldens of the engine pflow.xyz's `pflow_artifact` embeds inline
(`npm/goldens/`, over the nets in `npm/nets/`). A verdict on a page that loads
this script can therefore be re-run: take the net from the page, call `analyze`,
and replay any witness trace with `fire`. Byte-identical rebuilds of the
published `pflow-engine.js` are not claimed.

## License

MIT.
