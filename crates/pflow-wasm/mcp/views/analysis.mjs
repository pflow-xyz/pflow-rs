// The analysis view: what the engine can say about a net without running it.
//   - P- and T-invariants, conservation and structural boundedness, and the
//     incidence matrix: the hosted engine's `invariants` and `incidence`
//     answers, computed when the page is written (ctx.engine.hosted) and
//     embedded as data; the page is not given the 1 MB hosted engine.
//   - the reachability verdicts (deadlock_free, quasi_live L1, live L4, each
//     true, false or "unknown"), the bounded state and edge counts and a
//     witness trace: computed by the page's own embedded engine when it opens
//     (Net.reachability), and re-run with another search bound on request. The
//     witness is replayed by firing it, transition by transition, in a second
//     copy of the engine; the page compares where the replay ends with the
//     marking the engine reported.
// Nothing here is typed by hand: every verdict, number and trace is an engine
// result. A verdict of "unknown" is shown as unknown, with the bound it was
// bounded by.

const css = `
.an-sub{color:var(--mut);font-size:13px;margin:0 0 8px}
.an-kv{display:flex;flex-wrap:wrap;gap:8px;margin:0 0 10px}
.an-kv div{border:1px solid var(--line);border-radius:8px;padding:6px 12px;background:var(--bg);min-width:84px}
.an-kv b{display:block;font-size:18px;line-height:1.2;overflow-wrap:anywhere}.an-kv span{color:var(--mut);font-size:12px}
.an-kv [data-value=true] b::before{content:"\\2713  "}.an-kv [data-value=false] b::before{content:"\\2717  "}.an-kv [data-value=unknown] b::before{content:"? "}
.an-kv [data-value=true] b{color:#15803d}.an-kv [data-value=false] b{color:#dc2626}.an-kv [data-value=unknown] b{color:#b45309}
:root[data-theme=dark] .an-kv [data-value=true] b{color:#4ade80}:root[data-theme=dark] .an-kv [data-value=false] b{color:#f87171}:root[data-theme=dark] .an-kv [data-value=unknown] b{color:#fbbf24}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]) .an-kv [data-value=true] b{color:#4ade80}:root:not([data-theme=light]) .an-kv [data-value=false] b{color:#f87171}:root:not([data-theme=light]) .an-kv [data-value=unknown] b{color:#fbbf24}}
.an-list{margin:0 0 10px;padding-left:20px}.an-list li{margin:2px 0;font:13px/1.5 ui-monospace,monospace;overflow-wrap:anywhere}
.an-h3{font-size:13px;font-weight:600;margin:10px 0 4px}
.an-scroll{overflow-x:auto;max-width:100%;-webkit-overflow-scrolling:touch}
table.an-mx{border-collapse:collapse;font:12px ui-monospace,monospace}
table.an-mx th,table.an-mx td{border:1px solid var(--line);padding:3px 8px;text-align:right;white-space:nowrap}
table.an-mx th{color:var(--mut);font-weight:600;background:var(--bg)}table.an-mx th:first-child{text-align:left;position:sticky;left:0}
table.an-mx td.neg{color:#dc2626}table.an-mx td.pos{color:#15803d}table.an-mx td.zero{color:var(--mut)}
:root[data-theme=dark] table.an-mx td.neg{color:#f87171}:root[data-theme=dark] table.an-mx td.pos{color:#4ade80}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]) table.an-mx td.neg{color:#f87171}:root:not([data-theme=light]) table.an-mx td.pos{color:#4ade80}}
.an-note{color:var(--mut);font-size:13px;margin:6px 0}.an-warn{color:#b45309;font-size:13px;margin:6px 0}
.an-tr{display:flex;flex-wrap:wrap;gap:4px;margin:6px 0}
.an-tr span{border:1px solid var(--line);border-radius:6px;padding:2px 8px;background:var(--bg);font:12px ui-monospace,monospace}
.an-tr span.done{border-color:var(--acc);color:var(--acc)}
.an-ctl label{margin-right:12px;color:var(--mut);font-size:13px;display:inline-block}
.an-ctl input[type=number]{font:inherit;width:90px;background:var(--bg);color:var(--fg);border:1px solid var(--line);border-radius:6px;padding:3px 6px}
#an-replay-marks{display:flex;flex-wrap:wrap;gap:6px;margin:6px 0}
#an-replay-marks div{border:1px solid var(--line);border-radius:8px;padding:4px 10px;background:var(--bg)}
#an-replay-marks b{font-size:16px;display:block;line-height:1.2}#an-replay-marks span{color:var(--mut);font-size:12px}
`;

const html = `
<section id="an-structure"><h2>Invariants and conservation</h2><div id="an-inv"></div></section>
<section id="an-incidence"><h2>Incidence matrix</h2><div id="an-inc"></div></section>
<section id="an-reach"><h2>Reachability</h2>
<div class="an-ctl"><label>max states <input id="an-max" type="number" min="1" max="20000" value="2000"></label><button id="an-run">Search again</button></div>
<div id="an-verdicts" class="an-kv"></div><div id="an-counts" class="an-kv"></div><div id="an-reach-note" class="an-note"></div>
<div id="an-witness"></div></section>
`;

