#!/usr/bin/env node
// build.mjs: assemble @pflow-xyz/engine from the optimized module. No dependencies.
// `make engine-npm` runs cargo and wasm-opt first.
//
//   node crates/pflow-artifact-engine/npm/build.mjs <optimized .wasm> <package dir>
//
// Package layout:
//   pflow-engine.js   the main file: a UMD/IIFE script that embeds the module
//                     (gzip, then base64) and defines the global PflowEngine
//                     (and module.exports under CommonJS). It makes no request.
//   pflow-engine.mjs  the same engine as an ES module: named exports (load,
//                     wasmBytes, version, wasmSha256) and a default, no global
//                     (Node cannot see the names a UMD assigns, so `import
//                     { load }` from the .js fails); package.json "exports"
//                     sends import here and require to pflow-engine.js
//   pflow-engine.wasm the same module, for hosts that can fetch it
//   pflow-engine.d.ts (require) and .d.mts (import), README.md, LICENSE, package.json
//
// Why a script and not a .wasm: a claude.ai artifact may load scripts from
// cdn.jsdelivr.net/npm, cdnjs and unpkg, but its connect-src blocks every other
// fetch, including a .wasm from those same hosts. So the module has to arrive
// inside a script.
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const [wasmPath, outDir] = process.argv.slice(2);
if (!wasmPath || !outDir) {
  console.error('usage: build.mjs <optimized .wasm> <package dir>');
  process.exit(2);
}
const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..', '..');

// The package has its own version, independent of the cargo workspace's.
export const VERSION = '0.2.0';

const wasm = readFileSync(wasmPath);
const sha256 = createHash('sha256').update(wasm).digest('hex');
const b64 = gzipSync(wasm, { level: 9 }).toString('base64');

// The engine itself, as the body of a factory function returning the API.
// Both entry points below wrap this same text.
const factory = `function () {
  'use strict';
  var WASM_GZ_B64 = '${b64}';
  var loading = null;

  function bytesOf(b64) {
    var s = atob(b64), n = s.length, u = new Uint8Array(n);
    for (var i = 0; i < n; i++) u[i] = s.charCodeAt(i);
    return u;
  }
  function gunzip(u) {
    var ds = new DecompressionStream('gzip');
    return new Response(new Blob([u]).stream().pipeThrough(ds)).arrayBuffer();
  }
  // A u64 export arrives as a signed BigInt; read it unsigned, or a reply
  // pointer at or above 2 GiB would come out negative.
  function split(r) {
    var u = BigInt.asUintN(64, BigInt(r));
    return [Number(u >> BigInt(32)), Number(u & BigInt(0xffffffff))];
  }
  function wrap(instance) {
    var x = instance.exports, enc = new TextEncoder(), dec = new TextDecoder();
    function raw(fn, req) {
      var b = enc.encode(JSON.stringify(req)), p = x.alloc(b.length);
      new Uint8Array(x.memory.buffer, p, b.length).set(b);
      var pl = split(x[fn](p, b.length)), rp = pl[0], rl = pl[1];
      var out = JSON.parse(dec.decode(new Uint8Array(x.memory.buffer, rp, rl)));
      x.dealloc(rp, rl); x.dealloc(p, b.length);
      return out;
    }
    function call(fn, req) {
      var out = raw(fn, req);
      if (out && out.error) throw new Error(out.error);
      return out;
    }
    return {
      version: '${VERSION}',
      wasmSha256: '${sha256}',
      /** Reply as the module gave it, {"error": ...} included. */
      raw: raw,
      /** Throws on an {"error": ...} reply. */
      call: call,
      analyze: function (req) { return call('analyze', req); },
      fire: function (req) { return call('fire', req); },
    };
  }
  /** The module's bytes (Promise<ArrayBuffer>). */
  function wasmBytes() { return gunzip(bytesOf(WASM_GZ_B64)); }
  /** Promise of the engine; compiles once, safe to call again. */
  function load() {
    if (!loading) {
      loading = wasmBytes()
        .then(function (buf) { return WebAssembly.instantiate(buf, {}); })
        .then(function (r) { return wrap(r.instance); });
      loading.catch(function () { loading = null; });
    }
    return loading;
  }
  return { version: '${VERSION}', wasmSha256: '${sha256}', load: load, wasmBytes: wasmBytes, _split: split };
}`;

