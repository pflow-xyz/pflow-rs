// End-to-end: spawn the built server over stdio and drive it as an MCP client.
//   node crates/pflow-wasm/mcp/test.mjs <built pflow-mcp dir>
import assert from 'node:assert/strict';
import { join, resolve } from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const client = new Client({ name: 'test', version: '0' });
await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')] }));

const call = async (name, args = {}) => {
  const r = await client.callTool({ name, arguments: args });
  const text = r.content[0].text;
  return { error: !!r.isError, text, json: r.isError ? null : JSON.parse(text) };
};

const tools = (await client.listTools()).tools.map((t) => t.name).sort();
const HOSTED = ['bundle_add_link', 'bundle_candidates', 'bundle_example', 'bundle_flatten', 'bundle_incidence', 'bundle_run',
  'bundle_validate', 'check', 'cid', 'edit', 'export', 'incidence', 'info', 'invariants', 'sde', 'validate', 'verify', 'what_if'];
assert.deepEqual(tools, ['artifact_html', 'chyp_check', 'chyp_part', 'chyp_rule', 'fire', 'list_nets', 'load_net',
  'ontology_classes', 'ontology_describe', 'ontology_search', 'reachability', 'reset', 'simulate', 'state', 'stochastic',
  ...HOSTED].sort());
const { version } = JSON.parse((await import('node:fs')).readFileSync(join(dir, 'package.json'), 'utf8'));
assert.equal(client.getServerVersion().version, version, 'server reports the package version');

const coffee = {
  name: 'coffee',
  places: [{ id: 'beans', initial: 3 }, { id: 'cups' }],
  transitions: [{ id: 'brew', rate: 0.5 }],
  arcs: [{ from: 'beans', to: 'brew' }, { from: 'brew', to: 'cups' }],
};
let r = await call('load_net', { model: coffee });
assert.equal(r.json.name, 'coffee');
assert.deepEqual(r.json.enabled, ['brew']);

r = await call('fire', { transition: 'brew' });
assert.equal(r.json.marking.beans, 2);
assert.equal(r.json.marking.cups, 1);

r = await call('fire', { transition: 'nope' });
assert.ok(r.error, 'firing an unknown transition is an error result, not a crash');

r = await call('simulate', { tEnd: 10, maxPoints: 20 });
assert.ok(Math.abs(r.json.final.beans + r.json.final.cups - 3) < 1e-6, 'tokens conserved');

const a = await call('stochastic', { horizon: 10, samples: 11, seed: 42 });
const b = await call('stochastic', { horizon: 10, samples: 11, seed: 42 });
assert.equal(a.text, b.text, 'same seed, same run');

// reachability: from the initial marking by default, as the hosted tool
r = await call('reachability');
assert.equal(r.json.states, 4);
assert.equal(r.json.complete, true);
assert.equal(r.json.deadlock_free, false);
assert.equal(r.json.quasi_live, true);
assert.equal(r.json.live, false);
assert.deepEqual(r.json.first_deadlock, { beans: 0, cups: 3 });
assert.deepEqual(r.json.witness.trace, ['brew', 'brew', 'brew']);
r = await call('reachability', { from: 'current' });
assert.equal(r.json.states, 3, 'from the current marking (one brew fired)');
assert.equal((await call('state')).json.marking.beans, 2, 'the search does not move the marking');

// a livelock: deadlock-free and L1, but not L4; the witness replays with fire
const livelock = {
  name: 'livelock',
  places: [{ id: 'p', initial: 1 }, { id: 'q' }, { id: 'r' }, { id: 's' }],
  transitions: ['t', 'u', 'x', 'v', 'w'].map((id) => ({ id })),
  arcs: [['p', 't'], ['t', 'q'], ['q', 'u'], ['u', 'p'], ['p', 'x'], ['x', 'r'], ['r', 'v'], ['v', 's'], ['s', 'w'], ['w', 'r']]
    .map(([from, to]) => ({ from, to })),
};
r = await call('reachability', { model: livelock });
assert.equal(r.json.deadlock_free, true);
assert.equal(r.json.quasi_live, true);
assert.equal(r.json.live, false);
assert.equal(r.json.witness.kind, 'livelock');
assert.deepEqual(r.json.witness.never_fires, ['t', 'u', 'x']);
assert.ok(!(await call('list_nets')).json.nets.includes('livelock'), 'an inline model is not loaded');
await call('load_net', { model: livelock });
let m;
for (const t of r.json.witness.trace) m = (await call('fire', { transition: t })).json.marking;
assert.deepEqual(m, r.json.witness.marking, 'witness replays');

