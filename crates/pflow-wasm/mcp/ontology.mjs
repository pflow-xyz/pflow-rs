// The pflow ontology (CC0, ontology.pflow.xyz) as lookup tools over the Turtle
// files bundled in the package. Parsed lazily on first use; no network.
import { readFileSync, readdirSync } from 'node:fs';
import { z } from 'zod';
import { Parser, Store } from 'n3';

const dir = new URL('./ontology/', import.meta.url);
const NS = {
  ns: 'https://ontology.pflow.xyz/ns#',
  term: 'https://ontology.pflow.xyz/term/',
  pat: 'https://ontology.pflow.xyz/pattern/',
  lk: 'https://ontology.pflow.xyz/linkkind/',
  claim: 'https://ontology.pflow.xyz/claim/',
  ref: 'https://ontology.pflow.xyz/ref/',
  old: 'https://pflow.xyz/schema#',
  rdf: 'http://www.w3.org/1999/02/22-rdf-syntax-ns#',
  rdfs: 'http://www.w3.org/2000/01/rdf-schema#',
  owl: 'http://www.w3.org/2002/07/owl#',
  skos: 'http://www.w3.org/2004/02/skos/core#',
  dct: 'http://purl.org/dc/terms/',
  schema: 'https://schema.org/',
};
const P = {
  type: NS.rdf + 'type', label: NS.rdfs + 'label', comment: NS.rdfs + 'comment',
  sub: NS.rdfs + 'subClassOf', pref: NS.skos + 'prefLabel', alt: NS.skos + 'altLabel',
  def: NS.skos + 'definition', close: NS.skos + 'closeMatch', source: NS.dct + 'source',
  cls: NS.owl + 'Class',
};

let store = null;
let meta = null;

/**
 * Which ontology snapshot this package carries: { version, commit, dirty?, license,
 * source, snapshot }, from ontology/meta.json (stamped by build.mjs). Reads
 * one small file and never parses the Turtle; null when the package has no stamp.
 */
export function ontologyInfo() {
  if (meta) return meta;
  try { return JSON.parse(readFileSync(new URL('meta.json', dir), 'utf8')); } catch { return null; }
}

function load() {
  if (store) return store;
  store = new Store();
  for (const f of readdirSync(dir).filter((f) => f.endsWith('.ttl')).sort()) {
    store.addQuads(new Parser({ format: 'text/turtle' }).parse(readFileSync(new URL(f, dir), 'utf8')));
  }
  try { meta = JSON.parse(readFileSync(new URL('meta.json', dir), 'utf8')); } catch { meta = {}; }
  return store;
}

const compact = (iri) => {
  for (const [p, base] of Object.entries(NS)) if (iri.startsWith(base)) return `${p}:${iri.slice(base.length)}`;
  return iri;
};
const objs = (s, p) => load().getObjects(s, p, null);
const text = (s, ...ps) => {
  for (const p of ps) {
    const o = objs(s, p)[0];
    if (o) return o.value;
  }
  return undefined;
};
const iris = (s, p) => objs(s, p).filter((o) => o.termType === 'NamedNode').map((o) => o.value);
const has = (iri) => load().countQuads(iri, null, null, null) > 0;

