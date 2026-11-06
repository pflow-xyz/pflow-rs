// The page shell shared by artifact_html (every view) and compose_canvas (the
// footer). A page is one self-contained HTML file with the wasm engine inside,
// so it runs in a sandboxed Claude artifact: no network request, no external
// script, no iframe. It carries the net as JSON-LD in
// <script type="application/json" id="pflow-net">, so a session that reads the
// published artifact can parse the net out of it and load_net it.
import { readFileSync } from 'node:fs';
import { getViews } from './views/index.mjs';

const here = (p) => new URL(p, import.meta.url);
export const EDITOR = 'https://pflow.xyz/';

const escHtml = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const safeJson = (v) => JSON.stringify(v).replace(/</g, '\\u003c').replace(/[\u2028\u2029]/g, (c) => '\\u' + c.charCodeAt(0).toString(16));

// Versions the footer states: this package, the engine, the ontology snapshot.
export function readMeta(engineVersion) {
  const read = (p) => { try { return JSON.parse(readFileSync(here(p), 'utf8')); } catch { return null; } };
  const onto = read('./ontology/meta.json');
  return {
    pkg: read('./package.json')?.version ?? 'unknown',
    engine: engineVersion ?? 'unknown',
    ontology: onto ? `${onto.version ?? 'unknown'}${onto.commit ? ` (${onto.commit})` : ''}${onto.dirty ? ' dirty' : ''}` : 'unknown',
  };
}

// The wasm package's inline entry is an ES module; strip its exports so it can
// sit in a page's own <script type="module"> with no import and no fetch.
let engineText;
const engineSource = () => engineText ??=
  readFileSync(here('./pflow-wasm.inline.js'), 'utf8')
    .replace(/^export default function init/m, 'function init')
    .replace(/^export class Net/m, 'class Net')
    .replace(/^export function version/m, 'function version')
    .replace(/^export \{ syncInit as initSync \};\s*$/m, '')
    .replace(/<\/script/gi, '<\\/script');

// Colour tokens: light by default, dark by media query unless the page is
// forced light, and dark when forced dark.
const TOKENS = `
:root{--bg:#fff;--fg:#1c1c1e;--mut:#6b6b70;--line:#d8d8dc;--acc:#2563eb;--card:#f6f6f8}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#16161a;--fg:#ececf0;--mut:#9a9aa3;--line:#33333a;--acc:#6ea0ff;--card:#202026}}
:root[data-theme=dark]{--bg:#16161a;--fg:#ececf0;--mut:#9a9aa3;--line:#33333a;--acc:#6ea0ff;--card:#202026}
`;

// The footer's own styles use fallbacks, so the same footer sits in a page that
// does not define the tokens (the composition canvas).
export const FOOTER_CSS = `
.pf-foot{max-width:860px;margin:18px auto 0;padding:12px 16px 24px;border-top:1px solid var(--line,#d8d8dc);color:var(--mut,#6b6b70);font:12px/1.6 system-ui,sans-serif;overflow-wrap:anywhere}
.pf-foot code{font:12px ui-monospace,monospace;word-break:break-all;color:var(--fg,#1c1c1e)}
.pf-foot a{color:var(--acc,#2563eb)}.pf-foot .pf-row{margin:2px 0}
.pf-foot button{font:inherit;color:var(--fg,#1c1c1e);background:var(--bg,#fff);border:1px solid var(--line,#d8d8dc);border-radius:8px;padding:6px 12px;cursor:pointer;margin:6px 6px 0 0}
.pf-foot button:hover{border-color:var(--acc,#2563eb);color:var(--acc,#2563eb)}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]) .pf-foot{--mut:#9a9aa3;--line:#33333a;--fg:#ececf0;--acc:#6ea0ff;--bg:#16161a}}
`;

// ctx: { cid, cidNote, meta, seed, composeCid, netLinks, actions }
export function footerHtml({ cid, cidNote, meta, seed, composeCid, editorLink, actions, published, publishedMatches }) {
  const rows = [];
  if (cid !== undefined) {
    rows.push(`<div class="pf-row">document CID: ${cid ? `<code id="pf-cid">${escHtml(cid)}</code>` : `<span id="pf-cid" title="${escHtml(cidNote ?? '')}">not computed</span>`}${cid && cidNote ? ` <span title="${escHtml(cidNote)}">(names the document, layout included)</span>` : ''}</div>`);
  }
  rows.push(`<div class="pf-row">pflow-mcp ${escHtml(meta.pkg)} &middot; engine ${escHtml(meta.engine)} &middot; ontology snapshot ${escHtml(meta.ontology)}${seed != null ? ` &middot; seed <span id="f-seed">${escHtml(seed)}</span>` : ''}</div>`);
  const links = [];
  // a built-in example links to the CID pflow.xyz publishes it under; the local CID is shown with the mismatch
  const target = published || cid;
  if (published && publishedMatches === false) {
    rows.push(`<div class="pf-row" id="pf-published">pflow.xyz publishes this net as <code>${escHtml(published)}</code>; this engine seals the same document to the CID above, so the link below opens the published copy.</div>`);
  }
  if (editorLink && target) links.push(`<a href="${EDITOR}?cid=${encodeURIComponent(target)}" target="_blank" rel="noopener">Open this net in the pflow.xyz editor</a>`);
  if (composeCid) links.push(`<a href="${EDITOR}compose.html?cid=${encodeURIComponent(composeCid)}" target="_blank" rel="noopener">Open this composition on pflow.xyz</a>`);
  if (links.length) rows.push(`<div class="pf-row">${links.join(' &middot; ')}</div>`);
  return `<footer class="pf-foot">${rows.join('')}${actions ?? ''}</footer>`;
}

