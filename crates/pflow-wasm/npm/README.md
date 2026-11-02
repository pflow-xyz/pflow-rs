# @pflow-xyz/pflow-wasm

The [pflow-rs](https://github.com/pflow-xyz/pflow-rs) Petri net engine as
WebAssembly: load a net, fire it, run the mass-action ODE, and run the portable
Gillespie SSA, which is byte-exact with go-pflow and pflow.xyz at the same seed.

The default entry is one self-contained ES module with the `.wasm` embedded in it
(gzipped, then base64). Importing it and calling `init()` makes no further network
request, so it works where a page may load scripts from a CDN but may not `fetch()`
from one. It needs `DecompressionStream`: Chrome 80, Safari 16.4, Firefox 113,
Node 18, or later.

## Quick start

This is a complete page. Paste it into an HTML file or a sandboxed artifact.

```html
<pre id="out"></pre>
<script type="module">
  import init, { Net, version } from "https://cdn.jsdelivr.net/npm/@pflow-xyz/pflow-wasm@__VERSION__/pflow-wasm.inline.js";

  await init();

  const net = new Net({
    name: "coffee",
    places: [
      { id: "beans", initial: 3 },
      { id: "cups" },
    ],
    transitions: [{ id: "brew", rate: 0.5 }],
    arcs: [
      { from: "beans", to: "brew" },
      { from: "brew", to: "cups" },
    ],
  });

  const lines = [`pflow-wasm ${version()}`];
  lines.push(`enabled: ${net.enabled()}`);
  lines.push(`after brew: ${JSON.stringify(net.fire("brew"))}`);

  const ode = net.simulate({ tEnd: 10, maxPoints: 20 });
  lines.push(`ODE at t=10: ${JSON.stringify(ode.final)}`);

  const ssa = net.stochastic({ horizon: 10, samples: 11, realizations: 100, seed: 42 });
  lines.push(`SSA mean at t=10: ${JSON.stringify(ssa.final)}`);

  document.getElementById("out").textContent = lines.join("\n");
  console.log(lines.join("\n"));
</script>
```

The `.wasm` still has to compile, so a page's Content-Security-Policy must allow
that (`script-src 'wasm-unsafe-eval'`) even though nothing is fetched.

## Entries

| Import | File | Loads the `.wasm` from |
|---|---|---|
| `@pflow-xyz/pflow-wasm` | `pflow-wasm.inline.js` | Itself. No request. |
| `@pflow-xyz/pflow-wasm/split` | `pflow-wasm.js` | `pflow-wasm_bg.wasm` beside it, by `fetch` |

Use the split entry in a normal web app, where a separately cached `.wasm` is
better and `fetch` is allowed. It is the same module:

```js
import init, { Net } from "https://cdn.jsdelivr.net/npm/@pflow-xyz/pflow-wasm@__VERSION__/pflow-wasm.js";
await init(); // fetches pflow-wasm_bg.wasm from the same directory
```

Either entry's `init` also takes the module itself, from anywhere:

```js
await init("https://example.com/pflow-wasm_bg.wasm");  // a URL: string or URL object
await init(bytes);                                      // Uint8Array or ArrayBuffer
await init(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)));  // your own base64
```

`initSync(bytes)` does the same without a promise, from bytes you already hold.
The embedded module takes `await init()`, because decompressing it is async.

## API

`init(source?)` must finish before anything else is called.

| Export | Returns | |
|---|---|---|
| `version()` | `string` | The package version |
| `new Net(model)` | `Net` | Shape A (pflow.xyz) or Shape B JSON, as text or an object |
| `net.places()` | `string[]` | Token place ids, in declaration order |
| `net.transitions()` | `string[]` | Transition ids, in declaration order |
| `net.marking()` | `{place: n}` | The current marking |
| `net.enabled()` | `string[]` | Transitions that can fire now |
| `net.fire(id)` | `{place: n}` | Fires one transition and returns the new marking |
| `net.reset()` | | Back to the initial marking |
| `net.simulate(options?)` | `{method, places, times, series, final, gating}` | Mass-action ODE (Tsit5) |
| `net.stochastic(options?)` | `{method, seed, places, times, series, final}` | Portable Gillespie SSA |
| `net.reachability(options?)` | `{states, complete, deadlock_free, live, quasi_live, witness, ...}` | Bounded state-space search with liveness verdicts (not in 0.3.0) |

`simulate` and `stochastic` start from the current marking and do not move it.

`simulate` options: `tEnd` (100), `maxPoints` (200), `rates`, and `contextual`,
which says what to do with read and inhibitor arcs: `"refuse"` (the default),
`"selfloop"` or `"drop"`. `gating` in the result lists constraints the continuous
solution could not honour, such as a capacity.

`stochastic` options: `horizon` (10), `samples` (101), `realizations` (1), `seed`
(1), `rates`. Each place's series is `{values, stddev}`, with `stddev` present when
`realizations > 1`.

`reachability` options: `maxStates` (2000, at most 20000), `maxTokens` (1000, the
per-place count past which the net is called unbounded), and `from`: `"initial"`
(the default) or `"current"`. It ignores rates and does not move the marking. The
result is pflow.xyz/mcp `pflow_reachability`'s, key for key, and is held to
go-pflow's on sample nets (`tests/reachability_parity.rs`): `deadlock_free`,
`live` (L4) and `quasi_live` (L1) are each `true`, `false` or `"unknown"`, never
`true` unless `complete`; `witness` is a firing sequence that replays from `from`
to a dead marking or into a closed region, with `never_fires` naming the
transitions it shuts out; `truncated` and `bound` say where a search stopped.

Errors throw: a transition that is not enabled (with the reason), an unknown
option, a rate for a transition the net does not have.

### What it refuses

A net with guards, stages or rate schedules throws at `new Net`. The firing rule
does not evaluate guards and the portable SSA has neither of the others, so such a
net would otherwise run as a different net with no error. S-expression DSL input
is not in this package; send JSON.

### Long runs

Every call runs on the thread that made it. A large SSA ensemble on the page's main
thread blocks the page, so run it in a Worker.

## License

MIT
