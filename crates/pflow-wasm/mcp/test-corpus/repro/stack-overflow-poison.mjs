#!/usr/bin/env node
// Repro: reachability with a large max_states on a big incomplete state space overflows the
// wasm stack ("Maximum call stack size exceeded"), the server's `finally { n.free() }` then
// masks it with "attempted to take ownership of Rust value while it was borrowed", and the
// wasm instance may be left corrupted after repeated overflows (later calls fail with "memory access out of bounds").
//   node stack-overflow-poison.mjs <pflow-mcp package dir>
import { createRequire } from 'node:module';
import { join, resolve, dirname } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import { toJsonLd } from '../lib.mjs';
const dir = resolve(process.argv[2]);
const req = createRequire(join(dir, 'package.json'));
const sdk = (p) => import(pathToFileURL(req.resolve(`@modelcontextprotocol/sdk/${p}`)).href);
const { Client } = await sdk('client/index.js'); const { StdioClientTransport } = await sdk('client/stdio.js');
const c = new Client({ name: 'repro', version: '0' });
await c.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')], stderr: 'pipe' }));
const call = async (n, a) => { const r = await c.callTool({ name: n, arguments: a }); return (r.isError ? 'ERR ' : 'ok  ') + r.content[0].text.slice(0, 110).replace(/\s+/g, ' '); };
const toggles = JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', 'synthetic', 'toggles-16.json'), 'utf8'));
const tiny = { name: 'tiny', places: [{ id: 'a', initial: 1 }, { id: 'b' }], transitions: [{ id: 't' }], arcs: [{ from: 'a', to: 't' }, { from: 't', to: 'b' }] };
const ld = toJsonLd(toggles);
console.log('before :', await call('reachability', { model: tiny }));
for (let i = 1; i <= 6; i++) {
  console.log(`trigger ${i} (reachability max_states 20000):`, await call('reachability', { model: toggles, max_states: 20000 }));
  console.log(`   verify 20000:`, await call('verify', { net: ld, properties: [{ kind: 'deadlock-free' }], max_states: 20000 }));
  console.log(`   then tiny reachability:`, await call('reachability', { model: tiny }));
  console.log(`   then hosted check:`, await call('check', { net: toJsonLd(tiny) }));
}
process.exit(0);
