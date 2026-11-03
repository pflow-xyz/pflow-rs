// Explorer page client. Globals set by the builder before this runs:
//   SMALL (browser engine: Net) and/or FULL (hosted engine: Net, hosted, hostedTools)
//   MODELS [{id, title, blurb, ld}], VARIANT ('small'|'full'|'both'), PAGE_BYTES, ENGINE_BYTES
const T0 = performance.now();
const $ = (id) => document.getElementById(id);
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const num = (id, d) => { const v = $(id).value; return v === '' || isNaN(+v) ? d : +v; };
const fmt = (x) => (typeof x === 'number' ? (Number.isInteger(x) ? String(x) : (+x.toFixed(3)).toString()) : String(x));
const HAS_SMALL = typeof SMALL !== 'undefined', HAS_FULL = typeof FULL !== 'undefined';
const PLAY = HAS_SMALL ? SMALL : FULL;            // the engine that runs the token game and charts
const HOST = HAS_FULL ? FULL : null;              // the engine that proves things and answers every tool
const timing = {};
for (const [k, e] of [['small', HAS_SMALL && SMALL], ['full', HAS_FULL && FULL]]) {
  if (!e) continue;
  const t = performance.now(); await e.init(); timing[k] = performance.now() - t;
}

let cur = null, net = null;

// ---- plain-language checks ------------------------------------------------
const trace = (t) => (t && t.length ? t.join(' → ') : 'no steps');
const KIND = {
  'deadlock-free': { title: 'It never gets stuck', yes: 'In every situation it can reach, something can still happen.', no: 'It can get stuck: there is a situation where nothing can happen.' },
  bounded: { title: 'Nothing piles up without limit', yes: 'No place can ever hold more than a fixed number of tokens.', no: 'Some place can grow without limit.' },
  live: { title: 'Every step can happen', yes: 'Each action fires in at least one reachable situation.', no: 'Some action can never happen.' },
  'l4-live': { title: 'Every step stays possible', yes: 'From any reachable situation, each action can still happen later.', no: 'Once it gets into some situations, some actions are shut out for good.' },
};
function verdictRow(kind, v, how) {
  const k = KIND[kind];
  const st = v.status === 'proved' ? 'ok' : v.status === 'refuted' ? 'bad' : 'unk';
  const cx = v.counterexample;
  let text = st === 'ok' ? k.yes : st === 'bad' ? k.no : 'The search stopped at its limit, so this is not decided either way.';
  if (st === 'bad' && cx?.trace) text += ` Example: ${trace(cx.trace)}.`;
  return { st, title: k.title, text, how: how ?? v };
}
async function runChecks(model) {
  const rows = [];
  if (HOST) {
    const kinds = ['deadlock-free', 'bounded', 'live', 'l4-live'];
    const res = HOST.hosted('verify', { net: model.ld, properties: kinds.map((kind) => ({ kind })) });
    res.verdicts.forEach((v, i) => rows.push(verdictRow(kinds[i], v)));
    const inv = HOST.hosted('invariants', { net: model.ld, kind: 'p' });
    for (const p of (inv.p_invariants ?? []).slice(0, 4)) {
      const names = Object.keys(p.coefficients).join(', ');
      rows.push({ st: 'ok', title: `Always ${fmt(p.value)} in total across ${names}`, text: `The weighted sum ${p.equation.replace(/==/, 'stays equal to')} no matter what fires.`, how: p });
    }
    if (!(inv.p_invariants ?? []).length) rows.push({ st: 'unk', title: 'No conserved totals found', text: 'No weighted sum of places stays constant, so tokens are created or destroyed somewhere.', how: inv });
  } else {
    const r = net.reachability({ from: 'initial' });
    const tri = (v) => (v === true ? 'proved' : v === false ? 'refuted' : 'unknown');
    const w = r.witness;
    const shared = { counterexample: w ? { trace: w.trace } : undefined };
    rows.push(verdictRow('deadlock-free', { status: tri(r.deadlock_free), ...shared }, r));
    rows.push(verdictRow('bounded', { status: r.bounded === true ? 'proved' : typeof r.bounded === 'string' ? 'unknown' : 'refuted' }, { bounded: r.bounded }));
    rows.push(verdictRow('live', { status: tri(r.quasi_live) }, { quasi_live: r.quasi_live, dead_transitions: r.dead_transitions }));
    rows.push(verdictRow('l4-live', { status: tri(r.live), ...shared }, { live: r.live, not_live: r.not_live }));
    rows.push({ st: r.complete ? 'ok' : 'unk', title: r.complete ? `Checked all ${r.states} reachable situations` : `Checked ${r.states} situations, then stopped`, text: r.complete ? 'These answers cover every situation the model can reach.' : 'The model has more situations than the search limit, so a "no problem found" is not a proof.', how: { states: r.states, edges: r.edges, complete: r.complete, truncated: r.truncated } });
  }
  return rows;
}
function renderChecks(rows) {
  $('checks').innerHTML = rows.map((r, i) => `<li class="check ${r.st === 'unk' ? '' : r.st}"><span class="badge">${r.st === 'ok' ? '✓' : r.st === 'bad' ? '✕' : '?'}</span><div><b>${esc(r.title)}</b><p>${esc(r.text)}</p><details><summary>How this was checked</summary><pre id="how${i}"></pre></details></div></li>`).join('');
  rows.forEach((r, i) => { $('how' + i).textContent = JSON.stringify(r.how, null, 1); });
}

