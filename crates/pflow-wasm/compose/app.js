// The page: parts, canvas, links, proofs. `hosted`, `Net`, `verifyRecord` are
// inlined above (the engine and verify.mjs); nothing here touches the network.
const $ = (id) => document.getElementById(id);
const NS = 'http://www.w3.org/2000/svg';
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const el = (tag, attrs = {}, kids = []) => {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  for (const k of kids) e.appendChild(k);
  return e;
};

// ---- the engine, behind one door ------------------------------------------
function H(tool, args) {
  try { return [JSON.parse(JSON.stringify(hosted(tool, args))), null]; }
  catch (e) { return [null, String(e?.message ?? e)]; }
}

const CAT = H('bundle_parts', {})[0].parts;
const PART = Object.fromEntries(CAT.map((p) => [p.part, p]));

// ---- state -------------------------------------------------------------------
const S = { parts: [], links: [], sel: null };
let bundle = null, proof = null, flat = null, play = null, dragLink = null, hotLink = -1, timer = null;

const W = 204, ROW = 20, HEAD = 40;

function uniqueId(base) {
  let n = 1, id = base;
  while (S.parts.some((p) => p.id === id)) id = base + ++n;
  return id;
}

function bundleOf() {
  if (!S.parts.length) return null;
  const [r, e] = H('bundle_parts', { instances: S.parts.map((p) => ({ id: p.id, part: p.part, params: p.params })) });
  if (e) { setStatus(e); return null; }
  r.bundle.links = S.links.map((l) => structuredClone(l));
  return r.bundle;
}

function setStatus(msg) { $('inspect').dataset.err = msg || ''; if (msg) { $('inspect').innerHTML = `<span class="err">${esc(msg)}</span>`; } }

// ---- layout ------------------------------------------------------------------
const subnetOf = (id) => bundle?.subnets.find((s) => s.id === id);
function portsOf(id) {
  const s = subnetOf(id);
  return (s?.ports ?? []).map((p) => ({ ...p, transition: p.target === 'transition' }));
}
function side(p) { return p.kind === 'out' || p.kind === 'observe' ? 'r' : 'l'; }
function cardHeight(id) {
  const ps = portsOf(id);
  const l = ps.filter((p) => side(p) === 'l').length, r = ps.length - l;
  return HEAD + Math.max(l, r, 1) * ROW + 8;
}
function autoplace() {
  const colY = [14, 14, 14];
  S.parts.forEach((p, i) => {
    if (p.x != null) return;
    const c = i % 3;
    p.x = 16 + c * (W + 70); p.y = colY[c];
    colY[c] += cardHeight(p.id) + 24;
  });
}
function portPos(sub, port) {
  const p = S.parts.find((x) => x.id === sub);
  if (!p) return null;
  const ps = portsOf(sub);
  const q = ps.find((x) => x.id === port);
  if (!q) return null;
  const sd = side(q);
  const idx = ps.filter((x) => side(x) === sd).indexOf(q);
  return { x: p.x + (sd === 'l' ? 0 : W), y: p.y + HEAD + idx * ROW + ROW / 2, sd, port: q };
}

// ---- drawing -----------------------------------------------------------------
const KIND_OK = { token: 'token', data: 'data', event: 'event', guard: 'guard' };

