// The play view: marking, click-to-fire token game and ODE/SSA charts. What
// artifact_html wrote before views existed; the shell (../pages.mjs) supplies
// the engine, `$`, `esc`, `num` and `netDoc` (the net parsed from #pflow-net).

const css = `
.marks{display:flex;flex-wrap:wrap;gap:8px}.mark{border:1px solid var(--line);border-radius:8px;padding:6px 12px;background:var(--bg)}
.mark b{font-size:20px;display:block;line-height:1.1}.mark span{color:var(--mut);font-size:12px}
label{margin-right:12px;color:var(--mut);font-size:13px;display:inline-block}
input[type=number]{font:inherit;width:70px;background:var(--bg);color:var(--fg);border:1px solid var(--line);border-radius:6px;padding:3px 6px}
#log{color:var(--mut);font-size:13px;min-height:1.4em}
`;

const html = `
<section><h2>Marking</h2><div class="marks" id="marks"></div></section>
<section><h2>Fire</h2><div id="tx"></div><button id="reset">Reset</button><div id="log"></div></section>
<section><h2>Over time</h2>
<label>t <input id="t" type="number" value="10"></label><label>runs <input id="n" type="number" value="20"></label><label>seed <input id="seed" type="number" value="1"></label>
<div style="margin-top:8px"><button id="ode">Run ODE</button><button id="ssa">Run SSA</button></div>
<div id="chart"></div><div class="leg" id="legend"></div></section>
`;

const js = `
const COLORS=['#2563eb','#dc2626','#16a34a','#d97706','#9333ea','#0891b2','#be185d','#65a30d'];
const net=new Net(netDoc);
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
$('seed').addEventListener('input',()=>setSeed(num('seed',1)));
render();run('ode');
`;

export default {
  id: 'play',
  title: 'Play',
  needsEngineResult: false,
  // A view returns { css, html, js, seed? }. js runs as a module block after the
  // engine is initialised; seed is the default shown in the footer.
  buildSection: () => ({ css, html, js, seed: 1 }),
};