// Browser code, run by the shell after the engine is up. String.raw: no
// escaping, and no backtick or dollar-brace inside.
const BROWSER = String.raw`
const fmtV=v=>v===true?'true':v===false?'false':'unknown';
const kv=(label,value,extra)=>'<div data-k="'+esc(label)+'" data-value="'+esc(String(value))+'"><b>'+esc(extra!=null?extra:String(value))+'</b><span>'+esc(label)+'</span></div>';

// ---- invariants and conservation: the hosted engine's answers, as written ----
(function(){
  const el=$('an-inv'),inv=AN.invariants;
  if(!inv){el.innerHTML='<p class="an-note">'+esc(AN.invariantsError||'Invariants are not available for this net.')+'</p>';return}
  const p=inv.p_invariants||[],t=inv.t_invariants||[];
  let s='<div class="an-kv">'+
    kv('conservative',inv.conservative===true?'true':inv.conservative===false?'false':String(inv.conservative))+
    kv('structurally bounded',inv.structurally_bounded===true?'true':inv.structurally_bounded===false?'false':String(inv.structurally_bounded))+
    kv('P-invariants',p.length)+kv('T-invariants',t.length)+'</div>';
  s+='<div class="an-h3">P-invariants (token conservation laws, valued at the initial marking)</div>';
  s+=p.length?'<ul class="an-list" id="an-p-list">'+p.map(x=>'<li>'+esc(x.equation)+'</li>').join('')+'</ul>':'<p class="an-note" id="an-p-list">none</p>';
  if(inv.p_truncated)s+='<p class="an-warn">The elimination hit its row limit: the list is valid but may be incomplete.</p>';
  s+='<div class="an-h3">T-invariants (firing multisets that return the net to its marking)</div>';
  s+=t.length?'<ul class="an-list" id="an-t-list">'+t.map(x=>'<li>'+esc(x.firing_multiset)+'</li>').join('')+'</ul>':'<p class="an-note" id="an-t-list">none</p>';
  if(inv.t_truncated)s+='<p class="an-warn">The elimination hit its row limit: the list is valid but may be incomplete.</p>';
  el.innerHTML=s;
})();

// ---- incidence matrix C = post - pre ----
(function(){
  const el=$('an-inc'),m=AN.incidence;
  if(!m){el.innerHTML='<p class="an-note">'+esc(AN.incidenceError||'The incidence matrix is not available for this net.')+'</p>';return}
  const MAX=50,P=m.places,T=m.transitions,pr=Math.min(P.length,MAX),tc=Math.min(T.length,MAX);
  let s='<p class="an-sub">Rows are places, columns are transitions (post minus pre). Read and inhibitor arcs move nothing and are left out.</p><div class="an-scroll"><table class="an-mx" id="an-matrix"><thead><tr><th></th>'+T.slice(0,tc).map(t=>'<th>'+esc(t)+'</th>').join('')+'</tr></thead><tbody>';
  for(let i=0;i<pr;i++)s+='<tr><th>'+esc(P[i])+'</th>'+m.matrix[i].slice(0,tc).map(v=>'<td class="'+(v<0?'neg':v>0?'pos':'zero')+'">'+v+'</td>').join('')+'</tr>';
  s+='</tbody></table></div>';
  if(P.length>pr||T.length>tc)s+='<p class="an-note">Showing '+pr+' of '+P.length+' places and '+tc+' of '+T.length+' transitions.</p>';
  el.innerHTML=s;
})();

// ---- reachability, computed here by the embedded engine ----
const rpNet=new Net(netDoc);
let rpTrace=[],rpAt=0,rpExpect=null;
function marksHtml(m){return Object.keys(m).map(k=>'<div><b>'+m[k]+'</b><span>'+esc(k)+'</span></div>').join('')}
function renderReplay(){
  const at=$('an-replay-state');if(!at)return;
  $('an-replay-marks').innerHTML=marksHtml(rpNet.marking());
  $('an-tr').innerHTML=rpTrace.map((t,i)=>'<span class="'+(i<rpAt?'done':'')+'">'+esc(t)+'</span>').join('');
  $('an-step').disabled=rpAt>=rpTrace.length;$('an-play').disabled=rpAt>=rpTrace.length;$('an-back').disabled=rpAt===0;
  let msg='step '+rpAt+' of '+rpTrace.length;
  if(rpAt===rpTrace.length&&rpExpect){
    const cur=rpNet.marking(),same=Object.keys(rpExpect).every(k=>(cur[k]||0)===rpExpect[k])&&Object.keys(cur).every(k=>(rpExpect[k]||0)===cur[k]);
    msg+=same?'; the replay ends at the marking the engine reported':'; the replay ends at a DIFFERENT marking than the engine reported';
    $('an-replay-state').setAttribute('data-match',String(same));
  } else $('an-replay-state').removeAttribute('data-match');
  $('an-replay-state').textContent=msg;
}
function replayTo(n){
  rpNet.reset();
  for(let i=0;i<n;i++)rpNet.fire(rpTrace[i]);
  rpAt=n;renderReplay();
}
let timer=null;
function stopPlay(){if(timer){clearInterval(timer);timer=null}}
function runReach(){
  stopPlay();
  const max=Math.max(1,Math.min(20000,Math.floor(num('an-max',2000))));
  let r;
  try{const n=new Net(netDoc);try{r=n.reachability({maxStates:max,from:'initial'})}finally{if(n.free)n.free()}}
  catch(e){$('an-verdicts').innerHTML='';$('an-counts').innerHTML='';$('an-witness').innerHTML='';$('an-reach-note').textContent='Reachability failed: '+String(e&&e.message||e);return}
  $('an-verdicts').innerHTML=kv('deadlock free',fmtV(r.deadlock_free))+kv('quasi-live (L1)',fmtV(r.quasi_live))+kv('live (L4)',fmtV(r.live));
  const bounded=typeof r.bounded==='string'?'unknown':String(r.bounded);
  $('an-counts').innerHTML=kv('states',r.states)+kv('edges',r.edges)+kv('search complete',String(r.complete))+kv('bounded',bounded)+kv('deadlocks',r.deadlocks);
  let note='';
  if(!r.complete)note='The search was bounded'+(r.truncated?' ('+r.truncated+')':'')+': it stopped at '+r.states+' states, so no verdict above is true and a false one rests on its witness alone. Raise max states, up to 20000, to look further.';
  else note='The search saw every reachable marking ('+r.states+' states, '+r.edges+' edges).';
  if(r.dead_transitions&&r.dead_transitions.length)note+=' Transitions that never fire: '+r.dead_transitions.join(', ')+'.';
  $('an-reach-note').textContent=note;
  const w=r.witness,box=$('an-witness');
  if(!w){box.innerHTML='<p class="an-note">No witness: the engine found no deadlock or closed region'+(r.complete?'.':' in the part of the space it searched.')+'</p>';rpTrace=[];return}
  rpTrace=w.trace.slice();rpExpect=w.marking;
  let s='<div class="an-h3">Witness ('+esc(w.kind)+'): '+(w.kind==='deadlock'?'a firing sequence from the initial marking to a marking with nothing enabled':'a firing sequence into a closed region of '+w.region_size+' markings')+'</div>';
  if(w.never_fires&&w.never_fires.length)s+='<p class="an-note">Never fires from there: '+esc(w.never_fires.join(', '))+'</p>';
  s+='<div class="an-tr" id="an-tr"></div><div><button id="an-back">Back</button><button id="an-step">Step</button><button id="an-play">Play</button><button id="an-rewind">Rewind</button></div>';
  s+='<div id="an-replay-marks"></div><p class="an-note" id="an-replay-state" role="status"></p>';
  box.innerHTML=s;
  $('an-step').onclick=()=>{stopPlay();replayTo(rpAt+1)};
  $('an-back').onclick=()=>{stopPlay();replayTo(rpAt-1)};
  $('an-rewind').onclick=()=>{stopPlay();replayTo(0)};
  $('an-play').onclick=()=>{stopPlay();if(rpAt>=rpTrace.length)replayTo(0);timer=setInterval(()=>{if(rpAt>=rpTrace.length){stopPlay();return}replayTo(rpAt+1)},600)};
  replayTo(0);
}
$('an-run').onclick=runReach;
runReach();
`;