function drawCanvas(lit = null) {
  const svg = $('svg');
  svg.replaceChildren();
  if (!S.parts.length) {
    svg.setAttribute('viewBox', '0 0 640 120');
    const t = el('text', { x: 20, y: 60, fill: 'var(--mut)', 'font-size': 14 });
    t.textContent = 'Nothing here yet. Add a part above, or pick an example.';
    svg.appendChild(t);
    return;
  }
  let maxX = 0, maxY = 0;
  const linkLayer = el('g'), cardLayer = el('g');
  svg.append(linkLayer, cardLayer);
  for (const p of S.parts) {
    const h = cardHeight(p.id);
    maxX = Math.max(maxX, p.x + W + 16); maxY = Math.max(maxY, p.y + h + 16);
    const g = el('g', { class: 'card' + (S.sel === p.id ? ' sel' : ''), 'data-id': p.id });
    const body = el('rect', { class: 'body', x: p.x, y: p.y, width: W, height: h, rx: 9 });
    const head = el('g', { style: 'cursor:grab', 'data-drag': p.id });
    head.appendChild(el('rect', { x: p.x, y: p.y, width: W, height: 34, fill: 'transparent' }));
    const t1 = el('text', { class: 'title', x: p.x + 10, y: p.y + 16 }); t1.textContent = p.id;
    const t2 = el('text', { class: 'sub', x: p.x + 10, y: p.y + 29 }); t2.textContent = PART[p.part].title + paramText(p);
    head.append(t1, t2);
    g.append(body, head);
    for (const q of portsOf(p.id)) {
      const pos = portPos(p.id, q.id);
      const key = p.id + ':' + q.id;
      const c = lit?.get(key);
      const cls = 'port' + (lit ? (c?.src ? ' src' : c?.ok ? ' lit' : ' dim') : '');
      const pg = el('g', { class: cls, 'data-sub': p.id, 'data-port': q.id, tabindex: 0 });
      const gx = pos.x, gy = pos.y;
      pg.appendChild(q.transition
        ? el('rect', { class: 'g', x: gx - 5.5, y: gy - 5.5, width: 11, height: 11 })
        : el('circle', { class: 'g', cx: gx, cy: gy, r: 6 }));
      const tx = el('text', { x: pos.sd === 'l' ? gx + 11 : gx - 11, y: gy + 3.5, 'text-anchor': pos.sd === 'l' ? 'start' : 'end' });
      tx.textContent = q.id;
      pg.appendChild(tx);
      const tip = el('title');
      tip.textContent = `${q.id}: ${q.transition ? 'transition' : 'place'}, ${q.kind}${q.schema ? ', ' + q.schema : ''}` + (c && !c.ok && !c.src ? `\n${c.why}` : '');
      pg.appendChild(tip);
      pg.appendChild(el('rect', { x: pos.sd === 'l' ? gx - 8 : gx - 92, y: gy - 9, width: 100, height: 18, fill: 'transparent' }));
      g.appendChild(pg);
    }
    cardLayer.appendChild(g);
  }
  S.links.forEach((l, i) => {
    const a = portPos(l.from.subnet, l.from.port), b = portPos(l.to.subnet, l.to.port);
    if (!a || !b) return;
    const d = 70;
    const dd = `M${a.x},${a.y} C${a.x + (a.sd === 'r' ? d : -d)},${a.y} ${b.x + (b.sd === 'r' ? d : -d)},${b.y} ${b.x},${b.y}`;
    // the drawn line ignores the pointer; a wide invisible twin takes the click
    linkLayer.appendChild(el('path', { class: `link k-${KIND_OK[l.kind]}` + (hotLink === i ? ' hot' : ''), d: dd, 'pointer-events': 'none' }));
    const hit = el('path', { class: 'link hit', 'data-link': i, d: dd, stroke: 'transparent', 'stroke-width': 14, fill: 'none' });
    const tip = el('title'); tip.textContent = `${l.kind}: ${l.from.subnet}:${l.from.port} → ${l.to.subnet}:${l.to.port} (click to remove)`;
    hit.appendChild(tip);
    linkLayer.appendChild(hit);
  });
  const vw = Math.max(maxX, 640), vh = Math.max(maxY, 160);
  svg.setAttribute('viewBox', `0 0 ${vw} ${vh}`);
  svg.style.minHeight = Math.min(vh, 640) + 'px';
}

function paramText(p) {
  const ks = Object.keys(PART[p.part].params);
  const bits = ks.map((k) => `${k} ${p.params?.[k] ?? PART[p.part].params[k].default}`);
  return bits.length ? ' · ' + bits.join(' ') : '';
}

