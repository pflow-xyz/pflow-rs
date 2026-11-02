// Live cross-check: the built local server's ports of the pflow.xyz/mcp tools
// (hosted.mjs, crates/pflow-hosted) against the hosted ones, same arguments,
// on the nets in crates/pflow-hosted/tests/fixtures/nets. Needs the network, so
// it is not part of wasm-mcp-test; the offline guards are pflow-hosted's tests
// (cargo test -p pflow-hosted), which replay captured hosted answers and
// pflow-native's goldens.
//   node crates/pflow-wasm/mcp/crosscheck-tools.mjs <built pflow-mcp dir> [nets dir] [endpoint]
import { isDeepStrictEqual } from 'node:util';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

const here = dirname(fileURLToPath(import.meta.url));
const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const netsDir = resolve(process.argv[3] ?? join(here, '..', '..', 'pflow-hosted', 'tests', 'fixtures', 'nets'));
const endpoint = process.argv[4] ?? 'https://pflow.xyz/mcp';

// Per tool: the arguments besides `net`, and the keys whose values legitimately
// differ (documented in crates/pflow-hosted). export's svg is pflow-rs's own
// drawing, so only its json form is compared.
const CALLS = {
  check: [{}],
  cid: [{}],
  info: [{}],
  incidence: [{}],
  invariants: [{}, { kind: 'p' }, { kind: 't' }],
  validate: [{}, { reachability: true, max_states: 500 }],
  verify: [{ properties: [{ kind: 'deadlock-free' }, { kind: 'bounded' }, { kind: 'live' }, { kind: 'conserves' }] }],
  export: [{ format: 'json' }],
  sde: [{ runs: 3, seed: 5, points: 11 }],
};

const client = new Client({ name: 'crosscheck', version: '0' });
await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')] }));

const answer = (r) => ({ error: !!r.isError, value: r.isError ? r.content[0].text : JSON.parse(r.content[0].text) });
const hosted = async (name, args) => {
  const res = await fetch(endpoint, {
    method: 'POST',
    headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: `pflow_${name}`, arguments: args } }),
  });
  const body = await res.json();
  if (body.error) throw new Error(`hosted: ${JSON.stringify(body.error)}`);
  return answer(body.result);
};
const local = async (name, args) => answer(await client.callTool({ name, arguments: args }));

let failed = 0, passed = 0;
for (const f of readdirSync(netsDir).filter((f) => /\.json(ld)?$/.test(f)).sort()) {
  const net = JSON.parse(readFileSync(join(netsDir, f), 'utf8'));
  for (const [name, variants] of Object.entries(CALLS)) {
    for (const extra of variants) {
      const args = { net, ...extra };
      const [h, l] = await Promise.all([hosted(name, args), local(name, args)]);
      const label = `${name}${Object.keys(extra).length ? ' ' + JSON.stringify(extra) : ''} on ${f}`;
      if (h.error && l.error) { passed++; console.log(`ok   ${label}: both refuse`); continue; }
      if (isDeepStrictEqual(h, l)) { passed++; console.log(`ok   ${label}`); continue; }
      failed++;
      console.log(`DIFF ${label}\n  hosted: ${JSON.stringify(h).slice(0, 400)}\n  local:  ${JSON.stringify(l).slice(0, 400)}`);
    }
  }
}
await client.close();
console.log(`${passed} agree, ${failed} differ (${endpoint})`);
if (failed) process.exit(1);
