#!/usr/bin/env node
// pflow-mcp: the pflow-rs Petri net engine (compiled to WebAssembly) as a local
// MCP server over stdio. No native build, no network after install.
//
//   npx @pflow-xyz/pflow-mcp
//
// Nets live in memory for the life of the process, keyed by name.
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import { readFileSync, writeFileSync } from 'node:fs';
import { register as registerHosted, reachabilityGuarded, timeoutArg, withMarking } from './hosted.mjs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
// The engine with the pflow.xyz/mcp tools built in (make wasm-mcp-engine). The
// browser build, ./pflow-wasm.inline.js, is only read as text for artifact_html.
import init, { Net, hosted } from './pflow-wasm-hosted.inline.js';

await init();

const nets = new Map();
const models = new Map(); // name -> model object as loaded, for artifact_html
// name -> what the pflow.xyz-named tools read: { doc } (the JSON-LD form of the
// loaded net, sealed with its @id when the CID was computed) or { unsupported }
// (why it cannot be written as one), plus cid and layout.
const jsonld = new Map();
let last = null;

// The package has its own version (build.mjs); the engine's is version().
const pkgVersion = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')).version;
const server = new McpServer({ name: 'pflow-mcp', version: pkgVersion });

const ok = (value) => ({ content: [{ type: 'text', text: JSON.stringify(value, null, 2) }] });
const fail = (e) => ({ isError: true, content: [{ type: 'text', text: String(e?.message ?? e) }] });
const get = (name) => {
  const key = name ?? last;
  if (key == null) throw new Error('no net loaded: call load_net first');
  const net = nets.get(key);
  if (!net) throw new Error(`no net named "${key}"; loaded: ${[...nets.keys()].join(', ') || 'none'}`);
  return net;
};
const tool = (name, description, shape, fn) =>
  server.registerTool(name, { description, inputSchema: shape }, async (args) => {
    try { return ok(await fn(args)); } catch (e) { return fail(e); }
  });

const netName = z.string().optional().describe('Net name; defaults to the most recently loaded');
const rates = z.record(z.string(), z.number().nullable()).optional()
  .describe('Per-transition rate overrides. 0 switches the transition off (it never fires); null means unset (the net\'s own rate, else 1)');

const snapshot = (net) => ({
  places: net.places(), transitions: net.transitions(),
  marking: net.marking(), enabled: net.enabled(),
});

tool('load_net',
  'Load a Petri net: pflow.xyz JSON-LD, or the array shape (places: [{id, initial}], arcs: [{from, to, weight, type}]). Replaces any net with the same name. Every other tool then reads it by name, whichever shape it came in. Also answers shape, layout ("given", or "synthesized" when the net had no coordinates), and the net\'s CID: the CID names the document, layout and arc order included. It is computed here for up to 50 places; a larger net answers cid null with cid_skipped, and the cid tool computes it on demand. A net that cannot be written as a pflow.xyz document (data places, constraints, timed transitions) answers unsupported and still runs locally. Nets with guards, stages or rate schedules, and arcs with a fractional weight, are refused.',
  { model: z.union([z.string(), z.record(z.string(), z.any())]).describe('The net as a JSON object or JSON text'),
    name: z.string().optional().describe('Name to keep it under; defaults to the model name, else "net"') },
  ({ model, name }) => {
    const obj = typeof model === 'string' ? JSON.parse(model) : model;
    const key = name ?? obj?.name ?? 'net';
    const net = new Net(model);
    let conv;
    try {
      conv = hosted('net_to_jsonld', { model: obj, name: key });
    } catch (e) { net.free?.(); throw e; }
    nets.get(key)?.free?.();
    nets.set(key, net);
    models.set(key, obj);
    jsonld.set(key, { doc: conv.net, cid: conv.cid, unsupported: conv.unsupported });
    last = key;
    const { net: _doc, places: _n, ...about } = conv;
    return { name: key, ...snapshot(net), ...about };
  });

tool('list_nets', 'List loaded nets.', {},
  () => ({ current: last, nets: [...nets.keys()] }));

