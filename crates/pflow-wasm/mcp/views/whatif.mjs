// The whatif view: two scenarios on one seed, side by side. The baseline is the
// net as it is; the other arm changes one thing: one place's tokens, or one
// transition switched off (rate 0 is off). Both arms run on the same seed, so
// the difference measures the change and not the dice (the semantics of the
// what_if tool). Every number on the page is computed by the embedded engine at
// page time (Net.stochastic for SSA, Net.simulate for ODE); the page states no
// verdict of its own, only the engine's means, spread and differences.
//
// Limits it states instead of hiding: a place that holds several colours is not
// offered (the what_if tool names a colour; use it), and the SSA arms report the
// final mean and sd the engine returns (min/max across runs are in the tool).

const css = `
.wi-row{display:flex;flex-wrap:wrap;gap:8px 14px;align-items:end;margin:0 0 10px}
.wi-row label{display:flex;flex-direction:column;gap:2px;margin:0;color:var(--mut);font-size:12px}
.wi-row select,.wi-row input{font:inherit;font-size:14px;background:var(--bg);color:var(--fg);border:1px solid var(--line);border-radius:6px;padding:4px 6px;max-width:100%}
.wi-row input[type=number]{width:76px}
.wi-pair{display:grid;grid-template-columns:1fr 1fr;gap:10px}
.wi-pair>div{min-width:0}.wi-pair h3{font-size:13px;margin:0 0 4px;font-weight:600}
@media (max-width:620px){.wi-pair{grid-template-columns:1fr}}
.wi-tbl{width:100%;border-collapse:collapse;font-size:13px}
.wi-tbl th,.wi-tbl td{text-align:right;padding:4px 6px;border-bottom:1px solid var(--line)}
.wi-tbl th:first-child,.wi-tbl td:first-child{text-align:left}
.wi-tbl th{color:var(--mut);font-weight:500;font-size:12px}
.wi-scroll{overflow-x:auto}
.wi-bar{display:inline-block;height:8px;border-radius:2px;vertical-align:middle;margin-left:6px}
.wi-pos{color:var(--acc)}.wi-neg{color:#dc2626}
#wi-note{color:var(--mut);font-size:13px;margin:6px 0 0}
#wi-msg{font-size:13px;min-height:1.4em;color:var(--mut)}
`;

const html = `
<section id="wi-controls"><h2>What if</h2>
<div class="wi-row">
<label>change<select id="wi-kind"><option value="tokens">a place's tokens</option><option value="off">a transition off</option></select></label>
<label id="wi-pl">place<select id="wi-place"></select></label>
<label id="wi-dl">add tokens<input id="wi-delta" type="number" value="1" step="1"></label>
<label id="wi-tl" hidden>transition<select id="wi-trans"></select></label>
<label>t<input id="wi-t" type="number" value="10" min="0.1" step="any"></label>
<label>runs<input id="wi-runs" type="number" value="16" min="1" max="200"></label>
<label>seed<input id="wi-seed" type="number" value="7" min="0"></label>
</div>
<button id="wi-run">Run both arms</button><div id="wi-msg" role="status"></div>
<p id="wi-note"></p></section>
<section id="wi-sim"><h2>Simulation, both arms on one seed</h2>
<h3 style="font-size:13px;margin:0 0 6px">SSA, mean over the runs</h3>
<div class="wi-pair"><div><h3 id="wi-a-name">as is</h3><div id="wi-ssa-a"></div></div><div><h3 id="wi-b-name">changed</h3><div id="wi-ssa-b"></div></div></div>
<h3 style="font-size:13px;margin:12px 0 6px">ODE, mean-field</h3>
<div class="wi-pair"><div><div id="wi-ode-a"></div></div><div><div id="wi-ode-b"></div></div></div>
<div class="leg" id="wi-legend"></div></section>
<section id="wi-diff"><h2>Difference per place (changed minus as is)</h2>
<div class="wi-scroll"><table class="wi-tbl" id="wi-table"><thead><tr><th>place</th><th>SSA as is</th><th>SSA changed</th><th>SSA diff</th><th>ODE diff</th></tr></thead><tbody></tbody></table></div></section>
`;