// unbounded: the search stops, and claims nothing true
r = await call('reachability', { model: { name: 'src', places: [{ id: 'p' }], transitions: [{ id: 't' }], arcs: [{ from: 't', to: 'p' }] }, max_states: 50 });
assert.equal(r.json.complete, false);
assert.equal(r.json.states, 50);
assert.ok(r.json.truncated && /^unknown/.test(r.json.bounded));
for (const k of ['deadlock_free', 'live', 'quasi_live']) assert.notEqual(r.json[k], true, k);
assert.ok((await call('reachability', { max_states: 20001 })).error, 'over the budget cap');
assert.ok((await call('reachability', { model: livelock, from: 'current' })).error);

await call('reset', { net: 'coffee' });
assert.equal((await call('state', { net: 'coffee' })).json.marking.beans, 3);
assert.ok((await call('state', { net: 'missing' })).error);

r = await call('artifact_html', { net: 'coffee', path: join(dir, 'artifact-test.html') });
const { readFileSync, rmSync } = await import('node:fs');
const html = readFileSync(r.json.path, 'utf8');
rmSync(r.json.path);
assert.ok(html.includes('<title>coffee</title>') && html.includes('new Net(MODEL)'));
assert.ok(!/\bexport\s/.test(html.split('<script type="module">')[1]), 'exports stripped');
// the page's module must parse (node --check on an .mjs copy)
const { spawnSync } = await import('node:child_process');
const { writeFileSync } = await import('node:fs');
const mjs = join(dir, 'page-check.mjs');
writeFileSync(mjs, html.split('<script type="module">')[1].split('</script>')[0]);
const chk = spawnSync(process.execPath, ['--check', mjs], { encoding: 'utf8' });
rmSync(mjs);
assert.equal(chk.status, 0, chk.stderr);

// ontology: offline lookups from the bundled snapshot
r = await call('ontology_describe', { term: 'bounded' });
assert.ok(r.json.definition?.includes('finite maximum'), r.text.slice(0, 300));
assert.equal(r.json.ontology.license, 'CC0-1.0');
r = await call('ontology_describe', { term: 'ns:Arc' });
assert.ok(r.json.definition?.includes('directed connection'), 'class borrows its glossary definition via the reverse closeMatch');
r = await call('ontology_describe', { term: 'Place' });
assert.ok(r.json.id || r.json.terms, r.text.slice(0, 300));
r = await call('ontology_search', { text: 'petri' });
assert.ok(r.json.matches.length > 0);
r = await call('ontology_classes');
assert.ok(r.json.classes.some((c) => c.id === 'ns:Place'));
assert.ok((await call('ontology_describe', { term: 'no-such-thing-xyz' })).error);

// chyp: a small theory, checked
r = await call('chyp_check', { source: 'gen f : 1 -> 1\ngen g : 1 -> 1\nrule fg : f ; g = g ; f\n' });
assert.ok(!r.error, r.text.slice(0, 300));
assert.ok(r.json.upstream);
r = await call('chyp_rule', { rule: 'fg' });
assert.ok(!r.error, r.text.slice(0, 300));
assert.ok((await call('chyp_rule', { rule: 'nope' })).error);