tool('state',
  'Places, transitions, current marking and enabled transitions. For any loaded net `info` adds the hosted pflow_info summary: name, colours, each place (initial, capacity, label), transition (rate, label) and arc; its marking and enabled are the initial ones.',
  { net: netName }, ({ net }) => {
    const out = snapshot(get(net));
    const entry = jsonld.get(net ?? last);
    if (entry?.doc) out.info = hosted('info', { net: entry.doc });
    else if (entry?.unsupported) out.info_unavailable = entry.unsupported;
    return out;
  });

tool('fire', 'Fire one enabled transition and return the new marking. Errors say why it is not enabled.',
  { transition: z.string(), net: netName },
  ({ transition, net }) => {
    const n = get(net);
    const marking = n.fire(transition);
    return { marking, enabled: n.enabled() };
  });

tool('reset', 'Return a net to its initial marking.', { net: netName },
  ({ net }) => { const n = get(net); n.reset(); return snapshot(n); });

tool('simulate',
  'Deterministic mass-action ODE (Tsit5) from the current marking. Does not move the marking.',
  { net: netName,
    tEnd: z.number().optional().describe('End time (default 100)'),
    maxPoints: z.number().int().optional().describe('Max samples (default 200)'),
    rates,
    contextual: z.enum(['refuse', 'selfloop', 'drop']).optional()
      .describe('How read and inhibitor arcs are treated (default refuse)') },
  ({ net, ...options }) => get(net).simulate(options));

tool('stochastic',
  'Gillespie SSA from the current marking; byte-exact with go-pflow at the same seed. Does not move the marking.',
  { net: netName,
    horizon: z.number().optional().describe('End time (default 10)'),
    samples: z.number().int().optional().describe('Time samples (default 101)'),
    realizations: z.number().int().optional().describe('Ensemble size (default 1)'),
    seed: z.number().int().optional().describe('RNG seed (default 1)'),
    rates },
  ({ net, ...options }) => get(net).stochastic(options));

tool('reachability',
  'Explore the reachable markings (pflow-rs reachability, untimed: rates play no part), with parity to pflow.xyz/mcp pflow_reachability: state and edge count, bounded or not, dead markings with the first, transitions that never fire, and three verdicts, each true, false or "unknown": deadlock_free (no reachable marking has nothing enabled, an empty one included), live (L4: from every reachable marking, every transition can still fire) and quasi_live (L1: every transition fires in some reachable marking, which a net with a deadlock can pass). witness is a firing sequence that replays from `from` to the dead marking, or into a closed region shutting transitions out, with never_fires naming them. When complete is false the search stopped at its bound (truncated says why): no verdict is true, and only a witness makes one false. Pass a loaded net by name, or a model inline. Does not move the marking.',
  { net: netName,
    model: z.union([z.string(), z.record(z.string(), z.any())]).optional()
      .describe('A net to analyze without loading it (pflow.xyz JSON object or text); overrides net'),
    max_states: z.number().int().min(1).max(20000).optional().describe('State budget (default 2000, ≤ 20000)'),
    max_tokens: z.number().int().min(1).optional()
      .describe('Tokens in one place past which the net is called unbounded (default 1000)'),
    from: z.enum(['initial', 'current']).optional()
      .describe('Start from the initial marking (default, as the hosted tool) or a loaded net\'s current one'),
    timeout_ms: timeoutArg },
  async ({ net, model, max_states, max_tokens, from, timeout_ms }) => {
    // Runs in a worker thread (hosted.mjs): a search past timeout_ms is stopped and answers unknown.
    const options = { maxStates: max_states, maxTokens: max_tokens, from };
    if (model != null) {
      if (from === 'current') throw new Error('from "current" needs a loaded net, not an inline model');
      return reachabilityGuarded(model, options, timeout_ms);
    }
    const loaded = get(net);
    let m = models.get(net ?? last);
    if (from === 'current') {
      // the worker has no marking of its own: hand it the current one as the initial
      m = withMarking(m, loaded.marking());
      if (!m) throw new Error('from "current": this net\'s shape cannot carry its marking to the search');
      options.from = 'initial';
    }
    return reachabilityGuarded(m, options, timeout_ms);
  });

