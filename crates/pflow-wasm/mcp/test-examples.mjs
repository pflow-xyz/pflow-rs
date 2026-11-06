// list_examples / load_example (B6): every built-in example loads, its CID
// recomputed locally equals the recorded one, and artifact_html works on each.
//   node crates/pflow-wasm/mcp/test-examples.mjs <built pflow-mcp dir>
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const { Client } = await import(join(dir, 'node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js'));
const { StdioClientTransport } = await import(join(dir, 'node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js'));
const client = new Client({ name: 'test-examples', version: '0' });
await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')] }));
const call = async (name, args = {}) => {
  const r = await client.callTool({ name, arguments: args });
  let json; try { json = JSON.parse(r.content[0].text); } catch { /* text */ }
  return { error: !!r.isError, text: r.content[0].text, json };
};
const ok = async (name, args) => { const r = await call(name, args); assert.ok(!r.error, `${name}: ${r.text.slice(0, 300)}`); return r.json; };

const { examples } = await ok('list_examples');
assert.ok(examples.length >= 7, 'six public nets plus the composition');
assert.ok(examples.some((e) => e.kind === 'composition'));
const index = JSON.parse(readFileSync(join(dir, 'examples/index.json'), 'utf8'));
let published = 0;
for (const e of examples) {
  const loaded = await ok('load_example', { id: e.id });
  assert.equal(loaded.cid, e.cid, `${e.id}: recomputed CID differs from the recorded one`);
  assert.equal(loaded.cid_matches_recorded, true);
  assert.ok(!loaded.unsupported, `${e.id}: ${loaded.unsupported}`);
  const rec = index.examples.find((x) => x.id === e.id);
  // the published CID: equal, or a discrepancy that index.json states in a cid_note
  if (rec.pflow_xyz_cid) {
    assert.equal(loaded.cid_matches_published, loaded.cid === rec.pflow_xyz_cid);
    if (!loaded.cid_matches_published) assert.ok(rec.cid_note && loaded.cid_note, `${e.id}: unexplained published-CID mismatch`);
    else assert.equal(rec.cid_note, undefined);
    published += loaded.cid_matches_published ? 1 : 0;
  }
  for (const tool of ['check', 'info', 'cid']) await ok(tool, { net: e.id });
  const page = await ok('artifact_html', { net: e.id, path: join(dir, `ex-${e.id}.html`) });
  const html = readFileSync(page.path, 'utf8');
  assert.ok(html.includes(`?cid=${loaded.cid}`), `${e.id}: page lacks the pflow.xyz ?cid= link`);
  assert.ok(html.includes('pflow-net'), `${e.id}: page lacks #pflow-net`);
  console.log(`ok  ${e.id}  ${loaded.cid.slice(0, 14)}…  ${html.length} bytes`);
}
assert.ok(published >= 4, `only ${published} examples equal their published CID`);
// a custom name, and an unknown id refused with the list
assert.equal((await ok('load_example', { id: 'coffee-shop', name: 'mine' })).name, 'mine');
const bad = await call('load_example', { id: 'nope' });
assert.ok(bad.error && /coffee-shop/.test(bad.text));
await client.close();
console.log('examples: ok');
