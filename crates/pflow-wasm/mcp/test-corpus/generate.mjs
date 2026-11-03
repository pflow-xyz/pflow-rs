#!/usr/bin/env node
// Writes the synthetic corpus (synthetic/ and refused/): hand-built textbook nets in
// the pflow.xyz simple JSON shape (places/transitions/arcs), each with the verdicts
// it is meant to produce recorded in expectations.json. Deterministic; rerun to refresh.
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
const here = dirname(fileURLToPath(import.meta.url));
mkdirSync(join(here, 'synthetic'), { recursive: true });
mkdirSync(join(here, 'refused'), { recursive: true });
const out = (dir, name, net) => writeFileSync(join(here, dir, `${name}.json`), JSON.stringify(net, null, 1) + '\n');
const P = (id, initial = 0, extra = {}) => ({ id, initial, ...extra });
const T = (id, rate, extra = {}) => ({ id, ...(rate != null ? { rate } : {}), ...extra });
const A = (from, to, weight, extra = {}) => ({ from, to, ...(weight != null ? { weight } : {}), ...extra });

// SIR epidemic (mass action: infect needs S and I)
out('synthetic', 'sir', { name: 'sir',
  places: [P('S', 99), P('I', 1), P('R')],
  transitions: [T('infect', 0.3), T('recover', 0.1)],
  arcs: [A('S', 'infect'), A('I', 'infect'), A('infect', 'I', 2), A('I', 'recover'), A('recover', 'R')] });

// SEIR with a birth-death source/sink (unbounded in S via birth)
out('synthetic', 'seir-open', { name: 'seir-open',
  places: [P('S', 50), P('E'), P('I', 2), P('R')],
  transitions: [T('birth', 1), T('expose', 0.02), T('incubate', 0.5), T('recover', 0.2), T('die_S', 0.01)],
  arcs: [A('birth', 'S'), A('S', 'expose'), A('I', 'expose'), A('expose', 'E'), A('expose', 'I'),
         A('E', 'incubate'), A('incubate', 'I'), A('I', 'recover'), A('recover', 'R'), A('S', 'die_S')] });

// Producer/consumer with a bounded buffer (capacity counter), live and 1-bounded per component
out('synthetic', 'producer-consumer', { name: 'producer-consumer',
  places: [P('idle_p', 1), P('item', 0), P('space', 3), P('idle_c', 1)],
  transitions: [T('produce', 1), T('consume', 1)],
  arcs: [A('idle_p', 'produce'), A('space', 'produce'), A('produce', 'idle_p'), A('produce', 'item'),
         A('item', 'consume'), A('idle_c', 'consume'), A('consume', 'idle_c'), A('consume', 'space')] });

// Producer/consumer with NO bound: buffer grows without limit
out('synthetic', 'producer-consumer-unbounded', { name: 'producer-consumer-unbounded',
  places: [P('idle_p', 1), P('item', 0), P('idle_c', 1)],
  transitions: [T('produce', 1), T('consume', 0.5)],
  arcs: [A('idle_p', 'produce'), A('produce', 'idle_p'), A('produce', 'item'),
         A('item', 'consume'), A('idle_c', 'consume'), A('consume', 'idle_c')] });

// Mutex: two processes, one lock. Live, 1-bounded, mutual exclusion invariant crit1+crit2<=1
out('synthetic', 'mutex', { name: 'mutex',
  places: [P('idle1', 1), P('wait1'), P('crit1'), P('idle2', 1), P('wait2'), P('crit2'), P('lock', 1)],
  transitions: [T('req1', 1), T('enter1', 5), T('exit1', 2), T('req2', 1), T('enter2', 5), T('exit2', 2)],
  arcs: [A('idle1', 'req1'), A('req1', 'wait1'), A('wait1', 'enter1'), A('lock', 'enter1'), A('enter1', 'crit1'),
         A('crit1', 'exit1'), A('exit1', 'idle1'), A('exit1', 'lock'),
         A('idle2', 'req2'), A('req2', 'wait2'), A('wait2', 'enter2'), A('lock', 'enter2'), A('enter2', 'crit2'),
         A('crit2', 'exit2'), A('exit2', 'idle2'), A('exit2', 'lock')] });

