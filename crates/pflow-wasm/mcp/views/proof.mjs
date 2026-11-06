// The proof view: the standard properties of the net, each as proved, refuted or
// unknown with the engine's evidence, and a "Copy proof as text" record someone
// else can paste into a message.
//
// Where the verdicts come from. The page's own engine is the browser build,
// which has no verify; the server's engine is the hosted build, which does. So
// buildSection asks it (ctx.engine.hosted('verify', ...), the same call as the
// `verify` MCP tool, same net, same properties, same budget) and the page shows
// that result as it came back. Nothing here decides a verdict. On the page the
// embedded engine then replays every counterexample trace from the initial
// marking and says whether it reaches the marking the verdict claims.
//
// For a net that is the flattened composite of a bundle (ids written
// <subnet>/<element>, which is what bundle_net writes) the page also holds the
// engine-free proof re-checker, verify-record.mjs: a byte copy of
// ../../compose/verify.mjs (test-proof.mjs fails if they drift). It checks a
// bundle_prove record by arithmetic and replay, with no engine call.
import { readFileSync } from 'node:fs';

const PROPERTIES = [
  { kind: 'deadlock-free' }, { kind: 'bounded' }, { kind: 'live' },
  { kind: 'l4-live' }, { kind: 'terminating' }, { kind: 'conserves' },
];
export const MAX_STATES = 5000;

const safeJson = (v) => JSON.stringify(v).replace(/</g, '\\u003c').replace(/[\u2028\u2029]/g, (c) => '\\u' + c.charCodeAt(0).toString(16));
const escHtml = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

let checkerSource;
const checker = () => checkerSource ??= readFileSync(new URL('./verify-record.mjs', import.meta.url), 'utf8').replace(/^export /gm, '').replace(/<\/script/gi, '<\\/script');

// Ids of a bundle_net composite look like "orders/items": every place has a "/".
export function looksComposite(doc) {
  const ids = doc && doc.places && typeof doc.places === 'object' ? Object.keys(doc.places) : [];
  return ids.length > 0 && ids.every((p) => p.includes('/'));
}

const css = `
.pv-sum{color:var(--mut);font-size:13px;margin:0 0 10px}
.pv-v{border:1px solid var(--line);border-radius:8px;background:var(--bg);padding:10px 12px;margin:0 0 8px}
.pv-h{display:flex;flex-wrap:wrap;gap:8px;align-items:baseline}.pv-h b{font-size:15px}
.pv-s{font-size:12px;font-weight:700;letter-spacing:.04em;text-transform:uppercase;border-radius:999px;padding:1px 9px;border:1px solid currentColor}
.pv-proved{color:#15803d}.pv-refuted{color:#b91c1c}.pv-unknown{color:#b45309}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]) .pv-proved{color:#4ade80}:root:not([data-theme=light]) .pv-refuted{color:#f87171}:root:not([data-theme=light]) .pv-unknown{color:#fbbf24}}
:root[data-theme=dark] .pv-proved{color:#4ade80}:root[data-theme=dark] .pv-refuted{color:#f87171}:root[data-theme=dark] .pv-unknown{color:#fbbf24}
.pv-m{color:var(--mut);font-size:12px}.pv-d{margin:6px 0 0;font-size:14px}
.pv-e{margin:6px 0 0;font:12px/1.5 ui-monospace,monospace;color:var(--mut);white-space:pre-wrap;overflow-wrap:anywhere}
.pv-t{margin:6px 0 0;font-size:13px;overflow-wrap:anywhere}.pv-t code{font:12px ui-monospace,monospace}
.pv-ok{color:#15803d}.pv-bad{color:#b91c1c}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]) .pv-ok{color:#4ade80}:root:not([data-theme=light]) .pv-bad{color:#f87171}}
:root[data-theme=dark] .pv-ok{color:#4ade80}:root[data-theme=dark] .pv-bad{color:#f87171}
.pv-note{color:var(--mut);font-size:13px;margin:8px 0 0}
#pv-text{width:100%;height:11em;margin-top:8px;font:12px ui-monospace,monospace;background:var(--bg);color:var(--fg);border:1px solid var(--line);border-radius:6px}
.pv-in{width:100%;height:7em;font:12px ui-monospace,monospace;background:var(--bg);color:var(--fg);border:1px solid var(--line);border-radius:6px;margin:0 0 8px}
.pv-lab{display:block;color:var(--mut);font-size:13px;margin:6px 0 2px}
.pv-checks{list-style:none;margin:8px 0 0;padding:0;font-size:13px}.pv-checks li{margin:2px 0;overflow-wrap:anywhere}
`;

