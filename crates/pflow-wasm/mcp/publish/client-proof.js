// Proof page client. Globals set by the builder: FULL (hosted engine), CHYP (chyp-rs), CHYP_GZ_B64 (its wasm),
// THEORY (the saga theory text), PAGE_BYTES, ENGINE_BYTES.
const T0 = performance.now();
const $ = (id) => document.getElementById(id);
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const timing = {};
{ const t = performance.now(); await FULL.init(); timing.engine = performance.now() - t; }
{
  const t = performance.now();
  const bin = atob(CHYP_GZ_B64), gz = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) gz[i] = bin.charCodeAt(i);
  const bytes = new Uint8Array(await new Response(new Blob([gz]).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer());
  CHYP.initSync({ module: bytes });
  timing.chyp = performance.now() - t;
}
const ex = FULL.hosted('bundle_example', {});
const bundle = ex.bundle;

// ---- 1. the parts and their glue -------------------------------------------
{
  const ends = (e) => `${e.subnet}.${e.port}`;
  $('parts').innerHTML = bundle.subnets.map((s) => `<div class="mark"><b>${esc(s.id)}</b><span>${s.model.places.length} places, ${s.model.transitions.length} steps</span></div>`).join('');
  $('glue').innerHTML = bundle.links.map((l) => `<li>${esc(ends(l.from))} <span class="tag">${esc(l.kind)}</span> ${esc(ends(l.to))}</li>`).join('');
  const v = FULL.hosted('bundle_validate', { bundle });
  const rows = [];
  rows.push({ st: v.valid ? 'ok' : 'bad', title: v.valid ? 'The parts fit together' : 'The parts do not fit together', text: v.valid ? `All ${bundle.links.length} connections join compatible ports on different parts.` : `${v.errors.length} connection problem(s).`, how: v });
  for (const w of v.warnings ?? []) rows.push({ st: 'warn', title: 'Heads-up: ' + w.code, text: w.message, how: w });
  for (const e of v.errors ?? []) rows.push({ st: 'bad', title: e.code ?? 'error', text: e.message, how: e });
  $('glueChecks').innerHTML = rows.map((r, i) => `<li class="check ${r.st}"><span class="badge">${r.st === 'ok' ? '✓' : r.st === 'bad' ? '✕' : '!'}</span><div><b>${esc(r.title)}</b><p>${esc(r.text)}</p><details><summary>Raw result</summary><pre id="gw${i}"></pre></details></div></li>`).join('');
  rows.forEach((r, i) => { $('gw' + i).textContent = JSON.stringify(r.how, null, 1); });
}

// ---- 2. does the composite behave as specified -----------------------------
{
  let met = 0, limits = 0, html = '';
  ex.scenarios.forEach((sc, k) => {
    const res = FULL.hosted('bundle_run', { bundle, script: sc.script, expect: sc.expect.map(({ op, ...rest }) => rest) });
    const all = res.completed && res.expect.every((e) => e.pass);
    const documented = !!sc.note && !all;
    if (all) met++; else if (documented) limits++;
    html += `<div class="check ${all ? 'ok' : 'warn'}"><span class="badge">${all ? '✓' : '!'}</span><div><b>${esc(sc.title)}</b>`
      + res.expect.map((e) => `<p>${e.pass ? '✓' : '✕'} ${esc(e.says)} <span class="mono">(${esc(e.place)} is ${e.actual}, expected ${e.n})</span></p>`).join('')
      + (res.completed ? '' : `<p>✕ The run stopped before the last step.</p>`)
      + (sc.note ? `<p><span class="tag">${all ? 'note' : 'known limit'}</span> ${esc(sc.note)}</p>` : '')
      + `<details><summary>Steps fired</summary><pre id="sc${k}"></pre></details></div></div>`;
    setTimeout(() => { $('sc' + k).textContent = sc.script.join('\n'); }, 0);
  });
  $('scenarios').innerHTML = html;
  $('scenSummary').textContent = `${met} of ${ex.scenarios.length} scenarios meet every expectation; ${limits} show a limitation the example documents.`;
}

