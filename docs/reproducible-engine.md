# The reproducible, CID-checked engine

`crates/pflow-engine` is one wasm32 module that loads a model by its content
address and runs pflow-rs's existing checks and simulations on it. The goal is a
verdict anyone can re-derive from public infrastructure:

- the **model** is named by its CID (URDNA2015 + CIDv1, the one pflow.xyz uses);
- the **engine** is named by its sha256, and the same pflow-rs commit rebuilds
  it bit for bit on any host;
- the **answer** to `(cid, op, args)` carries a `verdict_sha256` that the same
  engine gives on every machine.

Code is MIT. Models and other content are CC0. Everything here builds from
public sources: crates.io, the Rust toolchain from rustup, and the binaryen and
wasm-bindgen releases on GitHub.

## The API

```js
import init, { initSync, Model, cidOf, verify, ops, engineVersion, runCases } from "./pflow_engine.js";
initSync({ module: wasmBytes });                 // check sha256(wasmBytes) against the manifest first

cidOf(modelBytes)                                // the CID pflow.xyz gives these bytes
const m = new Model(modelBytes, expectedCid);    // throws "cid mismatch: …" unless they seal to it
JSON.parse(m.run("ssa", JSON.stringify({ seed: 42, realizations: 8 })));
verify(modelBytes, expectedCid, "verify", JSON.stringify({ properties: [{ kind: "deadlock-free" }] }));
```

`run` returns canonical JSON text:

```json
{"args":{…},"cid":"z4EBG9j…","engine":"pflow-engine","engine_version":"0.3.0",
 "op":"ssa","result":{…},"verdict_sha256":"…"}
```

`verdict_sha256` is the sha256 of the canonical JSON of `{cid, op, args,
result}`: keys sorted at every level, no whitespace, serde_json's numbers. The
engine version is left out of it on purpose, so a rebuilt engine that answers
the same gives the same verdict.

| op | runs | args |
|----|------|------|
| `check`, `validate`, `verify`, `invariants`, `incidence`, `sde` | `pflow_hosted::call`: the pflow.xyz/mcp tools, locally | the hosted tool's arguments, without `net` (the engine supplies the loaded one) |
| `simulate` | `pflow_wasm::Core::simulate_json` (mass-action ODE, Tsit5) | `tEnd`, `maxPoints`, `rates`, `contextual` |
| `ssa` | `Core::stochastic_json` (portable Gillespie SSA) | `seed`, `realizations`, `horizon`, `samples`, `rates` |
| `reachability` | `Core::reachability_json` | `maxStates`, `maxTokens` |
| `fire` | `Core::fire_json` per step | `sequence`: transition ids |

The engine adds no behaviour (the scope of this crate). A refusal is the
underlying engine's, word for word.

## Building it

```bash
scripts/build-wasm-repro.sh <outdir>      # the module, its glue and manifest.json
scripts/check-wasm-repro.sh               # rebuild, compare with crates/pflow-engine/repro.json,
                                          # recheck the recorded verdicts, cargo test -p pflow-engine
scripts/repro-across-hosts.sh [commit] gir workbit
                                          # the same commit, cloned from GitHub, on each host
```

What is pinned:

| What | Pin |
|------|-----|
| rustc | `crates/pflow-engine/rust-toolchain.toml`, exact (`1.99.1`); cargo runs from that directory so rustup uses it, `--locked` against the committed `Cargo.lock` |
| wasm-bindgen | the lockfile's version (`=0.2.129` in the crate); the CLI must equal it |
| wasm-opt | binaryen `version_132`, the GitHub release tarball for the platform, sha256 pinned in the script |
| profile | `[profile.engine-repro]`: opt-level z, fat LTO, codegen-units 1, panic abort, strip, no debuginfo, no incremental |
| paths | `--remap-path-prefix` for the checkout, target dir, `CARGO_HOME` and the sysroot; the module has no host path in it |
| crate ids | `scripts/rustc-host-independent.sh` as `RUSTC_WRAPPER` (below) |
| time | `SOURCE_DATE_EPOCH` = the commit's time; nothing reads the clock |
| build scripts | the engine's tree has build scripts only in generic-array, proc-macro2, quote, serde, serde_core, serde_json, syn, thiserror, wasm-bindgen, wasm-bindgen-shared and zmij: rustc-version and cfg probes, no paths or times embedded. The manifest lists them so a new one shows up in review |

The build starts from a clean target dir every time (cargo's fingerprint does
not cover the wrapper, and a reproducible build should not depend on an earlier
one). It takes under a minute.

The manifest has no host name and no build time, so two hosts that agree write
the same `manifest.json`. `crates/pflow-engine/repro.json` is the recorded one;
`check-wasm-repro.sh` compares files, toolchain, tools and flags with it. A change
that moves the engine's bytes must re-record it (and the verdicts) in the same
commit.

### Proof: gir and workbit

2026-11-23, commit `72bd045`, cloned from GitHub on each host:

