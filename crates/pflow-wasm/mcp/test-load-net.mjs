// load_net end to end (B3-B7 of docs/pflow-mcp-0.4.0-test-report.md): an array-shape
// net is readable by every hosted-name tool, rates 0/null, CID cap, edit cid:false.
//   node crates/pflow-wasm/mcp/test-load-net.mjs <built pflow-mcp dir>
import assert from 'node:assert/strict';
import { copyFileSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const { Client } = await import(join(dir, 'node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js'));
const { StdioClientTransport } = await import(join(dir, 'node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js'));
const client = new Client({ name: 'test-load-net', version: '0' });
await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')] }));
const call = async (name, args = {}) => {
  const r = await client.callTool({ name, arguments: args });
  const text = r.content[0].text;
  let json; try { json = JSON.parse(text); } catch { /* text */ }
  return { error: !!r.isError, text, json };
};
const ok = async (name, args) => { const r = await call(name, args); assert.ok(!r.error, `${name}: ${r.text.slice(0, 300)}`); return r.json; };

// the array shape, no name, no layout, with a read and an inhibitor arc
const mutex = {
  places: [{ id: 'idle', initial: 2 }, { id: 'crit' }, { id: 'lock', initial: 1 }, { id: 'ban' }],
  transitions: [{ id: 'enter' }, { id: 'leave' }],
  arcs: [{ from: 'idle', to: 'enter' }, { from: 'lock', to: 'enter' }, { from: 'enter', to: 'crit' },
    { from: 'crit', to: 'leave' }, { from: 'leave', to: 'idle' }, { from: 'leave', to: 'lock' },
    { from: 'ban', to: 'enter', type: 'inhibitor' }],
};
let r0;
const loaded = await ok('load_net', { model: mutex, name: 'mutex' });
assert.equal(loaded.layout, 'synthesized');
assert.match(loaded.cid, /^z4EBG9j/);
assert.match(loaded.cid_note, /layout/);
const again = await ok('load_net', { model: mutex, name: 'mutex2' });
assert.notEqual(again.cid, undefined);
for (const tool of ['check', 'info', 'incidence', 'invariants', 'validate', 'export', 'cid']) await ok(tool, { net: 'mutex' });
await ok('verify', { net: 'mutex', properties: [{ kind: 'deadlock-free' }] });
r0 = await call('sde', { net: 'mutex' }); // refused (inhibitor) with a reason, not a parse error
assert.ok(r0.error && !/missing field/.test(r0.text), r0.text);
assert.equal((await ok('cid', { net: 'mutex' })).cid, loaded.cid);
assert.equal((await ok('state', { net: 'mutex' })).info.name, 'mutex');
assert.ok((await ok('invariants', { net: 'mutex' })).p_invariants.some((i) => i.equation === 'crit + lock == 1'));
assert.deepEqual((await ok('invariants', { net: 'mutex', kind: 'p' })).t_invariants, undefined);

// B7: fractional weight is refused with the arc named, by load_net and by the tools
let r = await call('load_net', { model: { name: 'f', places: [{ id: 'a' }], transitions: [{ id: 't' }], arcs: [{ from: 'a', to: 't', weight: 0.5 }] } });
assert.ok(r.error && r.text.includes('arcs[0]') && r.text.includes('0.5'), r.text);
r = await call('invariants', { net: { '@context': 'https://pflow.xyz/schema', places: { a: {} }, transitions: { t: {} }, arcs: [{ source: 'a', target: 't', weight: [0.5] }] } });
assert.ok(r.error && r.text.includes('arcs[0]'), r.text);
// B7: no p-invariant is [], not null
await ok('load_net', { model: { name: 'src', places: [{ id: 'a' }], transitions: [{ id: 't' }], arcs: [{ from: 't', to: 'a' }] } });
assert.deepEqual((await ok('invariants', { net: 'src' })).p_invariants, []);

// B4: 0 is off, null is unset
await ok('load_net', { name: 'drain', model: { places: [{ id: 'a', initial: 5 }, { id: 'b' }], transitions: [{ id: 'd', rate: 0 }, { id: 'e', rate: 0.5 }], arcs: [{ from: 'a', to: 'd' }, { from: 'd', to: 'b' }, { from: 'a', to: 'e' }, { from: 'e', to: 'b' }] } });
let s = await ok('stochastic', { net: 'drain', horizon: 100, samples: 2, rates: { e: 0 } });
assert.equal(s.final.a, 5);
s = await ok('stochastic', { net: 'drain', horizon: 100, samples: 2, rates: { e: null } });
assert.equal(s.final.a, 0);
s = await ok('simulate', { net: 'drain', tEnd: 50, maxPoints: 2, rates: { e: 0 } });
assert.equal(s.final.a, 5);

// B5: no eager CID above 50 places; on demand it is computed (and then cached)
const places = Array.from({ length: 60 }, (_, i) => ({ id: `p${i}`, initial: i === 0 ? 1 : 0 }));
const transitions = Array.from({ length: 59 }, (_, i) => ({ id: `t${i}` }));
const arcs = transitions.flatMap((t, i) => [{ from: `p${i}`, to: t.id }, { from: t.id, to: `p${i + 1}` }]);
const big = await ok('load_net', { model: { name: 'chain60', places, transitions, arcs } });
assert.equal(big.cid, null);
assert.match(big.cid_skipped, /cubic/);
const t0 = Date.now();
assert.match((await ok('cid', { net: 'chain60' })).cid, /^z4EBG9j/);
console.log(`   cid of 60 places on demand: ${Date.now() - t0} ms`);

// few places, many transitions: the guard counts the whole graph, so load_net does not stall in sealing
{
  const wt = Array.from({ length: 300 }, (_, i) => ({ id: `t${i}` }));
  const wa = wt.flatMap((t, i) => [{ from: `q${i % 5}`, to: t.id }, { from: t.id, to: `q${(i + 1) % 5}` }]);
  const wp = Array.from({ length: 5 }, (_, i) => ({ id: `q${i}`, initial: i === 0 ? 1 : 0 }));
  const t1 = Date.now();
  const wide = await ok('load_net', { model: { name: 'wide', places: wp, transitions: wt, arcs: wa } });
  assert.equal(wide.cid, null);
  assert.match(wide.cid_skipped, /cubic/);
  assert.ok(Date.now() - t1 < 5000, 'load_net of a wide net must not seal eagerly');
}

// edit cid:false
const e1 = await ok('edit', { net: 'mutex', ops: [{ op: 'add_place', id: 'x' }], cid: false });
assert.equal(e1.cid, undefined);
assert.match((await ok('edit', { net: 'mutex', ops: [{ op: 'add_place', id: 'x' }] })).cid, /^z4EBG9j/);

// what cannot be written as a pflow.xyz net still loads, and says why
const d = await ok('load_net', { model: { name: 'dat', places: [{ id: 'a' }, { id: 'm', kind: 'data' }], transitions: [], arcs: [] } });
assert.match(d.unsupported, /"m"/);
r = await call('check', { net: 'dat' });
assert.ok(r.error && r.text.includes('"m"'), r.text);

await client.close();
console.log('pflow-mcp load_net: ok');
