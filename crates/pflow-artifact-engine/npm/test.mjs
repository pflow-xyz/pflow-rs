#!/usr/bin/env node
// test.mjs: @pflow-xyz/engine's main file, loaded in Node, against the goldens
// recorded from the engine pflow_artifact embedded inline (gen-goldens.mjs).
//
//   node crates/pflow-artifact-engine/npm/test.mjs <package dir>
//
// Checks: the script defines the global PflowEngine when run as a plain script
// (as a <script src> does), it also loads under require(), it makes no network
// request, the package resolves by name under import (named and default) and
// require, its types compile against both entries, and every recorded
// analyze/fire reply is reproduced exactly. The only field allowed to differ is
// "engine", the build's self-description; the goldens' "amendments" (see
// requests.mjs) are the deliberate departures from the recorded engine.
//
// `make engine-npm-test` runs this against the unpacked `npm pack` tarball, so
// the "files" list is under test too.
import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import { BOUNDED_WHY, requests } from './requests.mjs';

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

// 2b. By package name, as an installed dependency: the "exports" map.
const consumer = mkdtempSync(join(tmpdir(), 'pflow-engine-consumer-'));
process.on('exit', () => rmSync(consumer, { recursive: true, force: true }));
cpSync(pkgDir, join(consumer, 'node_modules', '@pflow-xyz', 'engine'), { recursive: true });
const chain = JSON.stringify(JSON.parse(readFileSync(join(here, 'nets', 'chain.json'), 'utf8')));
function runIn(file, body) {
  writeFileSync(join(consumer, file), body);
  return execFileSync(process.execPath, [file], { cwd: consumer, encoding: 'utf8' }).trim();
}
await check('import { load } from the package (named ESM exports)', () => {
  const out = runIn('named.mjs', `import { load, version, wasmSha256, wasmBytes } from '@pflow-xyz/engine';
const e = await load();
const a = e.analyze({ net: ${chain} });
console.log(JSON.stringify({ version, wasmSha256, same: e.wasmSha256 === wasmSha256, wb: typeof wasmBytes, states: a.state_count }));`);
  const r = JSON.parse(out);
  assert.equal(r.version, pkg.version);
  assert.ok(r.same);
  assert.equal(r.wb, 'function');
  assert.equal(r.states, engine.analyze({ net: JSON.parse(chain) }).state_count);
});
await check('import PflowEngine from the package (default export), no global', () => {
  const out = runIn('default.mjs', `import PflowEngine, { load } from '@pflow-xyz/engine';
const e = await PflowEngine.load();
console.log(JSON.stringify({ same: PflowEngine.load === load, idem: (await load()) === e, global: typeof globalThis.PflowEngine, fire: e.fire({ net: ${chain} }).enabled }));`);
  const r = JSON.parse(out);
  assert.ok(r.same && r.idem);
  assert.equal(r.global, 'undefined', 'the ES module defines no global');
  assert.deepEqual(r.fire, engine.fire({ net: JSON.parse(chain) }).enabled);
});
await check("require('@pflow-xyz/engine') (CommonJS)", () => {
  const out = runIn('req.cjs', `const api = require('@pflow-xyz/engine');
api.load().then((e) => console.log(JSON.stringify({ version: api.version, states: e.analyze({ net: ${chain} }).state_count })));`);
  const r = JSON.parse(out);
  assert.equal(r.version, pkg.version);
  assert.equal(r.states, engine.analyze({ net: JSON.parse(chain) }).state_count);
});
await check('every file package.json names is in the package', () => {
  for (const f of pkg.files) readFileSync(join(pkgDir, f));
  const flat = (v) => (typeof v === 'string' ? [v] : Object.values(v).flatMap(flat));
  for (const f of flat(pkg.exports)) readFileSync(join(pkgDir, f));
});