// ---- play -----------------------------------------------------------------
let places = [];
function renderPlay() {
  const m = net.marking(), en = new Set(net.enabled());
  $('marks').innerHTML = places.map((p) => `<div class="mark"><b>${fmt(m[p])}</b><span>${esc(p)}</span></div>`).join('');
  $('tx').innerHTML = '';
  for (const t of net.transitions()) {
    const b = document.createElement('button'); b.textContent = t; b.disabled = !en.has(t);
    b.onclick = () => { try { net.fire(t); $('log').textContent = 'fired ' + t; } catch (e) { $('log').textContent = String(e.message || e); } renderPlay(); };
    $('tx').appendChild(b);
  }
}
const COLORS = Array.from({ length: 8 }, (_, i) => `var(--s${i + 1})`);
function chart(res) {
  const W = 800, H = 280, L = 46, B = 28, T = 8, R = 10, ts = res.times;
  const ser = (p) => (Array.isArray(res.series[p]) ? res.series[p] : res.series[p].values);
  let max = 0; for (const p of places) for (const v of ser(p)) max = Math.max(max, v); max = max || 1;
  const x = (t) => L + (t - ts[0]) / ((ts[ts.length - 1] - ts[0]) || 1) * (W - L - R), y = (v) => T + (1 - v / max) * (H - T - B);
  let s = `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="token counts over time">`;
  for (let i = 0; i <= 4; i++) { const v = max * i / 4, yy = y(v); s += `<line x1="${L}" x2="${W - R}" y1="${yy}" y2="${yy}" stroke="var(--line)"/><text x="${L - 6}" y="${yy + 4}" text-anchor="end" font-size="11" fill="var(--mut)">${+v.toFixed(2)}</text>`; }
  for (const i of [0, .5, 1]) { const t = ts[0] + (ts[ts.length - 1] - ts[0]) * i; s += `<text x="${x(t)}" y="${H - 8}" text-anchor="middle" font-size="11" fill="var(--mut)">${+t.toFixed(2)}</text>`; }
  places.forEach((p, k) => { s += `<polyline fill="none" stroke="${COLORS[k % 8]}" stroke-width="2" points="${ser(p).map((v, i) => x(ts[i]).toFixed(1) + ',' + y(v).toFixed(1)).join(' ')}"/>`; });
  $('chart').innerHTML = s + '</svg>';
  $('legend').innerHTML = places.map((p, k) => `<span><i class="sw" style="background:${COLORS[k % 8]}"></i>${esc(p)}</span>`).join('');
}
function run(kind) {
  const t = num('t', 10);
  try {
    chart(kind === 'ode' ? net.simulate({ tEnd: t, maxPoints: 120 }) : net.stochastic({ horizon: t, samples: 120, realizations: num('n', 1), seed: num('seed', 1) }));
    $('clog').textContent = kind === 'ode' ? 'Average behaviour (differential equations), from the marking above.' : `Random run (${num('n', 1)} run${num('n', 1) === 1 ? '' : 's'} averaged, seed ${num('seed', 1)}), from the marking above.`;
  } catch (e) { console.error(e?.stack); $('chart').innerHTML = ''; $('legend').innerHTML = ''; $('clog').textContent = String(e.message || e); }
}

