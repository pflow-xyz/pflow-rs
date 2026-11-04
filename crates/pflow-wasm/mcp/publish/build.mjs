#!/usr/bin/env node
// Builds the embedding prototypes for @pflow-xyz/pflow-mcp into publish/out/:
//   explorer-small.html  small engine only (pflow-wasm.inline.js, as artifact_html embeds today)
//   explorer-full.html   full engine only (pflow-wasm-hosted.inline.js: Net plus every hosted tool)
//   explorer-both.html   both engines in one page (measures what embedding the small one as well costs)
//   saga-proof.html      full engine + chyp-rs: bundle checks plus a Chyp rewrite proof re-checked in the page
//
//   node publish/build.mjs <built pflow-mcp dir> [out dir]
//
// Every page is one self-contained file: engines are wrapped in function scopes (their `export`s are
// stripped, so two builds that both define `Net` and `wasm` do not collide), wasm travels as
// gzip+base64 and is inflated with DecompressionStream, and there is no URL, font or script from elsewhere.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { corpus, toJsonLd } from '../test-corpus/lib.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const pkg = resolve(process.argv[2] ?? join(here, '..', '..', '..', '..', 'target', 'npm', 'pflow-mcp'));
const out = resolve(process.argv[3] ?? join(here, 'out'));
mkdirSync(out, { recursive: true });
const read = (p) => readFileSync(join(pkg, p), 'utf8');
const kb = (n) => `${Math.round(n / 1024)} KB`;

/** An ES module's source as an expression-free block that defines `const <name> = {…exports}`. */
function wrap(src, name, exports) {
  const s = src
    .replace(/^\/\* @ts-self-types.*$/m, '')
    .replace(/^export default function (\w+)/m, 'function $1')
    .replace(/^export (class|function) /gm, '$1 ')
    .replace(/^export \{[^}]*\};?\s*$/gm, '')
    .replace(/new URL\('chyp_rs_bg\.wasm', import\.meta\.url\)/, 'undefined')
    .replace(/<\/script/gi, '<\\/script');
  if (/\bimport\.meta\b|^export\b/m.test(s)) throw new Error(`${name}: an export or import.meta survived the wrap`);
  return `const ${name}=(()=>{${s}\nreturn {${exports.join(',')}};})();`;
}
const smallSrc = read('pflow-wasm.inline.js'), fullSrc = read('pflow-wasm-hosted.inline.js'), chypSrc = read('chyp/chyp_rs.js');
const SMALL = wrap(smallSrc, 'SMALL', ['Net', 'version', 'init: init']);
const FULL = wrap(fullSrc, 'FULL', ['Net', 'hosted', 'hostedTools', 'version', 'init: init']);
const CHYP = wrap(chypSrc, 'CHYP', ['Session', 'check', 'upstream_commit', 'initSync', 'init: __wbg_init']);
const chypGz = gzipSync(readFileSync(join(pkg, 'chyp/chyp_rs_bg.wasm')), { level: 9 }).toString('base64');

// ---- models for the explorer -------------------------------------------------
const pick = [
  ['synthetic/mutex', 'Mutual exclusion', 'Two processes share one lock. At most one is ever in its critical section.'],
  ['synthetic/philosophers-3', 'Dining philosophers', 'Three philosophers, three forks, each grabs left then right. A classic way to get stuck.'],
  ['synthetic/philosophers-3-ordered', 'Philosophers, ordered', 'The same table, but the last philosopher picks up forks in the opposite order.'],
  ['synthetic/producer-consumer', 'Bounded buffer', 'A producer fills a buffer of three slots and a consumer empties it.'],
  ['synthetic/sir', 'Epidemic (SIR)', 'A hundred people, one infected. Infection needs a susceptible and an infected person to meet.'],
  ['synthetic/enzyme', 'Enzyme reaction', 'An enzyme binds a substrate, converts it, and is released to start again.'],
];
const all = corpus(['synthetic']);
const MODELS = pick.map(([key, title, blurb]) => ({ id: key.split('/')[1], title, blurb, ld: toJsonLd(all.find((x) => x.key === key).model) }));