const banner = (how) => `/*! @pflow-xyz/engine ${VERSION} | MIT | https://github.com/pflow-xyz/pflow-rs
 * pflow-rs reachability, liveness verdicts with a witness, P/T-invariants and
 * firing, as a zero-import WebAssembly module embedded in this script.
 * wasm sha256 ${sha256}
 *
${how}
 *
 * Needs WebAssembly compilation ('wasm-unsafe-eval' under a CSP) and
 * DecompressionStream (Chrome 80, Safari 16.4, Firefox 113, Node 18).
 */
`;

// Classic script + CommonJS: <script src> defines the global; require() gets module.exports.
const script = banner(` *   const engine = await PflowEngine.load();     // or require('@pflow-xyz/engine').load()
 *   engine.analyze({ net, max_states: 2000 });   // {state_count, edge_count, deadlock_free, ...}
 *   engine.fire({ net, marking, transition });   // {marking, enabled}`) + `(function (root, factory) {
  var api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  if (root) root.PflowEngine = api;
})(typeof globalThis !== 'undefined' ? globalThis : typeof self !== 'undefined' ? self : this, ${factory});
`;

// ES module: named exports and a default, no global. Node's ESM loader cannot
// see names a UMD assigns at run time, so import needs a file of its own.
const esm = banner(` *   import { load } from '@pflow-xyz/engine';     // or: import PflowEngine from ...
 *   const engine = await load();
 *   engine.analyze({ net, max_states: 2000 });`) + `const api = (${factory})();
export const version = api.version;
export const wasmSha256 = api.wasmSha256;
export const load = api.load;
export const wasmBytes = api.wasmBytes;
export default api;
`;

const types = `export type Verdict = boolean | 'unknown';
export type Marking = Record<string, number>;

export interface AnalyzeRequest {
  /** A pflow.xyz JSON-LD net, or a pflow-rs Shape B model. */
  net: object;
  /** Stop exploring after this many states (default 2000). */
  max_states?: number;
}
export interface FireRequest {
  net: object;
  /** Overrides on the initial marking. */
  marking?: Marking;
  /** Fire this transition first; omitted, the reply is the marking as given. */
  transition?: string;
}

/** Refutes deadlock_free (kind "deadlock") or live (kind "livelock"). */
export interface LivenessWitness {
  kind: 'deadlock' | 'livelock';
  /** Shortest explored firing sequence from the initial marking to \`marking\`. */
  trace: string[];
  /** The dead marking, or the first marking of the closed region. */
  marking: Marking;
  /** Markings in the closed region; 1 for a dead marking. */
  region_size: number;
  /** Transitions that never fire again once \`marking\` is reached. */
  never_fires: string[];
}
export interface PInvariant {
  places: string[];
  coefficients: Record<string, number>;
  /** The weighted token sum, constant over every reachable marking. */
  value: number;
  render: string;
}
export interface TInvariant {
  transitions: string[];
  counts: Record<string, number>;
  render: string;
}
export interface AnalyzeResult {
  engine: string;
  state_count: number;
  edge_count: number;
  /** The whole state space was explored within max_states. */
  complete: boolean;
  /** false: a marking passed the token limit; "unknown": stopped at max_states first. */
  bounded: Verdict;
  max_tokens: Record<string, number>;
  deadlock_free: Verdict;
  live: Verdict;
  quasi_live: Verdict;
  /** Transitions shown not to be L4-live. */
  not_live: string[];
  witness: LivenessWitness | null;
  dead_markings: Marking[];
  p_invariants: PInvariant[];
  p_truncated: boolean;
  t_invariants: TInvariant[];
  t_truncated: boolean;
}
export interface FireResult { marking: Marking; enabled: string[] }
export interface ErrorReply { error: string }

export interface Engine {
  version: string;
  wasmSha256: string;
  /** The reply as the module gave it, {error} included. */
  raw(fn: 'analyze', req: AnalyzeRequest): AnalyzeResult | ErrorReply;
  raw(fn: 'fire', req: FireRequest): FireResult | ErrorReply;
  /** Throws on an {error} reply. */
  call(fn: 'analyze', req: AnalyzeRequest): AnalyzeResult;
  call(fn: 'fire', req: FireRequest): FireResult;
  analyze(req: AnalyzeRequest): AnalyzeResult;
  fire(req: FireRequest): FireResult;
}
export interface PflowEngineApi {
  version: string;
  wasmSha256: string;
  /** Compiles the embedded module once; safe to call again. */
  load(): Promise<Engine>;
  /** The embedded module's bytes. */
  wasmBytes(): Promise<ArrayBuffer>;
}
`;