// The pflow.xyz/mcp tools (hosted.mjs over crates/pflow-hosted). Their parity
// with the hosted answers is pflow-hosted's own tests; here each one runs once
// through the built server, its wasm engine and the net-by-name resolution.
{
  const sir = {
    '@context': 'https://pflow.xyz/schema', '@type': 'PetriNet', name: 'SIR', token: ['https://pflow.xyz/tokens/black'],
    places: { S: { initial: [99] }, I: { initial: [1] }, R: { initial: [0] } },
    transitions: { infect: { rate: 0.003 }, recover: { rate: 0.1 } },
    arcs: [{ source: 'S', target: 'infect', weight: [1] }, { source: 'I', target: 'infect', weight: [1] },
      { source: 'infect', target: 'I', weight: [2] }, { source: 'I', target: 'recover', weight: [1] },
      { source: 'recover', target: 'R', weight: [1] }],
  };
  r = await call('check', { net: sir });
  assert.deepEqual(r.json, { ok: true, places: 3, transitions: 2, arcs: 5 });
  r = await call('cid', { net: sir });
  assert.match(r.json.cid, /^z4EBG9j/);
  const cid = r.json.cid;
  r = await call('export', { net: sir });
  assert.equal(r.json.cid, cid, 'export json carries the same CID');
  r = await call('export', { net: sir, format: 'svg' });
  assert.ok(r.json.svg.startsWith('<svg') || r.json.svg.startsWith('<?xml'), r.text.slice(0, 120));
  r = await call('incidence', { net: sir });
  assert.deepEqual(r.json, { places: ['I', 'R', 'S'], transitions: ['infect', 'recover'], matrix: [[1, -1], [0, 1], [-1, 0]] });
  r = await call('invariants', { net: sir });
  assert.equal(r.json.p_invariants.length, 1);
  assert.equal(r.json.p_invariants[0].value, 100);
  r = await call('validate', { net: sir });
  assert.equal(r.json.valid, true);
  r = await call('verify', { net: sir, properties: [{ kind: 'conserves' }, { kind: 'reachable', target: { R: 1 } }] });
  assert.equal(r.json.verdicts[0].status, 'proved', r.text.slice(0, 300));
  assert.equal(r.json.verdicts[1].status, 'proved', 'R = 1 is reachable, with a trace');
  r = await call('info', { net: sir });
  assert.deepEqual(r.json.enabled, ['infect', 'recover']);
  r = await call('sde', { net: sir, runs: 2, seed: 3, points: 11 });
  assert.equal(r.json.engine, 'sde');
  assert.deepEqual(Object.keys(r.json.final).sort(), ['I', 'R', 'S']);
  r = await call('what_if', { net: sir, place: 'I', delta: 2, runs: 4, samples: 11 });
  assert.ok(!r.error, r.text.slice(0, 300));
  r = await call('sde', { net: sir, runs: 0 });
  assert.ok(r.error && r.text.includes('runs 0 is outside 1..200'), r.text);

  // A net given by name, and with no net at all the most recently loaded.
  await call('load_net', { model: sir, name: 'sir' });
  assert.equal((await call('cid', { net: 'sir' })).json.cid, cid);
  assert.equal((await call('cid')).json.cid, cid);
  assert.ok((await call('cid', { net: 'nope' })).error);
  r = await call('state', { net: 'sir' });
  assert.equal(r.json.info.name, 'SIR', 'state carries the info summary');

  // edit builds from nothing (no net means an empty one, not the loaded net).
  r = await call('edit', { ops: [{ op: 'add_place', initial: 2 }, { op: 'add_transition' }, { op: 'add_arc', source: 'p0', target: 't0' }] });
  assert.deepEqual(r.json.summary, { places: 1, transitions: 1, arcs: 1 });
  r = await call('edit', { ops: [{ op: 'nope' }] });
  assert.ok(r.error && r.text.startsWith('ops[0] (nope): unknown op'), r.text);

  // Compositions.
  r = await call('bundle_example');
  const bundle = r.json.bundle;
  assert.equal(r.json.scenarios.length, 4);
  assert.equal((await call('bundle_validate', { bundle })).json.valid, true);
  r = await call('bundle_flatten', { bundle });
  assert.ok(r.json.places.length > 0, r.text.slice(0, 200));
  r = await call('bundle_run', { bundle, scenario: 'commit' });
  assert.ok(r.json.completed && r.json.expect.every((p) => p.pass), r.text.slice(0, 400));
  r = await call('bundle_incidence', { bundle });
  assert.ok(Array.isArray(r.json.matrix), r.text.slice(0, 200));
  r = await call('bundle_candidates', { bundle, from: bundle.links[0].from, only_ok: true });
  assert.ok(!r.error, r.text.slice(0, 300));
  r = await call('bundle_add_link', { bundle, ...bundle.links[0] });
  assert.ok(r.error, 'the same link twice is refused');
}

await client.close();
console.log('pflow-mcp: ok');