// ---- artifact_html -------------------------------------------------------

// The wasm package's inline entry is an ES module; strip its exports so it can
// sit in a page's own <script type="module"> with no import and no fetch.
const engineSource = () =>
  readFileSync(new URL('./pflow-wasm.inline.js', import.meta.url), 'utf8')
    .replace(/^export default function init/m, 'function init')
    .replace(/^export class Net/m, 'class Net')
    .replace(/^export function version/m, 'function version')
    .replace(/^export \{ syncInit as initSync \};\s*$/m, '')
    .replace(/<\/script/gi, '<\\/script');

const PAGE_CSS = `
:root{--bg:#fff;--fg:#1c1c1e;--mut:#6b6b70;--line:#d8d8dc;--acc:#2563eb;--card:#f6f6f8}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#16161a;--fg:#ececf0;--mut:#9a9aa3;--line:#33333a;--acc:#6ea0ff;--card:#202026}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif}
main{max-width:860px;margin:0 auto;padding:20px 16px}h1{font-size:20px;margin:0 0 12px}
section{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px;margin:0 0 14px}
h2{font-size:12px;letter-spacing:.06em;text-transform:uppercase;color:var(--mut);margin:0 0 10px}
.marks{display:flex;flex-wrap:wrap;gap:8px}.mark{border:1px solid var(--line);border-radius:8px;padding:6px 12px;background:var(--bg)}
.mark b{font-size:20px;display:block;line-height:1.1}.mark span{color:var(--mut);font-size:12px}
button{font:inherit;color:var(--fg);background:var(--bg);border:1px solid var(--line);border-radius:8px;padding:6px 12px;cursor:pointer;margin:0 6px 6px 0}
button:hover:not(:disabled){border-color:var(--acc);color:var(--acc)}button:disabled{opacity:.4;cursor:default}
label{margin-right:12px;color:var(--mut);font-size:13px}input{font:inherit;width:70px;background:var(--bg);color:var(--fg);border:1px solid var(--line);border-radius:6px;padding:3px 6px}
svg{width:100%;height:auto;display:block}.leg{display:flex;flex-wrap:wrap;gap:12px;font-size:13px;color:var(--mut);margin-top:6px}
.sw{display:inline-block;width:10px;height:10px;border-radius:2px;margin-right:4px}#log{color:var(--mut);font-size:13px;min-height:1.4em}
`;

