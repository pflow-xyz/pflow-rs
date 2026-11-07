// Regression tests for the 0.4.1 fixes (docs/pflow-mcp-0.4.0-test-report.md B1-B5):
//   B1  a wasm trap no longer poisons the server: the engine is reinstantiated
//       and loaded nets reloaded (driven by the PFLOW_MCP_TEST_HOOKS trap hook,
//       and by the report's own repro, repeated)
//   B3  every hosted-name tool that takes `net` reads an inline array-shape net,
//       and a net in neither shape gets an error that says what was expected
//   B4  rate 0 is a real rate: the transition never fires (simulate, stochastic,
//       and a model transition's own rate)
//   B5  sealing is quadratic, not cubic: cid of a 200-place chain within budget,
//       and a seal past timeout_ms is stopped with a clear error while the server
//       keeps answering
//   node crates/pflow-wasm/mcp/test-fixes-041.mjs <built pflow-mcp dir>
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const here = dirname(fileURLToPath(import.meta.url));
const { Client } = await import(join(dir, 'node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js'));
const { StdioClientTransport } = await import(join(dir, 'node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js'));
const client = new Client({ name: 'test-fixes-041', version: '0' });
await client.connect(new StdioClientTransport({
  command: process.execPath, args: [join(dir, 'server.mjs')],
  env: { ...process.env, PFLOW_MCP_TEST_HOOKS: '1' },
}));
const call = async (name, args = {}) => {
  const r = await client.callTool({ name, arguments: args }, undefined, { timeout: 180000 });
  const text = r.content[0].text;
  let json; try { json = JSON.parse(text); } catch { /* text */ }
  return { error: !!r.isError, text, json };
};
const ok = async (name, args) => { const r = await call(name, args); assert.ok(!r.error, `${name}: ${r.text.slice(0, 300)}`); return r.json; };
let passed = 0;
const step = (what) => { passed += 1; console.log(`ok  ${what}`); };

const tiny = { name: 'tiny', places: [{ id: 'a', initial: 1 }, { id: 'b' }], transitions: [{ id: 't' }], arcs: [{ from: 'a', to: 't' }, { from: 't', to: 'b' }] };
const chain = (n) => ({
  name: `chain${n}`,
  places: Array.from({ length: n }, (_, i) => ({ id: `p${i}`, initial: i === 0 ? 1 : 0 })),
  transitions: Array.from({ length: n - 1 }, (_, i) => ({ id: `t${i}` })),
  arcs: Array.from({ length: n - 1 }, (_, i) => [{ from: `p${i}`, to: `t${i}` }, { from: `t${i}`, to: `p${i + 1}` }]).flat(),
});

// ---- B1: trap recovery ------------------------------------------------------
{
  await ok('load_net', { model: tiny, name: 'keep' });
  await ok('fire', { net: 'keep', transition: 't' });
  const r = await call('__test_trap');
  assert.ok(r.error, 'the trap hook answers an error');
  assert.match(r.text, /engine trapped/);
  assert.match(r.text, /restarted/);
  const l = await ok('list_nets');
  assert.equal(l.engine_restarts, 1);
  assert.ok(l.nets.includes('keep'), 'loaded nets survive the restart');
  const s = await ok('state', { net: 'keep' });
  assert.deepEqual(s.marking, { a: 1, b: 0 }, 'a reloaded net is back at its initial marking');
  await ok('fire', { net: 'keep', transition: 't' });
  await ok('check', { net: 'keep' });
  await ok('simulate', { net: 'keep', tEnd: 1, maxPoints: 5 });
  step('B1: a trap restarts the engine; nets reload at their initial marking; fire, check and simulate answer');
}
{
  // the report's repro: big reachability and verify searches, repeated, then small calls
  const toggles = JSON.parse(readFileSync(join(here, 'test-corpus/synthetic/toggles-16.json'), 'utf8'));
  for (let i = 0; i < 2; i++) {
    const a = await call('reachability', { model: toggles, max_states: 20000 });
    assert.ok(!a.error && a.json.complete === false, `round ${i}: ${a.text.slice(0, 200)}`);
    const v = await call('verify', { net: toggles, properties: [{ kind: 'deadlock-free' }], max_states: 20000 });
    assert.ok(!v.error && v.json.verdicts[0].status !== 'proved', `round ${i}: ${v.text.slice(0, 200)}`);
  }
  const t = await ok('reachability', { model: tiny });
  assert.equal(t.states, 2);
  await ok('check', { net: tiny });
  step('B1: the stack-overflow repro (reachability + verify at 20000 states, twice) leaves the server answering');
}

