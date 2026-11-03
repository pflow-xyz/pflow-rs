// Corpus helpers shared by test-tools.mjs and the embedding prototypes.
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
export const here = dirname(fileURLToPath(import.meta.url));

/** Every corpus net: [{ key: 'synthetic/sir', dir, file, model }] */
export function corpus(dirs = ['synthetic', 'public']) {
  const out = [];
  for (const dir of dirs)
    for (const file of readdirSync(join(here, dir)).filter((f) => /\.json(ld)?$/.test(f)).sort())
      out.push({ key: `${dir}/${file.replace(/\.json(ld)?$/, '')}`, dir, file, model: JSON.parse(readFileSync(join(here, dir, file), 'utf8')) });
  return out;
}

/**
 * The array shape load_net takes (places/transitions/arcs as lists, arc type
 * "inhibitor"/"read") as the pflow.xyz JSON-LD the 18 hosted-name tools take
 * (places/transitions keyed by id, arcs with source/target/inhibitTransition; a read
 * arc is written output-side: transition -> place with inhibitTransition).
 * A document that is already JSON-LD is returned as is.
 */
export function toJsonLd(m) {
  if (m['@context'] || (m.places && !Array.isArray(m.places))) return m;
  const places = {}, transitions = {}, isPlace = new Set(m.places.map((p) => p.id));
  m.places.forEach((p, i) => { places[p.id] = { '@type': 'Place', initial: [p.initial ?? 0], capacity: [p.capacity ?? 0], x: 60 + (i % 10) * 90, y: 60 + Math.floor(i / 10) * 90 }; });
  m.transitions.forEach((t, i) => { transitions[t.id] = { '@type': 'Transition', rate: t.rate ?? 1, x: 60 + (i % 10) * 90, y: 40 + Math.floor(i / 10) * 90 }; });
  const arcs = m.arcs.map((a) => {
    const w = [a.weight ?? 1];
    if (a.type === 'inhibitor') return { '@type': 'Arrow', source: a.from, target: a.to, weight: w, inhibitTransition: true };
    if (a.type === 'read') return { '@type': 'Arrow', source: a.to, target: a.from, weight: w, inhibitTransition: true };
    return { '@type': 'Arrow', source: a.from, target: a.to, weight: w, inhibitTransition: false };
  });
  return { '@context': 'https://pflow.xyz/schema', '@type': 'PetriNet', '@version': '1.1', name: m.name, token: ['https://pflow.xyz/tokens/black'], places, transitions, arcs };
}