// ---- palette, presets, inspector ---------------------------------------------
function buildPalette() {
  $('palette').replaceChildren(...CAT.map((c) => {
    const b = document.createElement('button');
    b.innerHTML = `${esc(c.title)}<small>${esc(c.part)}</small>`;
    b.title = c.why;
    b.onclick = () => addPart(c.part);
    return b;
  }));
}
function addPart(part, params = {}, id = null, x = null, y = null) {
  S.parts.push({ id: id ?? uniqueId(part.replace(/_/g, '-')), part, params, x, y });
  S.sel = S.parts[S.parts.length - 1].id;
  refresh(true);
}
const link = (kind, a, pa, b, pb) => ({ kind, from: { subnet: a, port: pa }, to: { subnet: b, port: pb } });
const PRESETS = [
  { name: 'Order pipeline', note: 'a queue feeds a worker pool and a payment saga: three event links', parts: [['orders', 'queue', { capacity: 3, initial: 2 }], ['workers', 'resource-pool', { capacity: 2 }], ['payment', 'saga', {}]],
    links: [link('event', 'orders', 'dequeue', 'workers', 'acquire'), link('event', 'orders', 'dequeue', 'payment', 'do:reserve'), link('event', 'workers', 'release', 'payment', 'commit')] },
  { name: 'Failing pool', note: 'a pool whose size is a machine count that can drop: the pool’s own law no longer holds as written', parts: [['pool', 'resource-pool', { capacity: 2 }], ['machines', 'failover', { machines: 2 }]],
    links: [link('token', 'machines', 'up', 'pool', 'available')] },
  { name: 'Guarded line', note: 'a rate limiter, a breaker and a lock all gate one queue’s dequeue', parts: [['jobs', 'queue', { capacity: 3, initial: 2 }], ['limiter', 'token_bucket', {}], ['breaker', 'circuit_breaker', {}], ['lock', 'mutex', {}]],
    links: [link('event', 'jobs', 'dequeue', 'limiter', 'admit'), link('event', 'jobs', 'dequeue', 'breaker', 'call'), link('event', 'jobs', 'dequeue', 'lock', 'lock')] },
  { name: 'Packing line', note: 'queue → batcher → kanban stage: token links share places, so the queue’s count law becomes part of a bigger one', parts: [['intake', 'queue', { capacity: 4, initial: 2 }], ['pack', 'batcher', { size: 2, capacity: 4 }], ['ship', 'kanban', { wip: 1, ready: 0 }]],
    links: [link('token', 'intake', 'out', 'pack', 'in'), link('token', 'pack', 'batches', 'ship', 'ready')] },
  { name: 'Rate vs pool (fails)', note: 'a rate limiter is a source and a sink; sharing its tokens with a pool breaks the pool’s law with nothing to account for it', parts: [['pool', 'resource-pool', { capacity: 2 }], ['limiter', 'token_bucket', {}]],
    links: [link('token', 'limiter', 'tokens', 'pool', 'available')] },
  { name: 'Green light only', note: 'a guard link lets the queue dequeue only on green', parts: [['line', 'queue', { capacity: 3, initial: 2 }], ['light', 'traffic_light', {}]],
    links: [link('guard', 'line', 'dequeue', 'light', 'green')] },
];
function buildPresets() {
  $('presets').replaceChildren(...PRESETS.map((p) => {
    const b = document.createElement('button');
    b.textContent = p.name; b.title = p.note;
    b.onclick = () => { S.parts = p.parts.map(([id, part, params]) => ({ id, part, params, x: null, y: null })); S.links = structuredClone(p.links); S.sel = null; bundle = null; refresh(true); };
    return b;
  }).concat(Object.assign(document.createElement('button'), { textContent: 'Clear', onclick: () => { S.parts = []; S.links = []; S.sel = null; refresh(true); } })));
}