// Broken mutex: no lock, both can be critical (mutual exclusion violated)
out('synthetic', 'mutex-broken', { name: 'mutex-broken',
  places: [P('idle1', 1), P('crit1'), P('idle2', 1), P('crit2')],
  transitions: [T('enter1', 1), T('exit1', 1), T('enter2', 1), T('exit2', 1)],
  arcs: [A('idle1', 'enter1'), A('enter1', 'crit1'), A('crit1', 'exit1'), A('exit1', 'idle1'),
         A('idle2', 'enter2'), A('enter2', 'crit2'), A('crit2', 'exit2'), A('exit2', 'idle2')] });

// Dining philosophers, N of them. Naive left-then-right fork pickup: deadlocks.
function philosophers(n, ordered) {
  const places = [], transitions = [], arcs = [];
  for (let i = 0; i < n; i++) { places.push(P(`think${i}`, 1), P(`fork${i}`, 1), P(`hasL${i}`), P(`eat${i}`)); }
  for (let i = 0; i < n; i++) {
    const l = ordered && i === n - 1 ? (i + 1) % n : i, r = ordered && i === n - 1 ? i : (i + 1) % n;
    transitions.push(T(`takeL${i}`, 1), T(`takeR${i}`, 1), T(`put${i}`, 1));
    arcs.push(A(`think${i}`, `takeL${i}`), A(`fork${l}`, `takeL${i}`), A(`takeL${i}`, `hasL${i}`),
      A(`hasL${i}`, `takeR${i}`), A(`fork${r}`, `takeR${i}`), A(`takeR${i}`, `eat${i}`),
      A(`eat${i}`, `put${i}`), A(`put${i}`, `think${i}`), A(`put${i}`, `fork${l}`), A(`put${i}`, `fork${r}`));
  }
  return { places, transitions, arcs };
}
for (const n of [2, 3, 5]) out('synthetic', `philosophers-${n}`, { name: `philosophers-${n}`, ...philosophers(n, false) });
// Resource-ordered variant (last philosopher takes forks in the opposite order): deadlock-free
out('synthetic', 'philosophers-3-ordered', { name: 'philosophers-3-ordered', ...philosophers(3, true) });

// Textbook deadlock: two processes grab two resources in opposite orders
out('synthetic', 'deadlock-two-locks', { name: 'deadlock-two-locks',
  places: [P('a', 1), P('b', 1), P('p1', 1), P('p1a'), P('p1done'), P('p2', 1), P('p2b'), P('p2done')],
  transitions: [T('p1_take_a', 1), T('p1_take_b', 1), T('p2_take_b', 1), T('p2_take_a', 1)],
  arcs: [A('p1', 'p1_take_a'), A('a', 'p1_take_a'), A('p1_take_a', 'p1a'),
         A('p1a', 'p1_take_b'), A('b', 'p1_take_b'), A('p1_take_b', 'p1done'),
         A('p2', 'p2_take_b'), A('b', 'p2_take_b'), A('p2_take_b', 'p2b'),
         A('p2b', 'p2_take_a'), A('a', 'p2_take_a'), A('p2_take_a', 'p2done')] });

// Pure sink: terminates, then dead (deadlock but bounded)
out('synthetic', 'pipeline-terminates', { name: 'pipeline-terminates',
  places: [P('in', 4), P('mid'), P('out')],
  transitions: [T('stage1', 2), T('stage2', 1)],
  arcs: [A('in', 'stage1'), A('stage1', 'mid'), A('mid', 'stage2'), A('stage2', 'out')] });

// Livelock (from the package's own test): deadlock-free, L1, not L4
out('synthetic', 'livelock', { name: 'livelock',
  places: [P('p', 1), P('q'), P('r'), P('s')],
  transitions: ['t', 'u', 'x', 'v', 'w'].map((id) => T(id)),
  arcs: [['p', 't'], ['t', 'q'], ['q', 'u'], ['u', 'p'], ['p', 'x'], ['x', 'r'], ['r', 'v'], ['v', 's'], ['s', 'w'], ['w', 'r']].map(([f, t]) => A(f, t)) });

