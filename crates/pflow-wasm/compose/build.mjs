#!/usr/bin/env node
// build.mjs: the snap-together canvas as ONE html file: the hosted engine
// (wasm embedded), verify.mjs and app.js inlined. No import, no fetch, no
// network; small enough (about 1.1 MB) to publish as a Claude artifact.
//
//   node crates/pflow-wasm/compose/build.mjs <hosted engine dir> <out.html>
//   (engine dir: target/npm/pflow-wasm-hosted, from `make wasm-mcp-engine`)
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const [dir = 'target/npm/pflow-wasm-hosted', out = 'target/compose-canvas.html'] = process.argv.slice(2);
const here = (f) => readFileSync(new URL(f, import.meta.url), 'utf8');
const stripExports = (s) =>
  s.replace(/^export default function init/m, 'function init')
    .replace(/^export class /gm, 'class ')
    .replace(/^export function /gm, 'function ')
    .replace(/^export \{[^}]*\};?\s*$/gm, '')
    .replace(/<\/script/gi, '<\\/script');

const engine = stripExports(readFileSync(join(resolve(dir), 'pflow-wasm.inline.js'), 'utf8'));
const html = here('canvas.template.html')
  .replace('/*__ENGINE__*/', () => engine)
  .replace('/*__VERIFY__*/', () => stripExports(here('verify.mjs')))
  .replace('/*__APP__*/', () => stripExports(here('app.js')));
if (/\bimport\s*\(|\bfrom\s+['"]/.test(html.replace(engine, ''))) throw new Error('the page must not import anything');
writeFileSync(out, html);
console.log(`${out}: ${(html.length / 1024).toFixed(0)} KiB`);