function drawInspector() {
  const box = $('inspect');
  box.replaceChildren();
  const p = S.parts.find((x) => x.id === S.sel);
  if (p) {
    const spec = PART[p.part];
    const head = document.createElement('span');
    head.innerHTML = `<b>${esc(p.id)}</b> <span class="mut small">${esc(spec.title)} — ${esc(spec.why)}</span>`;
    box.appendChild(head);
    for (const [k, d] of Object.entries(spec.params)) {
      const lab = document.createElement('label'); lab.className = 'small'; lab.textContent = k + ' ';
      const inp = Object.assign(document.createElement('input'), { type: 'number', value: p.params[k] ?? d.default, min: d.min, max: d.max, title: d.about });
      inp.onchange = () => {
        const old = { ...p.params };
        p.params[k] = Number(inp.value);
        if (!bundleOf()) { p.params = old; refresh(true); return; }
        refresh(true);
      };
      lab.appendChild(inp); box.appendChild(lab);
    }
    const rm = Object.assign(document.createElement('button'), { textContent: 'Remove part' });
    rm.onclick = () => { S.parts = S.parts.filter((x) => x !== p); S.links = S.links.filter((l) => l.from.subnet !== p.id && l.to.subnet !== p.id); S.sel = null; refresh(true); };
    box.appendChild(rm);
  }
  // keyboard-friendly joining: pick a port, then one of the ports it can legally join
  if (S.parts.length > 1 && bundle) {
    const row = document.createElement('div'); row.className = 'row small'; row.style.width = '100%';
    const from = document.createElement('select'), to = document.createElement('select'), go = Object.assign(document.createElement('button'), { textContent: 'Join', disabled: true });
    from.innerHTML = '<option value="">join from…</option>' + S.parts.flatMap((q) => portsOf(q.id).map((x) => `<option value="${esc(q.id)}|${esc(x.id)}">${esc(q.id)}:${esc(x.id)}</option>`)).join('');
    to.innerHTML = '<option value="">…to</option>';
    let cands = [];
    from.onchange = () => {
      const [sub, port] = from.value.split('|');
      cands = from.value ? candidates(sub, port).filter((c) => c.ok) : [];
      to.innerHTML = '<option value="">' + (cands.length ? '…to' : 'nothing can join this') + '</option>' + cands.map((c, i) => `<option value="${i}">${esc(c.ref.subnet)}:${esc(c.ref.port)} (${esc(c.link.kind)})</option>`).join('');
      go.disabled = true;
    };
    to.onchange = () => { go.disabled = to.value === ''; };
    go.onclick = () => addLink(cands[Number(to.value)]);
    row.append(from, to, go);
    box.appendChild(row);
  }
}

// ---- links -------------------------------------------------------------------
function candidates(sub, port) {
  const b = bundleOf();
  if (!b) return [];
  const [r, e] = H('bundle_candidates', { bundle: b, from: { subnet: sub, port } });
  if (e) return [];
  return r.candidates.map((c) => ({ ...c, why: c.verdict?.errors?.[0]?.message ?? '' }));
}
function addLink(c) {
  const b = bundleOf();
  const [r, e] = H('bundle_add_link', { bundle: b, kind: c.link.kind, from: c.link.from, to: c.link.to });
  if (e) { setStatus(e); return; }
  S.links.push(r.link);
  refresh(true);
}

// ---- pointer handling ----------------------------------------------------------
function svgPoint(ev) {
  const svg = $('svg'), pt = svg.createSVGPoint();
  pt.x = ev.clientX; pt.y = ev.clientY;
  return pt.matrixTransform(svg.getScreenCTM().inverse());
}
function wirePointer() {
  const svg = $('svg');
  let drag = null;
  svg.addEventListener('pointerdown', (ev) => {
    const port = ev.target.closest?.('.port');
    const head = ev.target.closest?.('[data-drag]');
    const lk = ev.target.closest?.('.link');
    if (port) {
      ev.preventDefault();
      const sub = port.dataset.sub, pn = port.dataset.port;
      const lit = new Map(candidates(sub, pn).map((c) => [c.ref.subnet + ':' + c.ref.port, c]));
      lit.set(sub + ':' + pn, { src: true });
      dragLink = { sub, port: pn, lit };
      const a = portPos(sub, pn);
      dragLink.line = el('line', { x1: a.x, y1: a.y, x2: a.x, y2: a.y, stroke: 'var(--acc)', 'stroke-width': 2, 'stroke-dasharray': '4 3', 'pointer-events': 'none' });
      drawCanvas(lit); svg.appendChild(dragLink.line);
      svg.setPointerCapture(ev.pointerId);
    } else if (head) {
      const id = head.dataset.drag, p = S.parts.find((x) => x.id === id), pt = svgPoint(ev);
      drag = { p, dx: pt.x - p.x, dy: pt.y - p.y };
      S.sel = id; svg.setPointerCapture(ev.pointerId); drawInspector();
    } else if (lk) {
      S.links.splice(Number(lk.dataset.link), 1); hotLink = -1; refresh(true);
    }
  });
  svg.addEventListener('pointermove', (ev) => {
    if (drag) { const pt = svgPoint(ev); drag.p.x = Math.max(0, pt.x - drag.dx); drag.p.y = Math.max(0, pt.y - drag.dy); drawCanvas(); }
    else if (dragLink) { const pt = svgPoint(ev); dragLink.line.setAttribute('x2', pt.x); dragLink.line.setAttribute('y2', pt.y); }
  });
  const end = (ev) => {
    if (drag) { drag = null; drawCanvas(); }
    if (dragLink) {
      const under = document.elementFromPoint(ev.clientX, ev.clientY)?.closest?.('.port');
      const c = under && dragLink.lit.get(under.dataset.sub + ':' + under.dataset.port);
      dragLink = null;
      if (c && c.ok) addLink(c); else drawCanvas();
    }
  };
  svg.addEventListener('pointerup', end);
  svg.addEventListener('pointercancel', () => { drag = null; dragLink = null; drawCanvas(); });
  svg.addEventListener('click', (ev) => { const c = ev.target.closest?.('.card'); if (c && !ev.target.closest('.port')) { S.sel = c.dataset.id; drawCanvas(); drawInspector(); } });
  svg.addEventListener('keydown', (ev) => {
    const port = ev.target.closest?.('.port');
    if (port && (ev.key === 'Enter' || ev.key === ' ')) { ev.preventDefault(); const sel = $('inspect').querySelector('select'); if (sel) { sel.value = port.dataset.sub + '|' + port.dataset.port; sel.dispatchEvent(new Event('change')); sel.focus(); } }
  });
}