// ---- B3: both net shapes on every hosted-name tool ---------------------------
{
  const extra = {
    verify: { properties: [{ kind: 'deadlock-free' }] },
    what_if: { place: 'b' },
    edit: { ops: [{ op: 'add_place', id: 'x' }] },
  };
  for (const name of ['check', 'cid', 'edit', 'info', 'validate', 'verify', 'invariants', 'incidence', 'export', 'sde', 'what_if']) {
    const r = await call(name, { net: tiny, ...(extra[name] ?? {}) });
    assert.ok(!r.error, `${name} with an inline array-shape net: ${r.text.slice(0, 300)}`);
  }
  const cidInline = (await ok('cid', { net: tiny })).cid;
  await ok('load_net', { model: tiny, name: 'tiny-ld' });
  assert.equal((await ok('cid', { net: 'tiny-ld' })).cid, cidInline, 'inline and loaded array nets seal to one CID');
  const bad = await call('info', { net: { places: [{ id: 'a' }], arcs: [{ from: 'a', to: 'nowhere' }] } });
  assert.ok(bad.error, bad.text);
  assert.match(bad.text, /array-shape net/, bad.text);
  assert.doesNotMatch(bad.text, /flat net pflow_bundle_flatten/, bad.text);
  console.log(`   a broken array-shape net: ${bad.text.slice(0, 160)}`);
  step('B3: all 11 hosted-name tools that take `net` read an inline array-shape net; a wrong shape says what is expected');
}

// ---- B4: rate 0 is off --------------------------------------------------------
{
  const sir = {
    name: 'sir',
    places: [{ id: 'S', initial: 99 }, { id: 'I', initial: 1 }, { id: 'R' }],
    transitions: [{ id: 'infect', rate: 0.3 }, { id: 'recover', rate: 0.1 }],
    arcs: [{ from: 'S', to: 'infect' }, { from: 'I', to: 'infect' }, { from: 'infect', to: 'I', weight: 2 },
      { from: 'I', to: 'recover' }, { from: 'recover', to: 'R' }],
  };
  await ok('load_net', { model: sir, name: 'sir' });
  const lastOf = (sim, p) => { const s = sim.series[p]; const v = Array.isArray(s) ? s : s.values; return v[v.length - 1]; };
  const ode = await ok('simulate', { net: 'sir', tEnd: 50, maxPoints: 20, rates: { infect: 0 } });
  assert.equal(lastOf(ode, 'S'), 99, 'ODE: infect at rate 0 never fires');
  const odeOn = await ok('simulate', { net: 'sir', tEnd: 50, maxPoints: 20 });
  assert.ok(lastOf(odeOn, 'S') < 99, 'ODE: and with its own rate it does');
  const ssa = await ok('stochastic', { net: 'sir', horizon: 50, seed: 7, rates: { infect: 0 } });
  assert.equal(ssa.final.S, 99, `SSA: infect at rate 0 never fires (${JSON.stringify(ssa.final)})`);
  assert.ok(ssa.final.R > 0, `SSA: and recover, at its own rate, does (${JSON.stringify(ssa.final)})`);
  const ssaOn = await ok('stochastic', { net: 'sir', horizon: 50, seed: 7 });
  assert.ok(ssaOn.final.S < 99, `SSA: with its own rate infect fires (${JSON.stringify(ssaOn.final)})`);
  const off = { ...sir, name: 'sir-off', transitions: [{ id: 'infect', rate: 0 }, { id: 'recover', rate: 0.1 }] };
  await ok('load_net', { model: off, name: 'sir-off' });
  assert.equal(lastOf(await ok('simulate', { net: 'sir-off', tEnd: 50, maxPoints: 20 }), 'S'), 99, 'a model rate of 0 is off too');
  const unset = await ok('simulate', { net: 'sir', tEnd: 50, maxPoints: 20, rates: { infect: null } });
  assert.ok(lastOf(unset, 'S') < 99, 'null still means unset');
  step('B4: rate 0 switches a transition off in simulate, stochastic and the model; null is unset');
}

// ---- B5: sealing cost ---------------------------------------------------------
{
  const big = chain(200);
  const t0 = Date.now();
  const c = await ok('cid', { net: big });
  const ms = Date.now() - t0;
  assert.match(c.cid, /^z4EBG9j/);
  assert.ok(ms < 30000, `cid of a 200-place chain took ${ms} ms (it was ~53 s before the fix)`);
  console.log(`   cid of a 200-place chain: ${ms} ms`);
  const slow = await call('cid', { net: chain(300), timeout_ms: 50 });
  assert.ok(slow.error, 'a seal past timeout_ms is an error');
  assert.match(slow.text, /ran past 50 ms and was stopped/);
  const after = await ok('check', { net: tiny });
  assert.ok(after, 'the server answers after a stopped seal');
  const e = await ok('edit', { net: big, ops: [{ op: 'add_place', id: 'extra' }], cid: false });
  assert.equal(e.cid, undefined);
  step('B5: cid of 200 places within budget; a seal past timeout_ms is stopped with a clear error; edit cid:false skips it');
}

console.log(`test-fixes-041: ${passed} groups passed`);
await client.close();
process.exit(0);
