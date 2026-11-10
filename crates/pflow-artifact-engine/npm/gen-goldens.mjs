#!/usr/bin/env node
// Writes goldens/<net>.json: what the engine pflow_artifact embedded inline before
// this package existed (pflow-xyz cmd/webserver/artifact/engine.wasm, sha256
// e8bdeed0…14f8) answers for each net in nets/. test.mjs holds the package to them.
// Run only to add a net, against that exact module; never to make a test pass.
//
//   node gen-goldens.mjs <old engine.wasm>
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { requests } from './requests.mjs';

const OLD_SHA = 'e8bdeed0081ba5811e4828b3788b41347a2a6eec0b1a6cf165d4af21c7e114f8';
const here = dirname(fileURLToPath(import.meta.url));
const wasm = readFileSync(process.argv[2]);
const sha = createHash('sha256').update(wasm).digest('hex');
if (sha !== OLD_SHA) throw new Error(`not the inline engine: sha256 ${sha}`);
const { instance } = await WebAssembly.instantiate(wasm, {});
const x = instance.exports, enc = new TextEncoder(), dec = new TextDecoder();
function raw(fn, req) {
  const b = enc.encode(JSON.stringify(req)), p = x.alloc(b.length);
  new Uint8Array(x.memory.buffer, p, b.length).set(b);
  const r = x[fn](p, b.length), rp = Number(r >> 32n), rl = Number(r & 0xffffffffn);
  const out = JSON.parse(dec.decode(new Uint8Array(x.memory.buffer, rp, rl)));
  x.dealloc(rp, rl); x.dealloc(p, b.length);
  return out;
}
const netsDir = join(here, 'nets');
for (const f of readdirSync(netsDir).filter((f) => f.endsWith('.json')).sort()) {
  const net = JSON.parse(readFileSync(join(netsDir, f), 'utf8'));
  const out = requests(net).map(([fn, req]) => ({ fn, req: { ...req, net: undefined }, reply: raw(fn, req) }));
  writeFileSync(join(here, 'goldens', f), JSON.stringify({ engine_sha256: OLD_SHA, net: f, calls: out }, null, 1) + '\n');
  console.log(f, out.length, 'calls');
}