// ---- the proof, in plain language ------------------------------------------------
const VERDICT = {
  consistent: ['The parts fit together.', 'Every promise the parts make still holds, and the composite is bounded.'],
  consistent_with_notes: ['The parts fit, with things worth knowing.', 'Nothing is broken; some laws now live at a larger scale, or a link restricts a part.'],
  needs_attention: ['A part’s promise no longer holds as written.', 'The composite does conserve something larger that explains it. Decide whether that is what you meant.'],
  inconsistent: ['A join breaks a promise nothing accounts for.', 'Look at the links marked fail.'],
  inconclusive: ['Not settled yet.', 'The search stopped at its bound before it finished; nothing below claims what it did not see.'],
};
const LV = { ok: 'no finding', explained: 'explained', warn: 'needs attention', fail: 'broken' };
const ST = { proved: 'proved', refuted: 'refuted', unknown: 'unknown' };

function drawProof() {
  const box = $('proof');
  if (!S.parts.length) { box.innerHTML = '<span class="mut">Add parts to see what they promise and what joining them does.</span>'; return; }
  if (!proof) { box.innerHTML = '<span class="err">' + esc(proofErr || 'no proof') + '</span>'; return; }
  const v = proof.verdict.status, [head, sub] = VERDICT[v];
  let h = `<div class="verdict v-${v}"><b>${esc(head)}</b><div class="small">${esc(sub)}</div><div class="small mut" style="margin-top:3px">${esc(proof.says[0])}</div></div>`;
  h += '<h2>The composite</h2>';
  for (const c of proof.claims) {
    const wit = c.id === 'deadlock_free' && c.status === 'refuted' ? ` <button data-witness="1">Show the steps</button>` : '';
    // a workflow that finishes has a dead end by design: amber, not red; the links say whether one is new
    const dead = c.id === 'deadlock_free' && c.status === 'refuted';
    h += `<div class="item"><span class="chip ${dead ? 'c-explained' : 'c-' + c.status}">${dead ? 'has a dead end' : ST[c.status]}</span> <b>${esc(label(c.id))}</b><p>${esc(c.says)}${wit}</p></div>`;
  }
  h += '<h2 style="margin-top:12px">Each link</h2>';
  if (!proof.links.length) h += '<span class="mut small">No links yet: the parts are side by side. Drag from one port to another.</span>';
  proof.links.forEach((l, i) => {
    h += `<div class="item" data-i="${i}"><span class="chip c-${l.verdict}">${LV[l.verdict]}</span> <b>${esc(l.kind)}</b> <code>${esc(l.from.subnet)}:${esc(l.from.port ?? l.from.place ?? l.from.transition)}</code> → <code>${esc(l.to.subnet)}:${esc(l.to.port ?? l.to.place ?? l.to.transition)}</code>`;
    if (l.findings.length) h += '<ul>' + l.findings.map((f) => `<li>${esc(f.says)}</li>`).join('') + '</ul>';
    else h += `<p class="mut">${l.effect ? 'Taking this link out changes nothing about deadlocks, bounds or which transitions can fire; no part law is moved.' : 'Not tried against the composite (too many links).'}</p>`;
    h += '</div>';
  });
  const decl = proof.laws.filter((l) => l.declared), derived = proof.laws.length - decl.length;
  h += `<details style="margin-top:8px"><summary>The parts’ own laws (${decl.length} declared, ${derived} derived)</summary>`;
  for (const l of decl) h += `<div class="item"><span class="chip c-${l.status}">${l.status}</span> <b>${esc(l.part)}</b> <code>${esc(l.law)}</code>${l.holds_on_reachable === 'refuted' ? ' <span class="chip c-refuted">false in a reachable state</span>' : l.holds_on_reachable === 'proved' ? ' <span class="chip c-proved">holds everywhere reachable</span>' : ''}</div>`;
  h += '</details>';
  box.innerHTML = h;
  box.querySelectorAll('.item[data-i]').forEach((it) => {
    it.onmouseenter = () => { hotLink = Number(it.dataset.i); drawCanvas(); };
    it.onmouseleave = () => { hotLink = -1; drawCanvas(); };
  });
  const w = box.querySelector('[data-witness]');
  if (w) w.onclick = showWitness;
}
const label = (id) => ({ bounded: 'Bounded', deadlock_free: 'Never stuck', quasi_live: 'Everything can fire' }[id] ?? id);
let proofErr = '';