const js = `
const WI_COLORS=['#2563eb','#dc2626','#16a34a','#d97706','#9333ea','#0891b2','#be185d','#65a30d'];
const wiNet=d=>new Net(d);
const wiBase=wiNet(netDoc);
const wiPlaces=wiBase.places(), wiTrans=wiBase.transitions();
const wiArr=Array.isArray(netDoc.places);
// a place whose initial marking is a colour vector cannot take a single delta
const wiInit=p=>{const v=wiArr?(netDoc.places.find(x=>x.id===p)||{}).initial:(netDoc.places[p]||{}).initial;return v};
const wiSimple=p=>{const v=wiInit(p);return !Array.isArray(v)||v.length<=1};
const wiOffer=wiPlaces.filter(wiSimple);
const wiCount=p=>{let v=wiInit(p);if(Array.isArray(v))v=v[0];return typeof v==='number'?v:0};
const wiOpt=(el,items)=>{el.innerHTML=items.map(x=>'<option value="'+esc(x)+'">'+esc(x)+'</option>').join('')};
wiOpt($('wi-place'),wiOffer);wiOpt($('wi-trans'),wiTrans);
const wiDefault=wiOffer.find(p=>wiCount(p)>0)||wiOffer[0];
if(wiDefault)$('wi-place').value=wiDefault;
if(wiOffer.length<wiPlaces.length)$('wi-note').textContent='Places that hold several colours are not offered here; the what_if tool takes a colour. Showing all '+wiPlaces.length+' places in the charts.';
if(!wiOffer.length){$('wi-kind').value='off';$('wi-kind').querySelector('option[value=tokens]').disabled=true}
if(!wiTrans.length){$('wi-kind').querySelector('option[value=off]').disabled=true}
const wiSync=()=>{const off=$('wi-kind').value==='off';$('wi-pl').hidden=off;$('wi-dl').hidden=off;$('wi-tl').hidden=!off};
$('wi-kind').onchange=()=>{wiSync();wiRun()};wiSync();
const wiChanged=(kind,place,delta)=>{
  const d=JSON.parse(JSON.stringify(netDoc));
  const to=wiCount(place)+delta;
  if(to<0)throw new Error(place+' has '+wiCount(place)+'; cannot remove '+(-delta));
  const q=wiArr?d.places.find(x=>x.id===place):d.places[place];q.initial=Array.isArray(q.initial)?[to]:to;
  return d};
const wiSer=(res,p)=>Array.isArray(res.series[p])?res.series[p]:res.series[p].values;
function wiChart(el,res,ymax){
  const W=400,H=210,L=40,B=24,T=6,R=6,ts=res.times;
  const x=t=>L+(t-ts[0])/((ts[ts.length-1]-ts[0])||1)*(W-L-R), y=v=>T+(1-v/ymax)*(H-T-B);
  let s='<svg viewBox="0 0 '+W+' '+H+'" role="img" aria-label="token counts over time">';
  for(let i=0;i<=4;i++){const v=ymax*i/4,yy=y(v);s+='<line x1="'+L+'" x2="'+(W-R)+'" y1="'+yy+'" y2="'+yy+'" stroke="var(--line)"/><text x="'+(L-5)+'" y="'+(yy+4)+'" text-anchor="end" font-size="10" fill="var(--mut)">'+(+v.toFixed(2))+'</text>'}
  for(const i of [0,.5,1]){const t=ts[0]+(ts[ts.length-1]-ts[0])*i;s+='<text x="'+x(t)+'" y="'+(H-7)+'" text-anchor="middle" font-size="10" fill="var(--mut)">'+(+t.toFixed(2))+'</text>'}
  wiPlaces.forEach((p,k)=>{const vals=wiSer(res,p);
    s+='<polyline fill="none" stroke="'+WI_COLORS[k%8]+'" stroke-width="2" points="'+vals.map((v,i)=>x(ts[i]).toFixed(1)+','+y(v).toFixed(1)).join(' ')+'"/>'});
  el.innerHTML=s+'</svg>'}
const wiMax=(...rs)=>{let m=0;for(const r of rs)for(const p of wiPlaces)for(const v of wiSer(r,p))m=Math.max(m,v);return m||1};
const wiFmt=v=>String(+(+v).toFixed(3));
const wiSigned=v=>(v>0?'+':'')+wiFmt(v);
function wiRun(){
  const msg=$('wi-msg');msg.textContent='';
  try{
    const kind=$('wi-kind').value,tEnd=num('wi-t',10),runs=Math.max(1,Math.round(num('wi-runs',16))),seed=Math.max(0,Math.round(num('wi-seed',7)));
    setSeed(seed);
    let bNet,rates={},label;
    if(kind==='off'){const t=$('wi-trans').value;bNet=wiNet(netDoc);rates={[t]:0};label=t+' off (rate 0)'}
    else{const p=$('wi-place').value,dl=Math.round(num('wi-delta',1));bNet=wiNet(wiChanged(kind,p,dl));label=(dl>=0?'+':'')+dl+' '+p}
    const so={horizon:tEnd,samples:80,realizations:runs,seed};
    const aS=wiNet(netDoc).stochastic(so);
    const bS=bNet.stochastic(Object.assign({},so,{rates}));
    const aO=wiNet(netDoc).simulate({tEnd,maxPoints:80});
    const bO=bNet.simulate({tEnd,maxPoints:80,rates});
    $('wi-a-name').textContent='as is';$('wi-b-name').textContent=label;
    const ys=wiMax(aS,bS),yo=wiMax(aO,bO);
    wiChart($('wi-ssa-a'),aS,ys);wiChart($('wi-ssa-b'),bS,ys);wiChart($('wi-ode-a'),aO,yo);wiChart($('wi-ode-b'),bO,yo);
    $('wi-legend').innerHTML=wiPlaces.map((p,k)=>'<span><i class="sw" style="background:'+WI_COLORS[k%8]+'"></i>'+esc(p)+'</span>').join('');
    const dS=wiPlaces.map(p=>bS.final[p]-aS.final[p]), dO=wiPlaces.map(p=>bO.final[p]-aO.final[p]);
    const big=Math.max(...dS.map(Math.abs),1e-9);
    $('wi-table').tBodies[0].innerHTML=wiPlaces.map((p,i)=>{
      const sd=r=>{const s=r.series[p];return s&&s.stddev?s.stddev[s.stddev.length-1]:null};
      const sdA=sd(aS),sdB=sd(bS);
      const cls=dS[i]>0?'wi-pos':dS[i]<0?'wi-neg':'';
      const w=Math.round(Math.abs(dS[i])/big*60);
      return '<tr data-place="'+esc(p)+'"><td>'+esc(p)+'</td>'+
        '<td class="wi-fa" data-v="'+aS.final[p]+'">'+wiFmt(aS.final[p])+(sdA!=null?' &plusmn;'+wiFmt(sdA):'')+'</td>'+
        '<td class="wi-fb" data-v="'+bS.final[p]+'">'+wiFmt(bS.final[p])+(sdB!=null?' &plusmn;'+wiFmt(sdB):'')+'</td>'+
        '<td class="wi-ds '+cls+'" data-v="'+dS[i]+'">'+wiSigned(dS[i])+'<i class="wi-bar" style="width:'+w+'px;background:currentColor"></i></td>'+
        '<td class="wi-do" data-v="'+dO[i]+'">'+wiSigned(dO[i])+'</td></tr>'}).join('');
    msg.textContent='SSA '+runs+' runs, seed '+seed+' on both arms; ODE from each arm\\'s own start. Final values at t='+tEnd+'. \\u00b1 is the engine\\'s sd across runs.';
    $('wi-diff').setAttribute('data-ready','1');
  }catch(e){msg.textContent=String(e&&e.message||e)}
}
$('wi-run').onclick=wiRun;
wiRun();
`;

export default {
  id: 'whatif',
  title: 'What if',
  needsEngineResult: false,
  buildSection: () => ({ css, html, js, seed: 7 }),
};