const PAGE_JS = `
const COLORS=['#2563eb','#dc2626','#16a34a','#d97706','#9333ea','#0891b2','#be185d','#65a30d'];
const $=id=>document.getElementById(id);
const esc=s=>String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
const num=(id,d)=>{const v=$(id).value;return v===''||isNaN(+v)?d:+v};
await init();
const net=new Net(MODEL);
const places=net.places();
function render(){
  const m=net.marking(), en=new Set(net.enabled());
  $('marks').innerHTML=places.map(p=>'<div class="mark"><b>'+m[p]+'</b><span>'+esc(p)+'</span></div>').join('');
  $('tx').innerHTML='';
  for(const t of net.transitions()){const b=document.createElement('button');b.textContent=t;b.disabled=!en.has(t);
    b.onclick=()=>{try{net.fire(t);$('log').textContent='fired '+t}catch(e){$('log').textContent=String(e.message||e)}render()};$('tx').appendChild(b)}
}
function chart(res){
  const W=800,H=300,L=44,B=28,T=8,R=8,ts=res.times;
  // ODE series are arrays, SSA series are {values, stddev}; an array has its own .values (a function), so test the type
  const ser=p=>Array.isArray(res.series[p])?res.series[p]:res.series[p].values;
  let max=0;for(const p of places)for(const v of ser(p))max=Math.max(max,v);max=max||1;
  const x=t=>L+(t-ts[0])/((ts[ts.length-1]-ts[0])||1)*(W-L-R), y=v=>T+(1-v/max)*(H-T-B);
  let s='<svg viewBox="0 0 '+W+' '+H+'" role="img" aria-label="token counts over time">';
  for(let i=0;i<=4;i++){const v=max*i/4,yy=y(v);s+='<line x1="'+L+'" x2="'+(W-R)+'" y1="'+yy+'" y2="'+yy+'" stroke="var(--line)"/><text x="'+(L-6)+'" y="'+(yy+4)+'" text-anchor="end" font-size="11" fill="var(--mut)">'+(+v.toFixed(2))+'</text>'}
  for(const i of [0,.5,1]){const t=ts[0]+(ts[ts.length-1]-ts[0])*i;s+='<text x="'+x(t)+'" y="'+(H-8)+'" text-anchor="middle" font-size="11" fill="var(--mut)">'+(+t.toFixed(2))+'</text>'}
  places.forEach((p,k)=>{const vals=ser(p);
    s+='<polyline fill="none" stroke="'+COLORS[k%8]+'" stroke-width="2" points="'+vals.map((v,i)=>x(ts[i]).toFixed(1)+','+y(v).toFixed(1)).join(' ')+'"/>'});
  $('chart').innerHTML=s+'</svg>';
  $('legend').innerHTML=places.map((p,k)=>'<span><i class="sw" style="background:'+COLORS[k%8]+'"></i>'+esc(p)+'</span>').join('');
}
function run(kind){
  const t=num('t',10);
  try{chart(kind==='ode'?net.simulate({tEnd:t,maxPoints:120}):net.stochastic({horizon:t,samples:120,realizations:num('n',1),seed:num('seed',1)}));$('log').textContent=kind==='ode'?'ODE from current marking':'SSA from current marking'}
  catch(e){$('log').textContent=String(e.message||e)}
}
$('reset').onclick=()=>{net.reset();render();run('ode')};
$('ode').onclick=()=>run('ode');$('ssa').onclick=()=>run('ssa');
render();run('ode');
`;

const buildPage = (title, model) => `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${title.replace(/[<&]/g, '')}</title><style>${PAGE_CSS}</style></head><body><main>
<h1>${title.replace(/[<&]/g, '')}</h1>
<section><h2>Marking</h2><div class="marks" id="marks"></div></section>
<section><h2>Fire</h2><div id="tx"></div><button id="reset">Reset</button><div id="log"></div></section>
<section><h2>Over time</h2>
<label>t <input id="t" type="number" value="10"></label><label>runs <input id="n" type="number" value="20"></label><label>seed <input id="seed" type="number" value="1"></label>
<div style="margin-top:8px"><button id="ode">Run ODE</button><button id="ssa">Run SSA</button></div>
<div id="chart"></div><div class="leg" id="legend"></div></section>
</main>
<script type="module">
${engineSource()}
const MODEL=${JSON.stringify(model).replace(/</g, '\\u003c')};
${PAGE_JS}
</script></body></html>
`;

tool('artifact_html',
  'Write a self-contained interactive HTML page for a loaded net: click-to-fire token game plus ODE/SSA charts. The pflow WebAssembly engine is embedded, so the page makes no network requests and runs inside a sandboxed artifact. Returns the file path; publish that file as an artifact (the HTML is not returned inline: it is ~250 KB).',
  { net: netName,
    title: z.string().optional().describe('Page title (default: the net name)'),
    path: z.string().optional().describe('Where to write it (default: a temp file)'),
    inline: z.boolean().optional().describe('Also return the HTML text. Large; off by default.') },
  ({ net, title, path, inline }) => {
    const key = net ?? last;
    get(net);
    const model = models.get(key);
    const html = buildPage(title ?? model?.name ?? key, model);
    const file = path ?? join(tmpdir(), `pflow-${String(key).replace(/[^\w.-]+/g, '_')}.html`);
    writeFileSync(file, html);
    return { path: file, bytes: html.length, ...(inline ? { html } : {}) };
  });

import { register as registerOntology } from './ontology.mjs';
import { register as registerChyp } from './chyp.mjs';
import { register as registerCompose } from './compose.mjs';
import { register as registerExamples } from './examples.mjs';
registerOntology(tool);
registerChyp(tool);
registerHosted(tool, { hosted, jsonld, last: () => last });
registerCompose(tool);

await server.connect(new StdioServerTransport());
