// The ontology snapshot and the part catalog, checked against each other.
//   node crates/pflow-wasm/mcp/ontology-parts.test.mjs <built pflow-mcp dir>
//   (copy it next to the built package, or run from here: it resolves the SDK
//   and n3 through the package's node_modules)
//
// 1. The snapshot ships core.ttl + symbols.ttl only (core.ttl already holds
//    components, glossary, knowledge, legacy and models), stamped by meta.json,
//    and ontologyInfo() reads that stamp.
// 2. The three parts both catalogs share (queue, resource-pool, saga): the
//    ontology's NetPattern entries (ports, parameters, invariants) against
//    bundle_parts. Every disagreement gets a stable id. The test fails on a
//    disagreement not in ALLOWED, and on an ALLOWED entry that no longer
//    occurs, so the list cannot rot. ALLOWED is the list of findings the owner
//    has been told about; this test does not decide which side is right.
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const need = createRequire(join(dir, 'package.json'));
const { Client } = await import(need.resolve('@modelcontextprotocol/sdk/client/index.js'));
const { StdioClientTransport } = await import(need.resolve('@modelcontextprotocol/sdk/client/stdio.js'));
const { Parser, Store } = await import(need.resolve('n3'));

// ---- 1. snapshot contents ---------------------------------------------------
const files = readdirSync(join(dir, 'ontology')).sort();
assert.deepEqual(files, ['core.ttl', 'meta.json', 'symbols.ttl'], 'snapshot ships core.ttl, symbols.ttl and the stamp, no duplicates of core.ttl');

const { ontologyInfo } = await import(pathToFileURL(join(dir, 'ontology.mjs')).href);
const info = ontologyInfo();
assert.equal(typeof info.version, 'string', 'ontologyInfo().version');
assert.ok(info.version.length > 0);
assert.ok(info.commit == null || /^[0-9a-f]{7,}$/.test(info.commit), `ontologyInfo().commit is a short hash or null, got ${info.commit}`);
assert.deepEqual(info, JSON.parse(readFileSync(join(dir, 'ontology', 'meta.json'), 'utf8')), 'ontologyInfo() is the stamp');
assert.equal(info.snapshot, true);

const client = new Client({ name: 'ontology-parts', version: '0' });
await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')] }));
const call = async (name, args = {}) => {
  const r = await client.callTool({ name, arguments: args });
  assert.ok(!r.isError, `${name}: ${r.content[0].text}`);
  return JSON.parse(r.content[0].text);
};
// the tools still see both files: a core term, a glossary concept (core.ttl) and a symbol (symbols.ttl)
assert.deepEqual((await call('ontology_describe', { term: 'pat:queue' })).ontology, info, 'tool answers carry the same stamp');
assert.ok((await call('ontology_search', { text: 'metamodel.APIRoute' })).matches.length > 0, 'symbols.ttl is loaded');
assert.ok((await call('ontology_search', { text: 'knowledge' })).matches.length > 0, 'core.ttl content is loaded');

// ---- 2. ontology patterns vs bundle_parts -----------------------------------
const NS = 'https://ontology.pflow.xyz/ns#';
const PAT = 'https://ontology.pflow.xyz/pattern/';
const store = new Store();
store.addQuads(new Parser({ format: 'text/turtle' }).parse(readFileSync(join(dir, 'ontology', 'core.ttl'), 'utf8')));
const one = (s, p) => store.getObjects(s, NS + p, null)[0];
const val = (s, p) => one(s, p)?.value;
const many = (s, p) => store.getObjects(s, NS + p, null).map((o) => o.value);

function pattern(id) {
  const s = PAT + id;
  assert.ok(store.countQuads(s, null, null, null) > 0, `ontology has pat:${id}`);
  return {
    params: many(s, 'hasParameter').map((p) => ({
      name: val(p, 'paramName'), required: val(p, 'paramRequired') === 'true', min: val(p, 'paramMin') == null ? undefined : Number(val(p, 'paramMin')),
    })),
    ports: many(s, 'hasPort').map((p) => ({
      id: val(p, 'portId'), template: val(p, 'portIdTemplate'), kind: val(p, 'portKind'), on: val(p, 'portTarget'),
      schema: val(p, 'portSchema'), cardinality: val(p, 'cardinality'), when: val(p, 'presentWhen'),
    })),
    invariants: many(s, 'hasInvariant').map((i) => ({ kind: val(i, 'invariantKind'), expr: val(i, 'constraintExpr'), when: val(i, 'presentWhen'), id: val(i, 'constraintId') })),
  };
}

const parts = Object.fromEntries((await call('bundle_parts')).parts.map((p) => [p.part, p]));
const SHARED = ['queue', 'resource-pool', 'saga'];