// ---- playing the composite ---------------------------------------------------------
const short = (flatId) => (flat?.members.transitions[flatId.replace(/:/g, '~')] ?? [flatId]).join(' + ');
function drawPlay(active = null) {
  const msg = $('playmsg'), marks = $('marks'), fire = $('fire');
  marks.replaceChildren(); fire.replaceChildren();
  if (!flat) { msg.textContent = S.parts.length ? (flat === null ? '' : '') : 'Add parts to play with them.'; return; }
  if (!flat.ok) { msg.innerHTML = `<span class="err">${esc(flat.error)}</span>`; return; }
  msg.textContent = 'Click an enabled transition to fire it; fused transitions fire all their members at once.';
  const m = play.marking(), en = new Set(play.enabled());
  for (const [pid, ms] of Object.entries(flat.members.places)) {
    const d = document.createElement('span'); d.className = 'mk' + (m[pid] ? ' on' : '');
    d.innerHTML = `<b>${m[pid]}</b>${esc(ms.join(' = '))}`; marks.appendChild(d);
  }
  for (const t of play.transitions()) {
    const b = document.createElement('button'); b.textContent = short(t); b.disabled = !en.has(t);
    if (t === active) b.style.borderColor = 'var(--acc)';
    b.onclick = () => { try { play.fire(t); $('log').textContent = 'fired ' + short(t); } catch (e) { $('log').textContent = String(e.message ?? e); } drawPlay(); };
    fire.appendChild(b);
  }
}
function showWitness() {
  const c = proof.claims.find((x) => x.id === 'deadlock_free');
  if (!play || !c?.certificate.trace) return;
  clearInterval(timer); play.reset(); drawPlay();
  const steps = c.certificate.trace.map((t) => t.replace(/:/g, '~'));
  let i = 0;
  timer = setInterval(() => {
    if (i >= steps.length) { clearInterval(timer); $('log').textContent = 'Reached a marking where nothing is enabled.'; drawPlay(); return; }
    try { play.fire(steps[i]); $('log').textContent = `step ${i + 1}/${steps.length}: ${short(steps[i])}`; } catch (e) { clearInterval(timer); $('log').textContent = String(e.message ?? e); }
    i++; drawPlay(steps[i]);
  }, 650);
  $('svg').scrollIntoView?.({ block: 'nearest' });
}