const recheckHtml = `
<section id="pv-recheck"><h2>Re-checking a composition's proof record</h2>
<p class="pv-d">This net looks like the flattened composite of a bundle (its place ids are written <code>&lt;subnet&gt;/&lt;element&gt;</code>). A proof record for the bundle (<code>bundle_prove</code>) is re-checked in three ways. <code>bundle_check_proof</code> re-checks it with the engine. The compose canvas page re-checks it twice, once with the engine and once with a separate script. And you can re-check it right here with that separate script alone: it does arithmetic and replays traces on this net, and calls no engine.</p>
<label class="pv-lab" for="pv-rec">the <code>bundle_prove</code> answer (JSON)</label><textarea class="pv-in" id="pv-rec" spellcheck="false" aria-label="bundle_prove answer"></textarea>
<label class="pv-lab" for="pv-mem">the <code>bundle_net</code> answer, or just its <code>members</code> (JSON)</label><textarea class="pv-in" id="pv-mem" spellcheck="false" aria-label="bundle_net members"></textarea>
<button id="pv-run">Re-check here, engine-free</button><span id="pv-rstat" role="status"></span>
<ul class="pv-checks" id="pv-checks"></ul>
<p class="pv-note">What it does not do: flatten the bundle (that is the engine's) or re-run the exhaustive searches. A refuted deadlock and a counterexample carry witnesses, which it replays in full. Searches that finished are listed as skipped, not as passed.</p>
</section>`;

const html = (composite) => `
<section id="pv-proof"><h2>Proof</h2>
<div id="pv-body"><p class="pv-d">Loading verdicts.</p></div>
<div style="margin-top:10px"><button id="pv-copy">Copy proof as text</button><span id="pv-status" role="status"></span></div>
<details><summary class="pv-m" style="cursor:pointer">The record as plain text</summary><textarea id="pv-text" readonly aria-label="proof record as text"></textarea></details>
</section>${composite ? recheckHtml : ''}`;

