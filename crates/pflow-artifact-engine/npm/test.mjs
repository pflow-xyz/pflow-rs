#!/usr/bin/env node
// test.mjs: @pflow-xyz/engine's main file, loaded in Node, against the goldens
// recorded from the engine pflow_artifact embedded inline (gen-goldens.mjs).
//
//   node crates/pflow-artifact-engine/npm/test.mjs <package dir>
//
// Checks: the script defines the global PflowEngine when run as a plain script
// (as a <script src> does), it also loads under require(), it makes no network
// request, and every recorded analyze/fire reply is reproduced exactly. The
// only field allowed to differ is "engine", the build's self-description.
import { readFileSync, readdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import { requests } from './requests.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const pkgDir = resolve(process.argv[2] ?? join(here, '..', '..', '..', 'target', 'npm', 'engine'));
const pkg = JSON.parse(readFileSync(join(pkgDir, 'package.json'), 'utf8'));
const mainFile = join(pkgDir, pkg.main);
const src = readFileSync(mainFile, 'utf8');
let failures = 0;
function check(name, fn) {
  return Promise.resolve().then(fn).then(
    () => console.log(`ok   ${name}`),
    (e) => { failures++; console.log(`FAIL ${name}\n     ${String(e.message).split('\n').join('\n     ')}`); },
  );
}

// 1. As a classic script in a fresh global with no fetch: a <script src> on a page.
const sandbox = {
  atob, Blob, Response, DecompressionStream, TextEncoder, TextDecoder, WebAssembly, BigInt, Uint8Array, JSON, Number, Promise,
  fetch: () => { throw new Error('the engine script must not fetch'); },
  XMLHttpRequest: function () { throw new Error('the engine script must not use XHR'); },
};
sandbox.globalThis = sandbox; sandbox.self = sandbox; sandbox.window = sandbox;
vm.createContext(sandbox);
vm.runInContext(src, sandbox, { filename: pkg.main });
let engine;
await check('script defines global PflowEngine with load()', async () => {
  assert.equal(typeof sandbox.PflowEngine, 'object');
  assert.equal(typeof sandbox.PflowEngine.load, 'function');
  assert.equal(sandbox.PflowEngine.version, pkg.version);
  engine = await sandbox.PflowEngine.load();
  assert.equal(typeof engine.analyze, 'function');
  assert.equal(typeof engine.fire, 'function');
  assert.equal(await sandbox.PflowEngine.load(), engine, 'load() is idempotent');
});
if (!engine) { console.log('\nengine did not load'); process.exit(1); }

// 2. Under CommonJS require().
await check('require() exports the same API', async () => {
  const req = createRequire(import.meta.url);
  const api = req(mainFile);
  assert.equal(typeof api.load, 'function');
  assert.equal(api.version, pkg.version);
  const e = await api.load();
  assert.equal(e.wasmSha256, api.wasmSha256);
});

// 3. The embedded module is the shipped pflow-engine.wasm.
await check('embedded module equals pflow-engine.wasm', async () => {
  const embedded = Buffer.from(await sandbox.PflowEngine.wasmBytes());
  assert.ok(embedded.equals(readFileSync(join(pkgDir, 'pflow-engine.wasm'))));
});

// 4. Goldens from the inline engine.
const strip = (r) => {
  if (r && typeof r === 'object' && 'engine' in r) { const { engine: _e, ...rest } = r; return rest; }
  return r;
};
const netsDir = join(here, 'nets');
const nets = readdirSync(join(here, 'goldens')).filter((f) => f.endsWith('.json')).sort();
assert.ok(nets.includes('philosophers3.json'), 'the deadlocking 3-philosopher net is among the goldens');
for (const f of nets) {
  await check(`golden ${f}`, () => {
    const g = JSON.parse(readFileSync(join(here, 'goldens', f), 'utf8'));
    const net = JSON.parse(readFileSync(join(netsDir, f), 'utf8'));
    const calls = requests(net);
    assert.equal(calls.length, g.calls.length, 'same calls as recorded');
    calls.forEach(([fn, req], i) => {
      const got = engine.raw(fn, req), want = g.calls[i].reply;
      assert.equal(g.calls[i].fn, fn);
      if (want.engine) assert.match(got.engine, /^pflow-rs pflow-reachability \(wasm32/);
      assert.deepStrictEqual(strip(got), strip(want), `${fn} #${i} ${JSON.stringify({ ...req, net: undefined })}`);
    });
  });
}

// 5. The headline numbers of the reference page, spelled out.
await check('3 philosophers: 14 states, 27 edges, deadlock takeL0 → takeL1 → takeL2', () => {
  const net = JSON.parse(readFileSync(join(netsDir, 'philosophers3.json'), 'utf8'));
  const a = engine.analyze({ net });
  assert.equal(a.state_count, 14);
  assert.equal(a.edge_count, 27);
  assert.equal(a.deadlock_free, false);
  assert.deepEqual(a.witness.trace, ['takeL0', 'takeL1', 'takeL2']);
  assert.equal(a.p_invariants.length, 6);
  let m;
  for (const t of a.witness.trace) m = engine.fire({ net, marking: m, transition: t }).marking;
  assert.deepEqual(engine.fire({ net, marking: m }).enabled, []);
  assert.throws(() => engine.fire({ net, transition: 'nope' }));
});

if (failures) { console.log(`\n${failures} failed`); process.exit(1); }
console.log(`\nall passed (${pkg.name}@${pkg.version}, ${src.length} B)`);
