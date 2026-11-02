// Live cross-check: the built local server's `reachability` against the hosted
// pflow.xyz/mcp `pflow_reachability` (go-pflow), on every net in
// crates/pflow-wasm/tests/reachability/nets. Needs the network, so it is not
// part of wasm-mcp-test; the offline guard is tests/reachability_parity.rs.
//   node crates/pflow-wasm/mcp/crosscheck-hosted.mjs <built pflow-mcp dir> [nets dir] [endpoint]
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

const here = dirname(fileURLToPath(import.meta.url));
const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const netsDir = resolve(process.argv[3] ?? join(here, '..', 'tests', 'reachability', 'nets'));
const endpoint = process.argv[4] ?? 'https://pflow.xyz/mcp';
const budgets = { source: 50 }; // as tests/reachability/gen

const client = new Client({ name: 'crosscheck', version: '0' });
await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')] }));

const hosted = async (net, max_states) => {
  const res = await fetch(endpoint, {
    method: 'POST',
    headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: 'pflow_reachability', arguments: { net, max_states } } }),
  });
  const body = await res.json();
  if (body.error || body.result?.isError) throw new Error(`hosted: ${JSON.stringify(body.error ?? body.result)}`);
  return JSON.parse(body.result.content[0].text);
};
const local = async (model, max_states) => {
  const r = await client.callTool({ name: 'reachability', arguments: { model, max_states } });
  if (r.isError) throw new Error(`local: ${r.content[0].text}`);
  return JSON.parse(r.content[0].text);
};
const sorted = (a) => [...(a ?? [])].sort();
const nz = (m) => m && Object.fromEntries(Object.entries(m).filter(([, v]) => v !== 0));

let failed = 0;
for (const f of readdirSync(netsDir).filter((f) => f.endsWith('.json')).sort()) {
  const name = f.replace(/\.json$/, '');
  const net = JSON.parse(readFileSync(join(netsDir, f), 'utf8'));
  const max = budgets[name] ?? 2000;
  const [g, l] = await Promise.all([hosted(net, max), local(net, max)]);
  try {
    for (const k of ['states', 'edges', 'complete', 'bounded', 'deadlocks', 'deadlock_free', 'live', 'quasi_live', 'truncated', 'bound'])
      assert.deepEqual(l[k], g[k], k);
    assert.deepEqual(nz(l.first_deadlock), nz(g.first_deadlock), 'first_deadlock');
    assert.deepEqual(sorted(l.not_live), sorted(g.not_live), 'not_live');
    assert.deepEqual(sorted(l.dead_transitions), sorted(g.dead_transitions), 'dead_transitions');
    assert.equal(l.witness?.kind, g.witness?.kind, 'witness kind');
    assert.equal(l.witness?.trace.length, g.witness?.trace.length, 'witness length');
    assert.deepEqual(sorted(l.witness?.never_fires), sorted(g.witness?.never_fires), 'never_fires');
    console.log(`ok   ${name}: ${l.states} states, deadlock_free=${l.deadlock_free} quasi_live=${l.quasi_live} live=${l.live}`);
  } catch (e) {
    failed++;
    console.log(`FAIL ${name}: ${e.message}`);
  }
}
await client.close();
if (failed) { console.error(`${failed} net(s) disagree with ${endpoint}`); process.exit(1); }
console.log(`reachability agrees with ${endpoint}`);
