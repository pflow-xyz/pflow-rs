// The view registry. A view is a module whose default export is
//   { id, title, needsEngineResult, buildSection(ctx) }
// and buildSection returns { css, html, js, seed? }: a page fragment (a few
// <section>s), its styles, and module code that runs after the engine is up in
// a block of its own, with `$`, `esc`, `num`, `Net`, `netDoc` (the net parsed
// from the page's #pflow-net block) and `setSeed(n)` in scope.
// ctx = { key, title, model, doc, cid, engine: { hosted, net }, meta }.
// Add a view: write views/<id>.mjs and list it here; nothing else changes.
import play from './play.mjs';
import proof from './proof.mjs';
import whatif from './whatif.mjs';
import analysis from './analysis.mjs';

const all = [play, analysis, whatif, proof];
export const registry = new Map(all.map((v) => [v.id, v]));
export const viewIds = () => [...registry.keys()];

export function getViews(ids) {
  const want = ids ?? ['play'];
  if (!Array.isArray(want) || want.length === 0) throw new Error(`views: give a non-empty list; known views: ${viewIds().join(', ')}`);
  const unknown = want.filter((id) => !registry.has(id));
  if (unknown.length) throw new Error(`unknown view${unknown.length > 1 ? 's' : ''} ${unknown.map((u) => JSON.stringify(u)).join(', ')}; known views: ${viewIds().join(', ')}`);
  return [...new Set(want)].map((id) => registry.get(id));
}