// 2c. The types, against both entries, under TypeScript's node16 resolution.
// TypeScript is fetched with npx (pinned); set PFLOW_ENGINE_SKIP_TSC=1 offline.
if (process.env.PFLOW_ENGINE_SKIP_TSC) {
  console.log('skip types (PFLOW_ENGINE_SKIP_TSC)');
} else {
  await check('types: named + default import (.d.mts) and require (.d.ts) compile; analyze() is typed', () => {
    writeFileSync(join(consumer, 'package.json'), JSON.stringify({ name: 'consumer', private: true, type: 'module' }));
    writeFileSync(join(consumer, 'esm.ts'), `import PflowEngine, { load, type AnalyzeResult, type LivenessWitness, type Verdict } from '@pflow-xyz/engine';
const e = await load();
const a: AnalyzeResult = e.analyze({ net: {}, max_states: 10 });
const b: Verdict = a.bounded;
const unknown: boolean = a.bounded === 'unknown' && a.complete === false;
const w: LivenessWitness | null = a.witness;
const trace: string[] = w ? w.trace : [];
const k: 'deadlock' | 'livelock' | undefined = w?.kind;
const c: number = a.p_invariants[0].coefficients['p'] + a.t_invariants[0].counts['t'];
const f: string[] = e.fire({ net: {}, transition: 't' }).enabled;
const r = e.raw('analyze', { net: {} });
const err: string | undefined = 'error' in r ? r.error : undefined;
// @ts-expect-error analyze() is typed, not any
const nope: string = a.state_count;
void [b, unknown, trace, k, c, f, err, nope, PflowEngine.wasmSha256];
`);
    writeFileSync(join(consumer, 'cjs.cts'), `import api = require('@pflow-xyz/engine');
api.load().then((e) => { const n: number = e.analyze({ net: {} }).edge_count; void n; });
// @ts-expect-error require() has no default export
void api.default;
`);
    writeFileSync(join(consumer, 'tsconfig.json'), JSON.stringify({
      compilerOptions: { module: 'node16', moduleResolution: 'node16', target: 'es2022', strict: true, noEmit: true, skipLibCheck: false, types: [] },
      files: ['esm.ts', 'cjs.cts'],
    }));
    try {
      execFileSync('npx', ['--yes', '--package=typescript@5.6.3', '--', 'tsc', '-p', '.'], { cwd: consumer, encoding: 'utf8', stdio: 'pipe' });
    } catch (e) { throw new Error(`tsc failed:\n${e.stdout}${e.stderr}`); }
  });
}

// 2d. The u64 reply is read unsigned: a pointer at or above 2 GiB stays positive.
await check('u64 reply (ptr << 32 | len) decodes unsigned', () => {
  const split = (r) => Array.from(sandbox.PflowEngine._split(r));
  assert.deepEqual(split(-1n), [0xffffffff, 0xffffffff]);
  const ptr = 0x80000010, len = 0x7;
  assert.deepEqual(split(BigInt.asIntN(64, (BigInt(ptr) << 32n) | BigInt(len))), [ptr, len]);
  assert.deepEqual(split((1024n << 32n) | 99n), [1024, 99]);
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
    const amended = new Set((g.amendments ?? []).map((a) => a.call));
    for (const a of g.amendments ?? []) {
      const r = g.calls[a.call].reply;
      assert.deepEqual([a.field, a.was, a.now, a.why, r.complete, r.bounded], ['bounded', true, 'unknown', BOUNDED_WHY, false, 'unknown'], `amendment ${JSON.stringify(a)}`);
    }
    g.calls.forEach((c, i) => {
      if (c.fn === 'analyze' && !amended.has(i)) assert.ok(!(c.reply.complete === false && c.reply.bounded === true), `call #${i}: bounded true on an incomplete search, not amended`);
    });
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

// 6. Boundedness is a verdict: unknown when the search stopped at max_states.
await check('bounded: true when complete, false past the token limit, "unknown" at max_states', () => {
  const load = (f) => JSON.parse(readFileSync(join(netsDir, f), 'utf8'));
  const full = engine.analyze({ net: load('weighted.json') });
  assert.equal(full.complete, true);
  assert.equal(full.bounded, true);
  const cut = engine.analyze({ net: load('weighted.json'), max_states: 5 });
  assert.equal(cut.complete, false);
  assert.equal(cut.bounded, 'unknown');
  const src = engine.analyze({ net: load('source.json') });
  assert.equal(src.complete, false);
  assert.equal(src.bounded, false, 'the source net passes the token limit');
  const srcCut = engine.analyze({ net: load('source.json'), max_states: 5 });
  assert.equal(srcCut.bounded, 'unknown', 'stopped at 5 states, before the token limit');
});

if (failures) { console.log(`\n${failures} failed`); process.exit(1); }
console.log(`\nall passed (${pkg.name}@${pkg.version}, ${src.length} B)`);