// ---- page shell --------------------------------------------------------------
const css = readFileSync(join(here, 'style.css'), 'utf8');
const safeJson = (v) => JSON.stringify(v).replace(/</g, '\\u003c');
function shell(title, body, script) {
  return `<title>${title}</title>
<meta name="claude:viewport-fit-sides" content="cover">
<style>${css}</style>
<meta charset="utf-8">
<main>${body}</main>
<script type="module">
${script}
</script>
`;
}
function finish(file, html, engineBytes) {
  const token = '0000000000';
  const withBytes = (n) => html.replace(`PAGE_BYTES=Number("${token}")`, `PAGE_BYTES=Number("${String(n).padStart(10, '0')}")`);
  const final = withBytes(Buffer.byteLength(html));
  writeFileSync(join(out, file), final);
  console.log(`${file.padEnd(22)} ${kb(Buffer.byteLength(final)).padStart(8)}  (engines ${engineBytes})`);
}
const explorerBody = (variant) => `
<header><span class="kicker">Petri net, checked in your browser</span><h1 id="title"></h1><p class="lede" id="blurb"></p></header>
<section><h2>Pick a model</h2><div class="picker" id="picker"></div></section>
<section><h2>What can be proved <span id="checkms" class="mono" style="font-weight:400;text-transform:none;letter-spacing:0"></span></h2><ul class="checks" id="checks"></ul></section>
<section><h2>Try it</h2><div class="marks" id="marks"></div><div class="tx" id="tx"></div><div class="row" style="margin-top:8px"><button id="reset" type="button">Start over</button></div><div class="log" id="log"></div></section>
<section><h2>Over time</h2>
<div class="row"><label for="t">time <input id="t" type="number" value="10"></label><label for="n">runs <input id="n" type="number" value="20"></label><label for="seed">seed <input id="seed" type="number" value="1"></label>
<button id="ode" type="button" class="primary">Average behaviour</button><button id="ssa" type="button">Random run</button></div>
<div id="chart"></div><div class="leg" id="legend"></div><div class="log" id="clog"></div></section>
${variant === 'small' ? '' : `
<section><h2>What if</h2><div class="row"><label for="wiplace">add tokens to <select id="wiplace"></select></label><label for="widelta">how many <input id="widelta" type="number" value="1"></label><label for="wit">until time <input id="wit" type="number" value="10"></label><button id="wi" type="button" class="primary">Compare</button></div><div id="wiout"></div></section>
<section><h2>Every tool in the engine</h2><div class="row"><label for="tool">tool <select id="tool"></select></label><button id="runtool" type="button" class="primary">Run</button></div>
<textarea id="args" spellcheck="false" aria-label="tool arguments (JSON)"></textarea><pre id="toolout">The current model is passed as <code>net</code> unless you set one.</pre></section>`}
<footer id="foot"></footer>`;

function explorer(variant, title) {
  const engines = variant === 'small' ? [SMALL] : variant === 'full' ? [FULL] : [SMALL, FULL];
  const bytes = variant === 'small' ? kb(Buffer.byteLength(smallSrc)) : variant === 'full' ? kb(Buffer.byteLength(fullSrc)) : `${kb(Buffer.byteLength(smallSrc))} + ${kb(Buffer.byteLength(fullSrc))}`;
  const script = [...engines, `const MODELS=${safeJson(MODELS)},VARIANT=${safeJson(variant)},ENGINE_BYTES=${safeJson(bytes)},PAGE_BYTES=Number("0000000000");`, readFileSync(join(here, 'client-net.js'), 'utf8')].join('\n');
  finish(`explorer-${variant}.html`, shell(title, explorerBody(variant), script), bytes);
}
explorer('small', 'Net Explorer Lite');
explorer('full', 'Net Explorer Full');
explorer('both', 'Net Explorer Twin Engines');

// ---- proof page ----------------------------------------------------------------
{
  const body = `
<header><span class="kicker">Composition proof</span><h1>An order saga assembled from three parts</h1>
<p class="lede">Three small models (an order queue, a worker pool and a payment saga) are joined by named connections. This page checks the joins, replays what happens when an order is paid or fails, and re-checks an algebraic proof about the payment steps. Nothing here talks to a server.</p></header>
<section><h2>The parts and how they connect</h2><div class="marks" id="parts"></div><ul class="glue" id="glue"></ul><ul class="checks" id="glueChecks" style="margin-top:12px"></ul></section>
<section><h2>Does the whole behave as specified?</h2><p class="lede" id="scenSummary" style="margin-bottom:10px"></p><div class="checks" id="scenarios" style="display:flex;flex-direction:column;gap:8px"></div></section>
<section><h2>Rewrite proof about the payment steps <span id="chypms" class="mono" style="font-weight:400;text-transform:none;letter-spacing:0"></span></h2>
<div class="check" id="verdict"><span class="badge">…</span><div><b>Checking…</b></div></div>
<div class="cols" style="margin-top:12px"><div><textarea id="theory" spellcheck="false" aria-label="Chyp theory" style="min-height:360px"></textarea>
<div class="row" style="margin-top:8px"><button id="check" type="button" class="primary">Check proof</button><button id="break" type="button">Break one step</button><button id="restore" type="button">Restore</button></div></div>
<div style="min-width:0"><ul class="parts" id="plist"></ul><div class="log" id="partinfo" style="margin-top:8px"></div><div class="diagram" id="diagram"></div></div></div>
<p class="log">This proof and the net checks above are two separate statements that share step names. No tool yet derives one from the other.</p></section>
<footer id="foot"></footer>`;
  const bytes = `${kb(Buffer.byteLength(fullSrc))} + chyp ${kb(Buffer.byteLength(chypSrc) + chypGz.length)}`;
  const script = [FULL, CHYP, `const CHYP_GZ_B64=${safeJson(chypGz)},THEORY=${safeJson(readFileSync(join(here, 'saga.chyp'), 'utf8'))},ENGINE_BYTES=${safeJson(bytes)},PAGE_BYTES=Number("0000000000");`, readFileSync(join(here, 'client-proof.js'), 'utf8')].join('\n');
  finish('saga-proof.html', shell('Order Saga Proof', body, script), bytes);
}