const SHELL_CSS = `
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif}
main{max-width:860px;margin:0 auto;padding:20px 16px 0;overflow-wrap:anywhere}h1{font-size:20px;margin:0 0 12px}
section{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px;margin:0 0 14px}
h2{font-size:12px;letter-spacing:.06em;text-transform:uppercase;color:var(--mut);margin:0 0 10px}
button{font:inherit;color:var(--fg);background:var(--bg);border:1px solid var(--line);border-radius:8px;padding:6px 12px;cursor:pointer;margin:0 6px 6px 0}
button:hover:not(:disabled){border-color:var(--acc);color:var(--acc)}button:disabled{opacity:.4;cursor:default}
details.pf-text{max-width:860px;margin:0 auto;padding:0 16px}details.pf-text summary{cursor:pointer;color:var(--mut);font-size:13px}
details.pf-text textarea{width:100%;height:12em;margin-top:8px;font:12px ui-monospace,monospace;background:var(--bg);color:var(--fg);border:1px solid var(--line);border-radius:6px}
#pf-err{color:#dc2626;font-size:13px}
svg{width:100%;height:auto;display:block}.leg{display:flex;flex-wrap:wrap;gap:12px;font-size:13px;color:var(--mut);margin-top:6px}
.sw{display:inline-block;width:10px;height:10px;border-radius:2px;margin-right:4px}
`;

const PRELUDE = `
const $=id=>document.getElementById(id);
const esc=s=>String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
const num=(id,d)=>{const v=$(id).value;return v===''||isNaN(+v)?d:+v};
const setSeed=n=>{const e=$('f-seed');if(e)e.textContent=String(n)};
const netText=$('pflow-net').textContent;
const netDoc=JSON.parse(netText);
const pretty=JSON.stringify(netDoc,null,2);
$('pf-download').onclick=()=>{
  try{const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([pretty],{type:'application/ld+json'}));
    a.download=(netDoc.name||'net').toString().replace(/[^\\w.-]+/g,'_')+'.jsonld';document.body.appendChild(a);a.click();a.remove()}
  catch(e){$('pf-err').textContent='Download is blocked here; use Copy as text.'}
};
$('pf-copy').onclick=async()=>{
  const ta=$('pf-ta');ta.value=pretty;
  try{await navigator.clipboard.writeText(pretty);$('pf-err').textContent='Copied.'}
  catch(e){$('pf-text').open=true;ta.focus();ta.select();
    let done=false;try{done=document.execCommand('copy')}catch(_){}
    $('pf-err').textContent=done?'Copied.':'Copy is blocked here; the text is selected below.'}
};
`;

// Build a page for the loaded net. net = { key, title, model, doc, cid, cidNote };
// viewIds default to ["play"]; unknown ids throw with the known list.
export function buildPage({ net, viewIds, meta, engine }) {
  const views = getViews(viewIds);
  const ctx = { key: net.key, title: net.title, model: net.model, doc: net.doc, cid: net.cid, engine, meta };
  const parts = views.map((v) => ({ id: v.id, ...v.buildSection(ctx) }));
  const seed = parts.find((p) => p.seed != null)?.seed;
  const title = escHtml(net.title).slice(0, 200);
  // What the page reads from: the JSON-LD document when the net can be one,
  // else the net as it was loaded (the page then says nothing about a CID).
  const payload = net.doc ?? net.model;
  const actions = `<div><button id="pf-download">Download net</button><button id="pf-copy">Copy as text</button><span id="pf-err" role="status"></span></div>`;
  const footer = footerHtml({ cid: net.cid ?? null, cidNote: net.cidNote, meta, seed, editorLink: !!net.doc, actions,
    published: net.published, publishedMatches: net.publishedMatches });
  const scripts = parts.map((p) => `try{\n{\n${p.js ?? ''}\n}\n}catch(e){$('pf-err').textContent='view ${p.id}: '+String(e&&e.message||e)}`).join('\n');
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${title}</title><style>${TOKENS}${SHELL_CSS}${parts.map((p) => p.css ?? '').join('')}${FOOTER_CSS}</style></head><body><main>
<h1>${title}</h1>
${parts.map((p) => `<div data-view="${p.id}">${p.html}</div>`).join('\n')}
</main>
<details class="pf-text" id="pf-text"><summary>Net as JSON-LD text</summary><textarea id="pf-ta" readonly aria-label="net as JSON-LD"></textarea></details>
${footer}
<script type="application/json" id="pflow-net">${safeJson(payload)}</script>
<script type="module">
${engineSource()}
${PRELUDE}
await init();
${scripts}
</script></body></html>
`;
}