| Host | Platform | Engine sha256 |
|------|----------|---------------|
| gir | x86_64 Linux | `db0fc985a2aba582c1e9e3e6a6f18444badc2c9525209ae78eddc7fd8a4bc119` |
| workbit | arm64 macOS | `db0fc985a2aba582c1e9e3e6a6f18444badc2c9525209ae78eddc7fd8a4bc119` |

All four files (`pflow_engine_bg.wasm`, `pflow_engine.js`, the two `.d.ts`)
are identical. The first attempt was not: the two hosts built modules 1.5 KB
apart. Cargo's `-C metadata` hash for a wasm32 crate folds in the hashes of its
host-built dependencies (proc-macros, build scripts), which differ by host
triple. That hash becomes the crate's StableCrateId, which reaches symbol names
and so the order of functions and data in the module. `memchr`, which has no
host-built dependency, had the same hash on both hosts; `serde_json` and every
pflow crate did not. The fix is the wrapper: for wasm32 units it replaces the
hash with one computed from the crate name, its source path relative to its
package, crate type, edition and cfgs. Host units pass through unchanged.

## The example set

`crates/pflow-engine/verdicts/cases.json`: ten public models (pflow-xyz's parity
and native fixtures, the pflow-mcp examples) × check, validate, verify,
invariants, incidence, simulate, ssa, sde and reachability, plus a `fire` and a
wrong-CID case that must stay a refusal. A refusal (the SDE refusing a capacity,
say) is recorded too and must recur word for word.

`verdicts/recorded.json` is what the WASM build answered: the reference. It was
recorded on gir (node 22) and is identical on workbit (node 23). It is checked by:

- `node crates/pflow-engine/js/check-verdicts.mjs <outdir> verdicts/cases.json verdicts/recorded.json`
  for the module (in `check-wasm-repro.sh`; `--record` rewrites it);
- `cargo test -p pflow-engine` for the native engine (also in the GitHub
  workflow's `cargo test --workspace`).

Run `scripts/check-wasm-repro.sh` on gir and workbit as the engine's CI on
Matt's machines, or `scripts/repro-across-hosts.sh` to do both at once.

## Float drift

WASM floats are IEEE-754 deterministic except for NaN bit patterns. NaN cannot
reach a verdict: a JSON number is finite, and serde_json writes any non-finite
`f64` as `null`, so every NaN hashes the same (`nan_hashes_as_null`).

Native and WASM agree bit for bit on every op except the ODE. Measured on the
example set (native on gir and on workbit, WASM under node):

| Case | native = WASM | largest difference |
|------|---------------|--------------------|
| every check, validate, verify, invariants, incidence, reachability, fire | yes | — |
| every ssa, sde | yes | — (portable RNG and `plog`, no libm) |
| coffee-shop/simulate | no | 6.8e-16 absolute (3.2e-13 relative) |
| with-rates/simulate | no | 3.9e-17 absolute (1.5e-9 relative, on a value near 0) |
| enzyme-kinetics/simulate | no | 8.0e-12 absolute (1.1e-12 relative) |
| the other simulate cases | yes | — |

Native gir (glibc) and native workbit (Apple libm) agree with each other on all
92. The cause is `f64::powf` in Tsit5's step-size control
(`crates/pflow-solver/src/ode.rs`, the accept and reject branches): it is the
only libm call on the ODE path. Natively it is the platform's `pow`; in the
module it is Rust's own libm, compiled in. A last-ulp difference in a step
factor moves the step grid, and every later point with it. There is no FMA or
SIMD drift: rustc does not contract `a*b+c` into an FMA on any target, and the
module is built without `simd128` or relaxed-simd.

So **the WASM module is the reference for verification**: it carries its `pow`
with it, so it answers the same under any JS engine on any host. The native
test holds every other op exact and prints, without failing, a `simulate`
case that differs. Making the native ODE match too would mean a portable `pow`
in the solver, the way SSA has `plog`. That changes the native engine's output
and is outside this crate.

## Publishing

`scripts/publish-engine-cdn.sh <outdir>` publishes a build to
`cdn.stackdump.com/ipfs/<CID>/` the way pflow-native publishes its wasm
(`publish-wasm.sh`, through pflow-xyz's `publish/cdn.py`). It refuses a build
whose files differ from `repro.json` or that came from a dirty tree. The CID is
`bafyrei` + the first 32 hex of the `.wasm`'s sha256 (the house rule), so
db0fc985… is published as `bafyreidb0fc985a2aba582c1e9e3e6a6f18444`. The
manifest is served as `meta/manifest.json`; it sits in a subdirectory because
`cdn.py` takes a top-level `.json` as the primary artifact, and the CID must
name the engine. A page pins the URL and the `.wasm` sha256 and refuses other
bytes, so the CDN is a transport and is never trusted for content (pflow-xyz's
`vendor/pflow-native-wasm/cdn.js` is the pattern).