// ---- what-if + console (hosted engine only) -------------------------------
function table(t) {
  return `<div class="scroll"><table><thead><tr>${t.columns.map((c) => `<th>${esc(c)}</th>`).join('')}</tr></thead><tbody>${t.rows.map((r) => `<tr>${r.map((c) => `<td>${esc(c === null ? '' : fmt(c))}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`;
}
function setupWhatIf() {
  if (!HOST) return;
  $('wiplace').innerHTML = places.map((p) => `<option>${esc(p)}</option>`).join('');
  $('wi').onclick = () => {
    try {
      const r = HOST.hosted('what_if', { net: cur.ld, place: $('wiplace').value, delta: num('widelta', 1), runs: 16, samples: 12, t_end: num('wit', 10), places: places.slice(0, 4) });
      $('wiout').innerHTML = `<p class="log">Same random seed on both runs, so the difference comes from the change alone. Mean tokens at the end of the run:</p>` + table(r.table);
    } catch (e) { $('wiout').textContent = String(e.message || e); }
  };
}
const EXAMPLE = {
  verify: () => ({ properties: [{ kind: 'deadlock-free' }, { kind: 'bounded' }] }),
  what_if: () => ({ place: places[0], delta: 1, runs: 4, samples: 6, t_end: 5 }),
  sde: () => ({ t_end: 5, points: 6, seed: 1 }),
  validate: () => ({ reachability: true }),
  invariants: () => ({ kind: 'both' }),
  export: () => ({ format: 'json' }),
  edit: () => ({ ops: [{ op: 'set_details', name: 'edited copy' }] }),
  bundle_example: () => ({}),
  bundle_validate: () => ({ bundle: '(paste a bundle)' }),
};
function setupConsole() {
  if (!HOST) return;
  const names = HOST.hostedTools();
  $('tool').innerHTML = names.map((n) => `<option>${n}</option>`).join('');
  const fill = () => { const f = EXAMPLE[$('tool').value]; $('args').value = JSON.stringify(f ? f() : {}, null, 1); };
  $('tool').onchange = fill; fill();
  $('runtool').onclick = () => {
    const tool = $('tool').value; let args;
    try { args = JSON.parse($('args').value || '{}'); } catch (e) { $('toolout').textContent = 'The arguments are not valid JSON: ' + e.message; return; }
    if (!tool.startsWith('bundle_') && args.net === undefined && tool !== 'edit') args.net = cur.ld;
    if (tool === 'edit' && args.net === undefined) args.net = cur.ld;
    const t = performance.now();
    try { const out = JSON.stringify(HOST.hosted(tool, args), null, 1); $('toolout').textContent = `${tool} → ${(performance.now() - t).toFixed(1)} ms, ${out.length} bytes\n` + (out.length > 20000 ? out.slice(0, 20000) + '\n… (cut at 20000 characters)' : out); }
    catch (e) { $('toolout').textContent = `${tool} refused: ${e?.message ?? e}`; }
  };
}

// ---- load a model ---------------------------------------------------------
async function load(id) {
  cur = MODELS.find((m) => m.id === id);
  net?.free?.(); net = new PLAY.Net(cur.ld);
  places = net.places();
  document.querySelectorAll('#picker button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.id === id)));
  $('title').textContent = cur.title; $('blurb').textContent = cur.blurb;
  $('checks').innerHTML = '<li class="check"><span class="badge">…</span><div><b>Checking…</b></div></li>';
  renderPlay(); run('ode'); setupWhatIf();
  await new Promise((r) => setTimeout(r, 0));
  const t = performance.now();
  try { renderChecks(await runChecks(cur)); $('checkms').textContent = `checked in ${(performance.now() - t).toFixed(0)} ms`; }
  catch (e) { $('checks').innerHTML = `<li class="check bad"><span class="badge">✕</span><div><b>The checks did not finish</b><p>${esc(e?.message ?? e)}</p></div></li>`; }
}
$('picker').innerHTML = MODELS.map((m) => `<button type="button" data-id="${m.id}" aria-pressed="false">${esc(m.title)}</button>`).join('');
$('picker').onclick = (e) => { const b = e.target.closest('button'); if (b) load(b.dataset.id); };
$('reset').onclick = () => { net.reset(); renderPlay(); $('log').textContent = 'back to the starting marking'; run('ode'); };
$('ode').onclick = () => run('ode'); $('ssa').onclick = () => run('ssa');
setupConsole();
await load(MODELS[0].id);

// ---- self-report ----------------------------------------------------------
const net_reqs = performance.getEntriesByType('resource').filter((r) => !/^(data|blob):/.test(r.name)).length;
const kb = (n) => (n / 1024).toFixed(0) + ' KB';
$('foot').innerHTML = [`variant: ${VARIANT}`, `page ${kb(PAGE_BYTES)}`, `engine${HAS_SMALL && HAS_FULL ? 's' : ''} ${ENGINE_BYTES}`,
  ...Object.entries(timing).map(([k, v]) => `${k} engine ready in ${v.toFixed(0)} ms`), `ready ${(performance.now() - T0).toFixed(0)} ms after script start`, `network requests: ${net_reqs}`]
  .map((x) => `<span>${esc(x)}</span>`).join('');
window.__ready = { variant: VARIANT, network: net_reqs, timing, ms: performance.now() - T0 };