// Unbounded: a source transition with no input
out('synthetic', 'unbounded-source', { name: 'unbounded-source',
  places: [P('p')], transitions: [T('t', 1)], arcs: [A('t', 'p')] });
// Unbounded with growth by doubling (weighted)
out('synthetic', 'unbounded-doubling', { name: 'unbounded-doubling',
  places: [P('p', 1)], transitions: [T('dbl', 0.1)], arcs: [A('p', 'dbl'), A('dbl', 'p', 2)] });
// Unbounded cycle that pumps a counter
out('synthetic', 'unbounded-pump', { name: 'unbounded-pump',
  places: [P('a', 1), P('b'), P('count')],
  transitions: [T('go', 1), T('back', 1)],
  arcs: [A('a', 'go'), A('go', 'b'), A('go', 'count'), A('b', 'back'), A('back', 'a')] });

// Inhibitor arcs: `serve` is blocked while the lock holds a token
out('synthetic', 'inhibitor-gate', { name: 'inhibitor-gate',
  places: [P('queue', 3), P('done'), P('lock', 0)],
  transitions: [T('serve', 2), T('halt', 1), T('release', 1)],
  arcs: [A('queue', 'serve'), A('serve', 'done'), A('lock', 'serve', 1, { type: 'inhibitor' }),
         A('done', 'halt'), A('halt', 'lock'), A('lock', 'release')] });
// Inhibitor with weight 2: transition fires only while place has < 2 tokens
out('synthetic', 'inhibitor-weighted', { name: 'inhibitor-weighted',
  places: [P('src', 5), P('cap'), P('sink')],
  transitions: [T('move', 1), T('drain', 0.1)],
  arcs: [A('src', 'move'), A('move', 'cap'), A('cap', 'move', 2, { type: 'inhibitor' }), A('cap', 'drain'), A('drain', 'sink')] });
// Read arc: `catalyst` is required but not consumed
out('synthetic', 'read-arc', { name: 'read-arc',
  places: [P('substrate', 5), P('catalyst', 1), P('product')],
  transitions: [T('react', 1)],
  arcs: [A('substrate', 'react'), A('catalyst', 'react', 1, { type: 'read' }), A('react', 'product')] });

// Weighted arcs: 2A + B -> 3C, C -> A, conservation-style invariants
out('synthetic', 'weighted', { name: 'weighted',
  places: [P('A', 6), P('B', 3), P('C')],
  transitions: [T('synth', 0.5), T('decay', 0.2)],
  arcs: [A('A', 'synth', 2), A('B', 'synth', 1), A('synth', 'C', 3), A('C', 'decay', 3), A('decay', 'A', 2), A('decay', 'B', 1)] });
// Weight 0 and fractional weight edge cases
out('synthetic', 'weight-edge', { name: 'weight-edge',
  places: [P('a', 3), P('b')],
  transitions: [T('t', 1)],
  arcs: [A('a', 't', 0.5), A('t', 'b', 2)] });

// Capacity: bounded place blocks the producer
out('synthetic', 'capacity', { name: 'capacity',
  places: [P('src', 5), P('buf', 0, { capacity: 2 }), P('out')],
  transitions: [T('fill', 1), T('drain', 0.2)],
  arcs: [A('src', 'fill'), A('fill', 'buf'), A('buf', 'drain'), A('drain', 'out')] });

// Enzyme kinetics: E + S <-> ES -> E + P (reversible, conservation laws, cyclic)
out('synthetic', 'enzyme', { name: 'enzyme',
  places: [P('E', 10), P('S', 40), P('ES'), P('P')],
  transitions: [T('bind', 0.05), T('unbind', 0.2), T('catalyze', 0.3)],
  arcs: [A('E', 'bind'), A('S', 'bind'), A('bind', 'ES'), A('ES', 'unbind'), A('unbind', 'E'), A('unbind', 'S'),
         A('ES', 'catalyze'), A('catalyze', 'E'), A('catalyze', 'P')] });

