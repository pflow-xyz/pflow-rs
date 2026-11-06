// verify.mjs: re-checks a bundle_prove record with arithmetic and replay only,
// on the flat net bundle_net returns. No engine call: a reader that does not
// trust the Rust engine needs this file and the two JSON documents. Pure (no
// DOM, no wasm), so the page and node tests share it.
//
// What it does NOT do: flatten the bundle (that is the engine's, held to
// go-pflow's goldens) or re-run the exhaustive searches (their "complete"
// verdicts rest on the engine; a refuted deadlock and a counterexample, which
// carry witnesses, are fully re-checked here).

const id = (s) => s.replace(/:/g, '~'); // bundle_net writes ':' as '~'

/** The net as the checker reads it: indices, capacities, M0 and arcs. */
export function readNet(net) {
  const places = Object.keys(net.places);
  const transitions = Object.keys(net.transitions);
  const pi = new Map(places.map((p, i) => [p, i]));
  const ti = new Map(transitions.map((t, i) => [t, i]));
  const m0 = places.map((p) => net.places[p].initial[0] | 0);
  const cap = places.map((p) => net.places[p].capacity[0] | 0);
  const pre = transitions.map(() => []); // [place, w, kind]  kind: n|r|i
  const post = transitions.map(() => []);
  const C = places.map(() => transitions.map(() => 0));
  for (const a of net.arcs) {
    const w = a.weight[0] | 0;
    const srcP = pi.has(a.source);
    if (!a.inhibitTransition) {
      if (srcP) { pre[ti.get(a.target)].push([pi.get(a.source), w, 'n']); C[pi.get(a.source)][ti.get(a.target)] -= w; }
      else { post[ti.get(a.source)].push([pi.get(a.target), w]); C[pi.get(a.target)][ti.get(a.source)] += w; }
    } else if (srcP) pre[ti.get(a.target)].push([pi.get(a.source), w, 'i']); // inhibitor
    else pre[ti.get(a.source)].push([pi.get(a.target), w, 'r']); // read arc
  }
  return { places, transitions, pi, ti, m0, cap, pre, post, C };
}

function enabled(N, t, m) {
  const consumed = new Map();
  for (const [p, w, k] of N.pre[t]) {
    if (k === 'n') { if (m[p] < w) return false; consumed.set(p, (consumed.get(p) ?? 0) + w); }
    else if (k === 'r') { if (m[p] < w) return false; }
    else if (m[p] >= w) return false; // inhibitor
  }
  // capacity is a post-firing bound, netting out what the firing consumes
  const produced = new Map();
  for (const [p, w] of N.post[t]) produced.set(p, (produced.get(p) ?? 0) + w);
  for (const [p, w] of produced) {
    const delta = w - (consumed.get(p) ?? 0);
    if (N.cap[p] > 0 && delta > 0 && m[p] + delta > N.cap[p]) return false;
  }
  return true;
}

function fire(N, t, m) {
  const o = m.slice();
  for (const [p, w, k] of N.pre[t]) if (k === 'n') o[p] -= w;
  for (const [p, w] of N.post[t]) o[p] += w;
  return o;
}

/** y·C per transition, for y a {flatPlaceId: coefficient} object. Non-zero entries as {flatTransitionId: value}. */
function residual(N, y) {
  const out = {};
  N.transitions.forEach((t, j) => {
    let r = 0;
    for (const [p, k] of Object.entries(y)) {
      const i = N.pi.get(id(p));
      if (i === undefined) return (out.__unknown = p);
      r += k * N.C[i][j];
    }
    if (r !== 0) out[t] = r;
  });
  return out;
}

const keyed = (o) => Object.fromEntries(Object.entries(o).map(([k, v]) => [id(k), v]));
const same = (a, b) => JSON.stringify(Object.entries(a).sort()) === JSON.stringify(Object.entries(b).sort());
const dot = (y, v, N) => Object.entries(y).reduce((s, [p, k]) => s + k * v[N.pi.get(id(p))], 0);

function replay(N, trace) {
  let m = N.m0.slice();
  for (const name of trace) {
    const t = N.ti.get(id(name));
    if (t === undefined || !enabled(N, t, m)) return null;
    m = fire(N, t, m);
  }
  return m;
}

const asMarking = (N, m) => Object.fromEntries(N.places.map((p, i) => [p, m[i]]).filter(([, v]) => v !== 0));

