// list_examples / load_example: the public curated nets, built in, offline.
//
// examples/*.jsonld are copied unchanged from pflow-xyz examples/landing; index.json
// records each one's CID (the document's own @id, as published at pflow.xyz), its
// sha256 and a short description (cid_matches_published says whether this engine
// seals the net to the CID pflow.xyz publishes it under). load_example goes through the same loader as
// load_net, so the net gets its JSON-LD, its CID (recomputed here, never read from
// the index), and every tool, artifact_html included, works on it. A page built
// from one carries the https://pflow.xyz/?cid= link to the public net.
//
// The composition is bundle_example's default PetriNetBundle, flattened with
// bundle_net into one net; its CID is the local one, recorded in index.json, and
// is not a published pflow.xyz net.
import { readFileSync } from 'node:fs';
import { z } from 'zod';

const dir = new URL('./examples/', import.meta.url);
const index = JSON.parse(readFileSync(new URL('index.json', dir), 'utf8'));
const EDITOR = 'https://pflow.xyz/';

const entries = () => index.examples.map((e) => ({
  id: e.id, title: e.title, description: e.description, kind: e.kind ?? 'net',
  cid: e.cid, pflow_xyz_cid: e.pflow_xyz_cid ?? null, cid_matches_published: e.cid_matches_published ?? null,
  url: e.pflow_xyz_cid ? `${EDITOR}?cid=${e.pflow_xyz_cid}` : null,
}));

export function register(tool, { loadNet, hosted }) {
  tool('list_examples',
    'List the public example nets built into this package (no network): the curated pflow.xyz nets plus the bundle_example composition as one net. Each has an id, description, its recorded CID and the pflow.xyz link. load_example loads one.',
    {}, () => ({ examples: entries(), source: index.source, next: 'load_example {id}, then artifact_html' }));

  tool('load_example',
    'Load a built-in public example under its id (or name), exactly as load_net would: it answers the net, its CID recomputed here (recorded_cid, cid_matches_recorded and cid_matches_published say whether it equals the recorded and the published pflow.xyz CID), and every other tool then reads it. artifact_html pages built from it link to the same net at pflow.xyz. See list_examples for ids.',
    { id: z.string().describe('Example id from list_examples'),
      name: z.string().optional().describe('Name to keep it under; defaults to the id') },
    ({ id, name }) => {
      const e = index.examples.find((x) => x.id === id);
      if (!e) throw new Error(`no example "${id}"; known: ${index.examples.map((x) => x.id).join(', ')}`);
      let model;
      if (e.kind === 'composition') {
        const b = hosted('bundle_example', {});
        model = hosted('bundle_net', { bundle: b.bundle }).net;
      } else {
        model = JSON.parse(readFileSync(new URL(e.file, dir), 'utf8'));
        // pflow.xyz keeps the author attribution and the @id outside the sealed document
        delete model.author; delete model['@id'];
      }
      const out = loadNet({ model, name: name ?? e.id, cid: true }) /* curated and bounded: always sealed, so the recorded CID can be checked */;
      return { example: e.id, title: e.title, description: e.description, recorded_cid: e.cid,
        cid_matches_recorded: out.cid === e.cid, pflow_xyz_cid: e.pflow_xyz_cid ?? null,
        cid_matches_published: e.pflow_xyz_cid ? out.cid === e.pflow_xyz_cid : null,
        ...(e.cid_note ? { cid_note: e.cid_note } : {}),
        pflow_xyz_url: e.pflow_xyz_cid ? `${EDITOR}?cid=${e.pflow_xyz_cid}` : null, ...out };
    });
}