function buildJs({ result, error, ctx, composite }) {
  const data = {
    result: result ?? null, error: error ?? null, maxStates: MAX_STATES, properties: PROPERTIES,
    title: ctx.title, cid: ctx.cid ?? null, meta: ctx.meta, composite,
  };
  return `
const PV=${safeJson(data)};
const pvNet=new Net(netDoc);
const pvPlaces=pvNet.places();
// Replay a trace from the initial marking on the page's engine; return what it reached.
function pvReplay(trace){
  pvNet.reset();
  try{for(const t of trace)pvNet.fire(t);return {marking:pvNet.marking(),enabled:pvNet.enabled()}}
  catch(e){return {error:String(e&&e.message||e)}}
  finally{pvNet.reset()}
}
const pvMark=m=>{const k=Object.keys(m).filter(p=>m[p]!==0);return k.length?k.map(p=>p+'='+m[p]).join(', '):'empty'};
function pvCheck(v){
  const cex=v.counterexample;if(!cex||!Array.isArray(cex.trace))return null;
  const r=pvReplay(cex.trace);
  if(r.error)return {ok:false,text:'trace did not replay: '+r.error};
  const claimed=cex.marking||{};
  const same=pvPlaces.every(p=>(r.marking[p]||0)===(claimed[p]||0));
  return {ok:same,enabled:r.enabled,text:same?'replayed on the embedded engine from the initial marking: it reaches the stated marking; enabled there: '+(r.enabled.length?r.enabled.join(', '):'none')
    :'replayed on the embedded engine: it reaches '+pvMark(r.marking)+', not the stated marking'};
}
const R=PV.result;
// why the engine stopped: its own reason when it gave one, else the state budget
const pvWhy=()=>R&&R.truncated_reason?' ('+R.truncated_reason+'; '+R.state_count+' states explored, budget '+PV.maxStates+')':' (budget '+PV.maxStates+' states)';
const rows=R?(R.verdicts||[]).map(v=>({v,chk:pvCheck(v)})):[];
const cnt=s=>rows.filter(x=>x.v.status===s).length;
function pvText(){
  const L=[];
  L.push('pflow proof record');
  L.push('net: '+PV.title);
  L.push('net CID: '+(PV.cid||'not computed (the net is too large to seal quickly)'));
  L.push('engine: pflow-wasm '+PV.meta.engine+' (pflow-mcp '+PV.meta.pkg+', ontology snapshot '+PV.meta.ontology+')');
  if(!R){L.push('verify: not run: '+PV.error);return L.join('\\n')}
  L.push('search: exhaustive up to '+PV.maxStates+' states; '+R.state_count+' explored; '+(R.truncated?'TRUNCATED (the search was bounded)':'complete'));
  L.push('properties: '+PV.properties.map(p=>p.kind).join(', '));
  L.push('');
  rows.forEach(({v,chk},i)=>{
    L.push((i+1)+'. '+v.property.kind+': '+v.status.toUpperCase()+(v.method?' ('+v.method+')':''));
    if(v.detail)L.push('   '+v.detail);
    if(v.evidence)L.push('   evidence: '+v.evidence);
    const c=v.counterexample;
    if(c){
      if(c.explanation)L.push('   '+c.explanation);
      if(c.trace)L.push('   counterexample: '+(c.trace.length?c.trace.join(' -> '):'(initial marking)'));
      if(c.marking)L.push('   ends at: '+pvMark(c.marking));
      if(chk)L.push('   '+(chk.ok?'replay: reproduced':'replay: NOT reproduced')+' ('+chk.text+')');
    }
    if(v.status==='unknown')L.push('   unknown is not a pass: the search was bounded'+pvWhy()+'.');
    if(v.property.kind==='deadlock-free')L.push('   note: verify treats a marking with nothing enabled as finished only when it is empty; the reachability verdict in the analysis section counts any such marking as a deadlock, so the two can differ.');
  });
  L.push('');
  L.push('summary: '+cnt('proved')+' proved, '+cnt('refuted')+' refuted, '+cnt('unknown')+' unknown');
  L.push('Verdicts are the pflow engine\\'s, not a reading of the net. To re-check: load the net (pflow.xyz JSON-LD'+(PV.cid?', CID '+PV.cid:'')+') in pflow-mcp and call verify with the same properties and max_states '+PV.maxStates+', or fire a counterexample trace by hand.');
  if(PV.composite)L.push('This net looks like a flattened composition: a bundle_prove record for it is re-checked with bundle_check_proof (engine) or with the engine-free checker on the compose canvas page.');
  return L.join('\\n');
}
function pvRender(){
  const b=$('pv-body');
  if(!R){b.innerHTML='<p class="pv-d">Verification did not run: '+esc(PV.error||'no result')+'</p>';return}
  let s='<p class="pv-sum" id="pv-sum" data-proved="'+cnt('proved')+'" data-refuted="'+cnt('refuted')+'" data-unknown="'+cnt('unknown')+'" data-states="'+esc(R.state_count)+'" data-truncated="'+!!R.truncated+'">'+cnt('proved')+' proved, '+cnt('refuted')+' refuted, '+cnt('unknown')+' unknown. Search budget '+PV.maxStates+' states; '+esc(R.state_count)+' explored'+(R.truncated?', truncated: the search was bounded'+esc(R.truncated_reason?' ('+R.truncated_reason+')':'')+'.':', complete.')+'</p>';
  for(const {v,chk} of rows){
    const st=['proved','refuted'].includes(v.status)?v.status:'unknown';
    s+='<div class="pv-v" data-kind="'+esc(v.property.kind)+'" data-status="'+esc(v.status)+'"><div class="pv-h"><b>'+esc(v.property.kind)+'</b><span class="pv-s pv-'+st+'">'+esc(st)+'</span>'+(v.method?'<span class="pv-m">'+esc(v.method)+'</span>':'')+'</div>';
    if(v.detail)s+='<p class="pv-d">'+esc(v.detail)+'</p>';
    if(v.evidence)s+='<div class="pv-e">'+esc(v.evidence)+'</div>';
    const c=v.counterexample;
    if(c){
      if(c.explanation)s+='<p class="pv-t">'+esc(c.explanation)+'</p>';
      if(c.trace)s+='<p class="pv-t">counterexample: <code>'+(c.trace.length?c.trace.map(esc).join(' &rarr; '):'(initial marking)')+'</code>'+(c.marking?'<br>ends at: <code>'+esc(pvMark(c.marking))+'</code>':'')+'</p>';
      if(chk)s+='<p class="pv-t '+(chk.ok?'pv-ok':'pv-bad')+'" data-replay="'+(chk.ok?'ok':'bad')+'">'+(chk.ok?'Replay reproduced. ':'Replay NOT reproduced. ')+esc(chk.text)+'</p>';
    }
    if(st==='unknown')s+='<p class="pv-t pv-unknown">Unknown is not a pass: the search was bounded'+esc(pvWhy())+'.</p>';
    if(v.property.kind==='deadlock-free')s+='<p class="pv-t pv-def">Definition used here (verify): a marking with nothing enabled counts as finished only when it is empty. The reachability verdict in the analysis section counts any marking with nothing enabled as a deadlock, so the two can differ for the same net.</p>';
    s+='</div>';
  }
  b.innerHTML=s;
}
pvRender();
$('pv-text').value=pvText();
$('pv-copy').onclick=async()=>{
  const t=pvText(),ta=$('pv-text'),st=$('pv-status');ta.value=t;
  try{await navigator.clipboard.writeText(t);st.textContent='Copied.'}
  catch(e){ta.closest('details').open=true;ta.focus();ta.select();
    let done=false;try{done=document.execCommand('copy')}catch(_){}
    st.textContent=done?'Copied.':'Copy is blocked here; the text is selected below.'}
};
${composite ? `
// the engine-free re-checker (verify-record.mjs), unchanged
${checker()}
$('pv-run').onclick=()=>{
  const out=$('pv-checks'),st=$('pv-rstat');out.innerHTML='';st.textContent='';
  try{
    const rec=JSON.parse($('pv-rec').value);
    const mem=JSON.parse($('pv-mem').value);
    const res=verifyRecord(netDoc,mem.members||mem,rec);
    out.innerHTML=res.checks.map(c=>'<li class="'+(c.ok?'pv-ok':'pv-bad')+'">'+(c.ok?'ok':'FAILED')+': '+esc(c.what)+' &mdash; '+esc(c.detail)+'</li>').join('')
      +res.skipped.map(x=>'<li class="pv-m">skipped: '+esc(x)+'</li>').join('');
    st.textContent=res.valid?'Every check this script can make passed.':'A check failed.';
  }catch(e){st.textContent='Could not re-check: '+String(e&&e.message||e)}
};` : ''}
`;
}

export default {
  id: 'proof',
  title: 'Proof',
  needsEngineResult: true,
  buildSection(ctx) {
    const composite = looksComposite(ctx.doc);
    let result, error;
    if (!ctx.doc) error = 'this net cannot be written as pflow.xyz JSON-LD, which verify reads';
    else {
      try { result = ctx.engine.hosted('verify', { net: ctx.doc, properties: PROPERTIES, max_states: MAX_STATES }); }
      catch (e) { error = String(e?.message ?? e); }
    }
    return { css, html: html(composite), js: buildJs({ result, error, ctx, composite }) };
  },
};