/** IRIs a short name, curie or full IRI could mean, among those the graph states something about. */
function resolve(name) {
  const t = name.trim().replace(/^<|>$/g, '');
  if (/^https?:\/\//.test(t)) return has(t) ? [t] : [];
  const m = t.match(/^([a-z]+):(.+)$/);
  if (m && NS[m[1]]) return has(NS[m[1]] + m[2]) ? [NS[m[1]] + m[2]] : [];
  const out = [];
  for (const base of [NS.ns, NS.term, NS.pat, NS.lk, NS.old]) if (has(base + t)) out.push(base + t);
  if (out.length) return out;
  const want = t.toLowerCase();
  for (const base of [NS.ns, NS.term, NS.pat, NS.lk]) {
    for (const s of load().getSubjects(null, null, null)) {
      if (s.termType === 'NamedNode' && s.value.startsWith(base) && s.value.slice(base.length).toLowerCase() === want) out.push(s.value);
    }
  }
  return [...new Set(out)];
}

// closeMatch is symmetric in use: the glossary asserts `term:x skos:closeMatch ns:Arc`,
// so a class finds its glossary concept by the reverse edge.
const closeMatches = (iri) => {
  const fwd = iris(iri, P.close);
  const rev = load().getSubjects(P.close, iri, null).filter((x) => x.termType === 'NamedNode').map((x) => x.value);
  return [...new Set([...fwd, ...rev])];
};

// Own definition, then a close match's, then rdfs:comment (the hosted server's order).
function definitionOf(iri) {
  const own = text(iri, P.def);
  if (own) return own;
  for (const c of closeMatches(iri)) {
    const d = text(c, P.def);
    if (d) return d;
  }
  return text(iri, P.comment);
}

function view(iri) {
  const s = load();
  const subs = s.getSubjects(P.sub, iri, null).filter((x) => x.termType === 'NamedNode').map((x) => compact(x.value));
  return {
    iri,
    id: compact(iri),
    label: text(iri, P.pref, P.label),
    altLabels: objs(iri, P.alt).map((o) => o.value),
    types: iris(iri, P.type).map(compact),
    definition: definitionOf(iri),
    superclasses: iris(iri, P.sub).map(compact),
    subclasses: subs,
    closeMatch: closeMatches(iri).map(compact),
    sources: objs(iri, P.source).map((o) => o.value),
  };
}

function search(q, limit) {
  const needle = q.toLowerCase();
  const hits = [];
  for (const s of load().getSubjects(null, null, null)) {
    if (s.termType !== 'NamedNode') continue;
    const label = text(s.value, P.pref, P.label) ?? '';
    const def = definitionOf(s.value) ?? '';
    const alts = objs(s.value, P.alt).map((o) => o.value).join(' ');
    const hay = `${label} ${alts} ${def} ${s.value.split(/[#/]/).pop()}`.toLowerCase();
    if (!hay.includes(needle)) continue;
    const score = label.toLowerCase() === needle ? 0 : label.toLowerCase().includes(needle) ? 1 : 2;
    hits.push({ score, id: compact(s.value), label: label || undefined, types: iris(s.value, P.type).map(compact), definition: def.slice(0, 240) || undefined });
  }
  hits.sort((a, b) => a.score - b.score || a.id.localeCompare(b.id));
  return hits.slice(0, limit).map(({ score, ...h }) => h);
}

function classes() {
  const s = load();
  const all = s.getSubjects(P.type, P.cls, null).filter((x) => x.termType === 'NamedNode' && x.value.startsWith(NS.ns)).map((x) => x.value);
  const set = new Set(all);
  const up = (c) => iris(c, P.sub).filter((x) => set.has(x));
  const reach = (from, seen = new Set()) => {
    for (const p of up(from)) if (!seen.has(p)) { seen.add(p); reach(p, seen); }
    return seen;
  };
  return all.sort().map((c) => {
    const direct = up(c);
    // drop a superclass another listed one already implies
    const kept = direct.filter((p) => !direct.some((q) => q !== p && reach(q).has(p)));
    return { id: compact(c), label: text(c, P.label, P.pref), superclasses: kept.map(compact) };
  });
}

export function register(tool) {
  tool('ontology_describe',
    'Look up one term in the bundled pflow ontology: a class, property, glossary concept, pattern or link kind. Give a short name (Place, bounded, pat:queue, lk:event) or a full IRI. Offline: reads the snapshot shipped in this package.',
    { term: z.string() },
    ({ term }) => {
      const found = resolve(term);
      if (!found.length) throw new Error(`nothing is stated about "${term}"; try ontology_search`);
      const views = found.map(view);
      return { ontology: meta ?? {}, ...(views.length === 1 ? views[0] : { terms: views }) };
    });

  tool('ontology_search',
    'Find ontology terms whose label, alternative label, name or definition contains the text (case-insensitive). Offline.',
    { text: z.string(), limit: z.number().int().min(1).max(100).optional().describe('Default 25') },
    ({ text: q, limit }) => ({ ontology: (load(), meta), matches: search(q, limit ?? 25) }));

  tool('ontology_classes',
    'All classes in the ontology namespace with their direct superclasses, as asserted (a superclass another listed one already implies is left out). Offline.',
    {}, () => ({ ontology: (load(), meta), classes: classes() }));
}
