#!/usr/bin/env node
// test.mjs: the composition page's logic against the real engine.
//
//   node crates/pflow-wasm/compose/test.mjs <hosted engine dir>   (default target/npm/pflow-wasm-hosted)
//
// - the wasm build answers bundle_net / bundle_prove exactly as the native
//   goldens (tests/fixtures/composite/composite.json) say;
// - verify.mjs, which uses no engine, accepts every golden record and
//   rejects tampered ones;
// - the flat net loads in the wasm Net class and fires like the composite.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { verifyRecord } from './verify.mjs';

const dir = resolve(process.argv[2] ?? 'target/npm/pflow-wasm-hosted');
const { default: init, hosted, Net } = await import(pathToFileURL(join(dir, 'pflow-wasm.inline.js')));
await init();
const golden = JSON.parse(readFileSync(new URL('../../pflow-hosted/tests/fixtures/composite/composite.json', import.meta.url), 'utf8'));
const norm = (v) => JSON.parse(JSON.stringify(v));
let n = 0;
const ok = (name) => { n++; console.log('ok', name); };

assert.deepEqual(norm(hosted('bundle_parts', {})), golden.catalog);
ok('catalog equals the native golden');

for (const c of golden.cases) {
  const net = norm(hosted('bundle_net', { bundle: c.bundle }));
  assert.deepEqual(net, c.net, `${c.name}: bundle_net`);
  const proof = norm(hosted('bundle_prove', { bundle: c.bundle }));
  assert.deepEqual(proof, c.prove, `${c.name}: bundle_prove`);
  const engine = norm(hosted('bundle_check_proof', { bundle: c.bundle, proof }));
  assert.equal(engine.valid, true, `${c.name}: engine check ${JSON.stringify(engine.failed)}`);
  const js = verifyRecord(net.net, net.members, proof);
  assert.equal(js.valid, true, `${c.name}: js check ${JSON.stringify(js.checks.filter((x) => !x.ok))}`);
  assert.ok(js.checks.length >= 3);
  ok(`${c.name}: wasm = native; engine and independent JS checker accept the record (${js.checks.length} checks, ${js.skipped.length} left to the engine)`);
}

// tampering: the JS checker has to notice
{
  const c = golden.cases.find((x) => x.name === 'queue-chain');
  const net = norm(hosted('bundle_net', { bundle: c.bundle }));
  const t = (edit) => { const r = structuredClone(c.prove); edit(r); return verifyRecord(net.net, net.members, r); };
  assert.equal(t((r) => { r.laws[0].status = 'preserved'; r.laws[0].residual = {}; }).valid, false);
  assert.equal(t((r) => { r.laws[0].counterexample.trace = []; }).valid, false);
  assert.equal(t((r) => { r.claims[0].certificate.z[Object.keys(r.claims[0].certificate.z)[0]] += 3; }).valid, false);
  const d = golden.cases.find((x) => x.name === 'default');
  const dn = norm(hosted('bundle_net', { bundle: d.bundle }));
  const r = structuredClone(d.prove);
  r.claims.find((x) => x.id === 'deadlock_free').certificate.trace = [];
  assert.equal(verifyRecord(dn.net, dn.members, r).valid, false);
  ok('tampered records are rejected');
}

// the composite plays: the flat net loads in Net and fires
{
  const c = golden.cases.find((x) => x.name === 'default');
  const net = norm(hosted('bundle_net', { bundle: c.bundle }));
  const g = new Net(net.net);
  const witness = c.prove.claims.find((x) => x.id === 'deadlock_free').certificate;
  for (const t of witness.trace) g.fire(t.replace(/:/g, '~'));
  assert.deepEqual(Object.fromEntries(Object.entries(g.marking()).filter(([, v]) => v)), witness.marking);
  assert.deepEqual(g.enabled(), []);
  const r = g.reachability({ maxStates: 2000 });
  assert.equal(r.deadlock_free, false);
  ok('the composite fires in Net: the deadlock witness replays to a dead marking');
}
console.log(`${n} groups passed`);