// ---- 3. the Chyp proof -------------------------------------------------------
let session = null, parts = [], sel = -1;
function drawDiagram(d) {
  const sc = 44, pad = 14, b = d.bounds, h = Math.max(b.y_max - b.y_min, 1);
  const w = (b.x_max - b.x_min) * sc + 2 * pad, H = h * sc + 2 * pad;
  const X = (x) => pad + (x - b.x_min) * sc, Y = (y) => pad + (y - b.y_min) * sc + (h - (b.y_max - b.y_min)) * sc / 2;
  const V = Object.fromEntries(d.vertices.map((v) => [v.id, v]));
  let s = `<svg viewBox="0 0 ${w} ${H}" style="max-height:${Math.max(H, 60)}px" role="img" aria-label="string diagram">`;
  for (const e of d.edges) {
    if (e.wire) { for (const a of e.sources) for (const t of e.targets) s += `<line class="wire" x1="${X(V[a].x)}" y1="${Y(V[a].y)}" x2="${X(V[t].x)}" y2="${Y(V[t].y)}"/>`; continue; }
    const bw = Math.max(0.9, e.label.length * 0.17) * sc, bh = 0.62 * sc, cx = X(e.x), cy = Y(e.y);
    for (const a of e.sources) s += `<line class="wire" x1="${X(V[a].x)}" y1="${Y(V[a].y)}" x2="${cx - bw / 2}" y2="${cy}"/>`;
    for (const t of e.targets) s += `<line class="wire" x1="${cx + bw / 2}" y1="${cy}" x2="${X(V[t].x)}" y2="${Y(V[t].y)}"/>`;
    s += `<rect class="box" x="${cx - bw / 2}" y="${cy - bh / 2}" width="${bw}" height="${bh}" rx="4"/><text class="lbl" x="${cx}" y="${cy}">${esc(e.label)}</text>`;
  }
  for (const v of d.vertices) if (v.input?.length || v.output?.length) s += `<circle class="vtx" cx="${X(v.x)}" cy="${Y(v.y)}" r="3.5"/>`;
  return s + '</svg>';
}
function showPart(i) {
  sel = i; renderParts();
  const p = JSON.parse(session.part(i));
  const fig = (cap, d) => `<figure><figcaption>${cap}</figcaption>${drawDiagram(d)}</figure>`;
  $('diagram').innerHTML = p.lhs && p.rhs ? fig('left side', p.lhs) + fig('right side', p.rhs) : '<p class="log">No diagram for this part.</p>';
  $('partinfo').textContent = `${p.kind} ${p.name || ''} — line ${p.line}, ${p.status}` + (p.tactic ? `, step by ${p.tactic_args?.join(' ') || p.tactic}` : '');
}
function renderParts() {
  $('plist').innerHTML = parts.filter((p) => p.kind !== 'GenPart').map((p) => `<li class="part ${p.status}" data-i="${p.index}" ${p.index === sel ? 'aria-current="true"' : ''}><span class="dot"></span><span class="mono">${esc(p.kind.replace('Part', '').toLowerCase())}</span><span>${esc(p.name || '')} <span class="mono" style="color:var(--mut)">line ${p.line} · ${esc(p.status)}</span></span></li>`).join('');
}
function check() {
  session?.free?.();
  session = new CHYP.Session($('theory').value);
  const r = JSON.parse(session.check_all());
  parts = r.parts;
  const bad = r.counts.invalid, errs = r.errors?.length ?? 0;
  const proofs = parts.filter((p) => p.kind === 'TheoremPart');
  const okTheorems = proofs.filter((p) => { const idx = parts.indexOf(p); const end = parts.findIndex((q, j) => j > idx && q.kind === 'ProofQedPart'); return parts.slice(idx, end + 1).every((q) => q.status !== 'invalid'); }).length;
  const el = $('verdict');
  el.className = 'check ' + (errs || bad ? 'bad' : 'ok');
  el.innerHTML = `<span class="badge">${errs || bad ? '✕' : '✓'}</span><div><b>${errs ? 'The theory has syntax errors' : bad ? `${okTheorems} of ${proofs.length} theorems are proved; ${bad} step${bad === 1 ? '' : 's'} do not check` : `All ${proofs.length} theorems are proved`}</b><p>${errs ? esc(r.errors.map((e) => `line ${e.line}: ${e.msg}`).join('; ')) : bad ? 'A step cites a rule that does not turn its left side into its right side. Select it below to see both diagrams.' : 'Every rewrite step was re-checked in this page, with no server.'}</p></div>`;
  sel = parts.findIndex((p) => p.status === 'invalid'); if (sel < 0) sel = parts.findIndex((p) => p.kind === 'RulePart');
  renderParts(); if (sel >= 0) showPart(sel);
  $('chypms').textContent = `chyp-rs ${r.upstream ?? ''}`;
}
$('theory').value = THEORY;
$('plist').onclick = (e) => { const li = e.target.closest('li'); if (li) showPart(+li.dataset.i); };
$('check').onclick = check;
$('break').onclick = () => { $('theory').value = THEORY.replace('= reserve ; id ; undo_reserve by charge_undone', '= reserve ; id ; undo_reserve by reserve_undone'); check(); };
$('restore').onclick = () => { $('theory').value = THEORY; check(); };
check();

// ---- self-report ----------------------------------------------------------
const reqs = performance.getEntriesByType('resource').filter((r) => !/^(data|blob):/.test(r.name)).length;
const kb = (n) => (n / 1024).toFixed(0) + ' KB';
$('foot').innerHTML = [`page ${kb(PAGE_BYTES)}`, `engines ${ENGINE_BYTES}`, `pflow engine ready in ${timing.engine.toFixed(0)} ms`, `chyp ready in ${timing.chyp.toFixed(0)} ms`, `ready ${(performance.now() - T0).toFixed(0)} ms after script start`, `network requests: ${reqs}`]
  .map((x) => `<span>${esc(x)}</span>`).join('');
window.__ready = { variant: 'proof', network: reqs, timing, ms: performance.now() - T0 };