// require() / <script>: module.exports is the API object; no default export.
const dts = types + `
export declare const version: string;
export declare const wasmSha256: string;
export declare function load(): Promise<Engine>;
export declare function wasmBytes(): Promise<ArrayBuffer>;
declare global { var PflowEngine: PflowEngineApi }
`;

// import: named exports and a default.
const dmts = types + `
export declare const version: string;
export declare const wasmSha256: string;
export declare function load(): Promise<Engine>;
export declare function wasmBytes(): Promise<ArrayBuffer>;
declare const PflowEngine: PflowEngineApi;
export default PflowEngine;
`;

const pkg = {
  name: '@pflow-xyz/engine',
  version: VERSION,
  description: 'pflow-rs reachability, liveness, invariants and firing as one script that embeds its WebAssembly: a global PflowEngine, no fetch.',
  license: 'MIT',
  homepage: 'https://pflow.xyz',
  repository: { type: 'git', url: 'git+https://github.com/pflow-xyz/pflow-rs.git', directory: 'crates/pflow-artifact-engine' },
  main: 'pflow-engine.js',
  module: 'pflow-engine.mjs',
  browser: 'pflow-engine.js',
  jsdelivr: 'pflow-engine.js',
  unpkg: 'pflow-engine.js',
  types: 'pflow-engine.d.ts',
  exports: {
    '.': {
      import: { types: './pflow-engine.d.mts', default: './pflow-engine.mjs' },
      require: { types: './pflow-engine.d.ts', default: './pflow-engine.js' },
      default: './pflow-engine.js',
    },
    './pflow-engine.js': './pflow-engine.js',
    './pflow-engine.mjs': './pflow-engine.mjs',
    './pflow-engine.wasm': './pflow-engine.wasm',
    './package.json': './package.json',
  },
  files: ['pflow-engine.js', 'pflow-engine.mjs', 'pflow-engine.wasm', 'pflow-engine.d.ts', 'pflow-engine.d.mts', 'README.md', 'LICENSE'],
  keywords: ['petri-net', 'reachability', 'webassembly', 'pflow'],
  sideEffects: true,
};

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, 'pflow-engine.js'), script);
copyFileSync(wasmPath, join(outDir, 'pflow-engine.wasm'));
writeFileSync(join(outDir, 'pflow-engine.mjs'), esm);
writeFileSync(join(outDir, 'pflow-engine.d.ts'), dts);
writeFileSync(join(outDir, 'pflow-engine.d.mts'), dmts);
writeFileSync(join(outDir, 'package.json'), JSON.stringify(pkg, null, 2) + '\n');
copyFileSync(join(here, 'README.md'), join(outDir, 'README.md'));
copyFileSync(join(repo, 'LICENSE'), join(outDir, 'LICENSE'));
console.log(`${outDir}: pflow-engine.js ${script.length} B, pflow-engine.mjs ${esm.length} B, wasm ${wasm.length} B, sha256 ${sha256}`);