// ---- the whole refresh ---------------------------------------------------------------
let proveTimer = null;
function refresh(full) {
  bundle = bundleOf();
  autoplace();
  drawCanvas(); drawInspector();
  clearTimeout(proveTimer);
  $('proof').style.opacity = '.55';
  proveTimer = setTimeout(settle, 80);
}
function settle() {
  clearInterval(timer);
  $('recheckout').textContent = '';
  if (!bundle) { proof = null; flat = null; play = null; drawProof(); drawPlay(); $('netinfo').textContent = ''; $('proof').style.opacity = ''; return; }
  const [p, pe] = H('bundle_prove', { bundle, max_states: 2000 });
  proof = p; proofErr = pe ?? '';
  const [f, fe] = H('bundle_net', { bundle });
  flat = f ?? { ok: false, error: fe };
  try { play = flat.ok ? new Net(flat.net) : null; } catch (e) { play = null; flat = { ok: false, error: String(e.message ?? e) }; }
  $('netinfo').textContent = flat.ok ? `${flat.places} places, ${flat.transitions} transitions, ${flat.arcs} arcs` : '';
  $('log').textContent = '';
  drawProof(); drawPlay();
  $('proof').style.opacity = '';
}

// ---- re-checking -------------------------------------------------------------------------
$('recheck').onclick = () => {
  const out = $('recheckout');
  if (!proof || !bundle) { out.textContent = 'Nothing to check yet.'; return; }
  const [e, err] = H('bundle_check_proof', { bundle, proof });
  const engine = err ? `<span class="err">${esc(err)}</span>` : `${e.valid ? '<span class="chip c-proved">passed</span>' : '<span class="chip c-refuted">failed</span>'} the engine re-derived its answers and checked ${e.checked} things.`;
  let js = '';
  if (flat?.ok) {
    const r = verifyRecord(flat.net, flat.members, proof);
    js = `<div style="margin-top:6px">${r.valid ? '<span class="chip c-proved">passed</span>' : '<span class="chip c-refuted">failed</span>'} the separate script checked ${r.checks.length} certificates by arithmetic and replay on the flat net.` +
      (r.skipped.length ? `<details><summary>${r.skipped.length} claim(s) rest on the engine’s finished search</summary><ul>${r.skipped.map((s) => `<li>${esc(s)}</li>`).join('')}</ul></details>` : '') +
      (r.valid ? '' : '<ul>' + r.checks.filter((c) => !c.ok).map((c) => `<li class="err">${esc(c.what)}: ${esc(c.detail)}</li>`).join('') + '</ul>') + '</div>';
  } else js = '<div class="mut" style="margin-top:6px">The composite cannot be written as a plain net, so the separate script has nothing to read.</div>';
  out.innerHTML = engine + js + `<div class="mut" style="margin-top:6px">Both trust the flattening itself (held to go-pflow’s goldens) and the firing rule.</div>`;
};
$('dl').onclick = () => {
  if (!proof) return;
  const a = Object.assign(document.createElement('a'), { href: URL.createObjectURL(new Blob([JSON.stringify({ bundle, proof }, null, 2)], { type: 'application/json' })), download: 'composition-proof.json' });
  document.body.appendChild(a); a.click(); a.remove();
};
$('copynet').onclick = async () => {
  if (!flat?.ok) return;
  const txt = JSON.stringify(flat.net, null, 2);
  try { await navigator.clipboard.writeText(txt); $('netinfo').textContent = 'copied'; }
  catch { $('netinfo').innerHTML = '<details open><summary>copy from here</summary><div class="pre"></div></details>'; $('netinfo').querySelector('.pre').textContent = txt; }
};
$('reset').onclick = () => { clearInterval(timer); play?.reset(); $('log').textContent = ''; drawPlay(); };

buildPalette(); buildPresets(); wirePointer();
PRESETS[0].parts.forEach(([id, part, params]) => S.parts.push({ id, part, params, x: null, y: null }));
S.links = structuredClone(PRESETS[0].links);
refresh(true);
// for automation and tests
window.__compose = { S, H, addPart, candidates, addLink, refresh, state: () => ({ bundle, proof, flat }) };