const tokenSet = (expr) => [...expr.matchAll(/tokens\("([^"]+)"\)/g)].map((m) => m[1]).sort().join(',');
const found = new Map(); // id -> detail
const diff = (id, detail) => found.set(id, detail);

for (const name of SHARED) {
  const part = parts[name];
  assert.ok(part, `bundle_parts has ${name}`);
  const o = pattern(name);

  // ports: a templated ontology port (`do:<step>`, `account:<id>`) matches every catalog port of that shape
  const cat = new Map(part.ports.map((p) => [p.id, p]));
  const matched = new Set();
  const cmp = (id, c, ex) => {
    for (const [k, ck] of [['kind', 'kind'], ['on', 'on'], ['schema', 'schema']]) {
      if ((ex[k] ?? undefined) !== (c[ck] ?? undefined)) diff(`${name}:port:${id}:${k}`, `ontology ${ex[k]}, bundle_parts ${c[ck]}`);
    }
  };
  for (const ex of o.ports) {
    if (ex.id) {
      const c = cat.get(ex.id);
      if (c) { matched.add(ex.id); cmp(ex.id, c, ex); } else if (!ex.when) diff(`${name}:port:${ex.id}:missing-in-bundle_parts`, 'ontology port with no condition, absent from bundle_parts');
    } else {
      const re = new RegExp('^' + ex.template.replace(/[.*+?^${}()|[\]\\]/g, '\\$&').replace(/<[a-z]+>/, '.+') + '$');
      const hits = part.ports.filter((p) => re.test(p.id));
      if (!hits.length) diff(`${name}:port:${ex.template}:missing-in-bundle_parts`, 'templated ontology port with no instance in bundle_parts');
      for (const c of hits) { matched.add(c.id); cmp(c.id, c, ex); }
    }
  }
  for (const c of part.ports) if (!matched.has(c.id)) diff(`${name}:port:${c.id}:missing-in-ontology`, 'bundle_parts port the ontology pattern does not declare');

  // parameters (catalog names are lower case, ontology ones follow the Go source)
  const lower = new Map(o.params.map((p) => [p.name.toLowerCase(), p]));
  for (const [k, c] of Object.entries(part.params ?? {})) {
    const ex = lower.get(k.toLowerCase());
    if (!ex) { diff(`${name}:param:${k}:missing-in-ontology`, 'bundle_parts parameter the ontology pattern does not declare'); continue; }
    if (ex.min != null && c.min != null && ex.min !== c.min) diff(`${name}:param:${k}:min`, `ontology min ${ex.min}, bundle_parts min ${c.min}`);
  }
  for (const ex of o.params) {
    if (ex.name.toLowerCase() === 'id') continue; // the instance id of a part is the instances[].id
    if (ex.required && !(ex.name in (part.params ?? {})) && !Object.keys(part.params ?? {}).some((k) => k.toLowerCase() === ex.name.toLowerCase())) {
      diff(`${name}:param:${ex.name}:required-in-ontology-missing-in-bundle_parts`, 'ontology says required, bundle_parts has no such parameter');
    }
  }

  // invariants: expand per-step templates over the steps and accounts the catalog's own ports show
  const steps = part.ports.map((p) => p.id.match(/^do:(.+)$/)?.[1]).filter(Boolean);
  const accounts = part.ports.map((p) => p.id.match(/^account:(.+)$/)?.[1]).filter(Boolean);
  const wantSets = []; // place sets the ontology pins down
  let wantOpaque = 0; // invariants it states only in prose
  for (const inv of o.invariants) {
    if (inv.id.includes('<step>')) {
      for (const st of steps) wantSets.push({ key: tokenSet(inv.expr.replaceAll('<step>', st).replace(/tokens\(([^")]+)\)/g, 'tokens("$1")')), from: `${inv.id.replace('<step>', st)}` });
    } else if (tokenSet(inv.expr)) {
      wantSets.push({ key: tokenSet(inv.expr), from: inv.id });
    } else if (inv.kind === 'conservation' && accounts.length) {
      wantSets.push({ key: [...accounts].sort().join(','), from: inv.id }); // "sum of tokens over the accounts"
    } else wantOpaque++;
  }
  const have = part.laws.map((l) => ({ key: tokenSet(l), law: l }));
  const used = new Set();
  for (const w of wantSets) {
    const i = have.findIndex((h, j) => !used.has(j) && h.key === w.key);
    if (i < 0) diff(`${name}:law:${w.from}:missing-in-bundle_parts`, `ontology invariant over {${w.key}} has no law in bundle_parts`); else used.add(i);
  }
  const rest = have.filter((_, j) => !used.has(j));
  if (rest.length !== wantOpaque) {
    for (const r of rest) diff(`${name}:law:{${r.key}}:missing-in-ontology`, `bundle_parts law ${r.law} matches no ontology invariant (${wantOpaque} prose-only ones unmatched)`);
  }
}

// Findings the owner has been told about (B5 report, 2026-11-06). Id -> what and why it stays.
const REPORTED = 'reported to the owner as a B5 finding; decide which catalog is right, then delete the entry';
const ALLOWED = {
  'queue:param:capacity:min': REPORTED + ': ontology allows 0 (Capacity 0 = unbounded queue), bundle_parts requires >= 1, so the catalog cannot express an unbounded queue',
  'resource-pool:param:name:required-in-ontology-missing-in-bundle_parts': REPORTED + ': ontology lists the Go constructor argument name as required; bundle_parts has no such parameter (instance id only)',
  'resource-pool:param:metadata:required-in-ontology-missing-in-bundle_parts': REPORTED + ': ontology lists the Go constructor argument metadata as required; bundle_parts has no such parameter',
  'saga:param:customer:missing-in-ontology': REPORTED + ': bundle_parts exposes the customer account balance as a parameter; the ontology models it as accounts.<id>.initial',
  'saga:param:steps:required-in-ontology-missing-in-bundle_parts': REPORTED + ': ontology says steps is required; bundle_parts offers a fixed reserve/charge saga with no steps parameter',
};

const unreported = [...found].filter(([id]) => !(id in ALLOWED));
const stale = Object.keys(ALLOWED).filter((id) => !found.has(id));
if (process.env.PRINT_DISAGREEMENTS) for (const [id, d] of found) console.log(`${id}\t${d}`);
await client.close();
assert.deepEqual(unreported, [], `disagreements between the ontology patterns and bundle_parts not yet reported to the owner:\n${unreported.map(([i, d]) => `  ${i}: ${d}`).join('\n')}`);
assert.deepEqual(stale, [], `ALLOWED entries that no longer occur (remove them):\n${stale.map((i) => '  ' + i).join('\n')}`);
console.log(`ontology-parts: ${found.size} known disagreements across ${SHARED.join(', ')} (all reported); snapshot ${info.version}@${info.commit}`);