// Stress: token ring of N places (state space N), grid of independent toggles (2^N), counters.
const ring = (n) => ({ name: `ring-${n}`, places: Array.from({ length: n }, (_, i) => P(`r${i}`, i === 0 ? 1 : 0)),
  transitions: Array.from({ length: n }, (_, i) => T(`s${i}`, 1)),
  arcs: Array.from({ length: n }, (_, i) => [A(`r${i}`, `s${i}`), A(`s${i}`, `r${(i + 1) % n}`)]).flat() });
out('synthetic', 'ring-50', ring(50));
// k independent two-state toggles: reachable states 2^k (k=10 -> 1024, k=16 -> 65536 exceeds the 20000 cap)
const toggles = (k) => ({ name: `toggles-${k}`, places: Array.from({ length: k }, (_, i) => [P(`off${i}`, 1), P(`on${i}`)]).flat(),
  transitions: Array.from({ length: k }, (_, i) => [T(`up${i}`, 1), T(`down${i}`, 1)]).flat(),
  arcs: Array.from({ length: k }, (_, i) => [A(`off${i}`, `up${i}`), A(`up${i}`, `on${i}`), A(`on${i}`, `down${i}`), A(`down${i}`, `off${i}`)]).flat() });
out('synthetic', 'toggles-10', toggles(10));
out('synthetic', 'toggles-16', toggles(16));
// m tokens distributed across n places by moves: C(m+n-1, n-1) states
const spread = (n, m) => ({ name: `spread-${n}x${m}`, places: Array.from({ length: n }, (_, i) => P(`p${i}`, i === 0 ? m : 0)),
  transitions: Array.from({ length: n - 1 }, (_, i) => [T(`f${i}`, 1), T(`b${i}`, 1)]).flat(),
  arcs: Array.from({ length: n - 1 }, (_, i) => [A(`p${i}`, `f${i}`), A(`f${i}`, `p${i + 1}`), A(`p${i + 1}`, `b${i}`), A(`b${i}`, `p${i}`)]).flat() });
out('synthetic', 'spread-6x8', spread(6, 8)); // C(13,5)=1287 states
out('synthetic', 'spread-8x12', spread(8, 12)); // C(19,7)=50388 states -> exceeds cap
// Big structural net: 200-stage pipeline (many places/transitions, small state space)
const chain = (n) => ({ name: `chain-${n}`, places: Array.from({ length: n + 1 }, (_, i) => P(`c${i}`, i === 0 ? 3 : 0)),
  transitions: Array.from({ length: n }, (_, i) => T(`k${i}`, 1)),
  arcs: Array.from({ length: n }, (_, i) => [A(`c${i}`, `k${i}`), A(`k${i}`, `c${i + 1}`)]).flat() });
out('synthetic', 'chain-200', chain(200));

// ---- must be refused cleanly ----
const base = { places: [P('a', 2), P('b')], arcs: [A('a', 't'), A('t', 'b')] };
out('refused', 'guard', { name: 'guard', ...base, transitions: [T('t', 1, { guard: 'a >= 2' })] });
out('refused', 'stages', { name: 'stages', ...base, transitions: [T('t', 1, { stages: 3 })] });
out('refused', 'schedule', { name: 'schedule', ...base, transitions: [T('t', 1, { schedule: [{ until: 5, value: 2 }, { until: 10, value: 0.5 }] })] });
out('refused', 'malformed-arc-target', { name: 'bad-arc', places: [P('a', 1)], transitions: [T('t', 1)], arcs: [A('a', 'nope')] });
out('refused', 'duplicate-ids', { name: 'dup', places: [P('a', 1), P('a', 2)], transitions: [T('t', 1)], arcs: [A('a', 't')] });
out('refused', 'negative-initial', { name: 'neg', places: [P('a', -1)], transitions: [T('t', 1)], arcs: [A('a', 't')] });
out('refused', 'empty', { name: 'empty', places: [], transitions: [], arcs: [] });
console.log('written');
