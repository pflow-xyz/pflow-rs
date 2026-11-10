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
//   pflow-engine.wasm the same module, for hosts that can fetch it
//   pflow-engine.d.ts, README.md, LICENSE, package.json
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
export const VERSION = '0.1.0';

const wasm = readFileSync(wasmPath);
const sha256 = createHash('sha256').update(wasm).digest('hex');
const b64 = gzipSync(wasm, { level: 9 }).toString('base64');

const script = `/*! @pflow-xyz/engine ${VERSION} | MIT | https://github.com/pflow-xyz/pflow-rs
 * pflow-rs reachability, liveness verdicts with a witness, P/T-invariants and
 * firing, as a zero-import WebAssembly module embedded in this script.
 * wasm sha256 ${sha256}
 *
 *   const engine = await PflowEngine.load();
 *   engine.analyze({ net, max_states: 2000 });   // {state_count, edge_count, deadlock_free, ...}
 *   engine.fire({ net, marking, transition });   // {marking, enabled}
 *
 * Needs WebAssembly compilation ('wasm-unsafe-eval' under a CSP) and
 * DecompressionStream (Chrome 80, Safari 16.4, Firefox 113, Node 18).
 */
(function (root, factory) {
  var api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  if (root) root.PflowEngine = api;
})(typeof globalThis !== 'undefined' ? globalThis : typeof self !== 'undefined' ? self : this, function () {
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
  function wrap(instance) {
    var x = instance.exports, enc = new TextEncoder(), dec = new TextDecoder();
    function raw(fn, req) {
      var b = enc.encode(JSON.stringify(req)), p = x.alloc(b.length);
      new Uint8Array(x.memory.buffer, p, b.length).set(b);
      var r = x[fn](p, b.length), rp = Number(r >> BigInt(32)), rl = Number(r & BigInt(0xffffffff));
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
  return { version: '${VERSION}', wasmSha256: '${sha256}', load: load, wasmBytes: wasmBytes };
});
`;

const dts = `export interface AnalyzeRequest { net: object; max_states?: number }
export interface FireRequest { net: object; marking?: Record<string, number>; transition?: string }
export interface Engine {
  version: string;
  wasmSha256: string;
  raw(fn: 'analyze' | 'fire', req: object): any;
  call(fn: 'analyze' | 'fire', req: object): any;
  analyze(req: AnalyzeRequest): any;
  fire(req: FireRequest): { marking: Record<string, number>; enabled: string[] };
}
export declare const version: string;
export declare const wasmSha256: string;
export declare function load(): Promise<Engine>;
export declare function wasmBytes(): Promise<ArrayBuffer>;
declare global { var PflowEngine: { version: string; wasmSha256: string; load(): Promise<Engine>; wasmBytes(): Promise<ArrayBuffer> } }
`;

const pkg = {
  name: '@pflow-xyz/engine',
  version: VERSION,
  description: 'pflow-rs reachability, liveness, invariants and firing as one script that embeds its WebAssembly: a global PflowEngine, no fetch.',
  license: 'MIT',
  homepage: 'https://pflow.xyz',
  repository: { type: 'git', url: 'git+https://github.com/pflow-xyz/pflow-rs.git', directory: 'crates/pflow-artifact-engine' },
  main: 'pflow-engine.js',
  browser: 'pflow-engine.js',
  jsdelivr: 'pflow-engine.js',
  unpkg: 'pflow-engine.js',
  types: 'pflow-engine.d.ts',
  files: ['pflow-engine.js', 'pflow-engine.wasm', 'pflow-engine.d.ts', 'README.md', 'LICENSE'],
  keywords: ['petri-net', 'reachability', 'webassembly', 'pflow'],
  sideEffects: true,
};

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, 'pflow-engine.js'), script);
copyFileSync(wasmPath, join(outDir, 'pflow-engine.wasm'));
writeFileSync(join(outDir, 'pflow-engine.d.ts'), dts);
writeFileSync(join(outDir, 'package.json'), JSON.stringify(pkg, null, 2) + '\n');
copyFileSync(join(here, 'README.md'), join(outDir, 'README.md'));
copyFileSync(join(repo, 'LICENSE'), join(outDir, 'LICENSE'));
console.log(`${outDir}: pflow-engine.js ${script.length} B, wasm ${wasm.length} B, sha256 ${sha256}`);