/**
 * @param net      the `net` of a bundle_net answer
 * @param members  the `members` of that answer
 * @param record   a bundle_prove answer
 * @returns {{valid:boolean, checks:{what:string, ok:boolean, detail:string}[], skipped:string[]}}
 */
export function verifyRecord(net, members, record) {
  const N = readNet(net);
  const checks = [];
  const skipped = [];
  const add = (what, ok, detail) => checks.push({ what, ok: !!ok, detail });
  const partPlaces = (part) =>
    Object.entries(members.places).filter(([, ms]) => ms.some((m) => m.startsWith(part + '/'))).map(([f]) => f);

  add('composite net', record.composite.places === N.places.length && record.composite.transitions === N.transitions.length,
    `${N.places.length} places, ${N.transitions.length} transitions`);

  for (const l of record.laws) {
    const what = `law ${l.part} / ${l.id}`;
    const y = l.lifted;
    const res = residual(N, y);
    if ('__unknown' in res) { add(what, false, `place ${res.__unknown} is not in the net`); continue; }
    const claimed = keyed(l.residual);
    if (l.status === 'preserved') add(what, Object.keys(res).length === 0 && Object.keys(claimed).length === 0, 'y·C = 0');
    else {
      let ok = same(res, claimed) && Object.keys(res).length > 0;
      let detail = `moved by ${Object.keys(res).length} transition(s)`;
      if (l.status === 'extended') {
        const z = l.extended_by.z, [zn, yn] = l.extended_by.ratio;
        const zr = residual(N, z);
        const prop = partPlaces(l.part).every((p) => (z[p] ?? 0) * yn === (y[p] ?? 0) * zn);
        ok = ok && Object.keys(zr).length === 0 && Object.values(z).every((k) => k >= 0) && prop && zn !== 0;
        detail = 'z·C = 0, z ≥ 0, z is a fixed multiple of the law on the part\'s places';
      }
      add(what, ok, detail);
    }
    if (l.declared && l.holds_on_reachable !== 'refuted') {
      add(`${what} (start)`, dot(y, N.m0, N) === l.value, `y·M0 = ${dot(y, N.m0, N)}, declared ${l.value}`);
    }
    if (l.holds_on_reachable === 'refuted') {
      const end = replay(N, l.counterexample.trace ?? []);
      const ok = end && same(asMarking(N, end), keyed(l.counterexample.marking)) && dot(y, end, N) !== l.value;
      add(`${what} counterexample`, ok, 'the trace replays to a marking where the law\'s sum differs');
    }
  }

  for (const c of record.claims) {
    const what = `claim ${c.id}`;
    const cert = c.certificate;
    if (c.id === 'bounded' && c.status === 'proved') {
      const z = cert.z, zr = residual(N, z);
      const z0 = dot(z, N.m0, N);
      let ok = Object.keys(zr).length === 0 && Object.values(z).every((k) => k >= 0) && cert.z_value === z0;
      let uncertified = 0;
      for (const [i, p] of N.places.entries()) {
        const e = cert.places?.[p] ?? cert.places?.[p.replace(/~/g, ':')];
        const zp = z[p] ?? z[p.replace(/~/g, ':')] ?? 0;
        const good = e && ((e.by === 'invariant' && zp > 0 && e.bound === Math.floor(z0 / zp)) || (e.by === 'capacity' && N.cap[i] > 0 && e.bound === N.cap[i]));
        if (!good) uncertified++;
      }
      if (uncertified) { skipped.push(`${what}: ${uncertified} place(s) rest on a finished search`); add(what, ok, 'z·C = 0 and z ≥ 0 hold; the rest rests on a finished search (not re-run here)'); }
      else add(what, ok, 'z·C = 0, z ≥ 0, every place bounded by z or a capacity');
    } else if (c.id === 'deadlock_free' && c.status === 'refuted') {
      const end = replay(N, cert.trace);
      const dead = end && N.transitions.every((_, t) => !enabled(N, t, end));
      add(what, dead && same(asMarking(N, end), keyed(cert.marking)), 'the witness replays to the stated marking, where nothing is enabled');
    } else {
      skipped.push(`${what} (${c.status}): rests on an exhaustive search; the engine's count is not re-run here`);
    }
  }
  return { valid: checks.every((c) => c.ok), checks, skipped };
}