// Engine answers for the page, computed with the hosted engine now. A failure
// is kept as text the page shows, not swallowed.
function engineResults(ctx) {
  const out = { invariants: null, invariantsError: null, incidence: null, incidenceError: null };
  if (!ctx.doc || !ctx.engine?.hosted) {
    const why = 'This net cannot be written as a pflow.xyz net, so the structural analysis is not available; reachability below is unaffected.';
    out.invariantsError = out.incidenceError = why;
    return out;
  }
  try { out.invariants = ctx.engine.hosted('invariants', { net: ctx.doc, kind: 'both' }); }
  catch (e) { out.invariantsError = `invariants: ${e?.message ?? e}`; }
  try { out.incidence = ctx.engine.hosted('incidence', { net: ctx.doc }); }
  catch (e) { out.incidenceError = `incidence: ${e?.message ?? e}`; }
  return out;
}

const safeJson = (v) => JSON.stringify(v).replace(/</g, '\\u003c').replace(/[\u2028\u2029]/g, (c) => '\\u' + c.charCodeAt(0).toString(16));

export default {
  id: 'analysis',
  title: 'Analysis',
  needsEngineResult: true,
  buildSection: (ctx) => ({ css, html, js: `const AN=${safeJson(engineResults(ctx))};\n${BROWSER}` }),
};
