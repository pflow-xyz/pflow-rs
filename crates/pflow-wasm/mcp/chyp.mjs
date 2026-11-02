// chyp-rs (an unofficial Rust port of part of Chyp, Apache-2.0): string-diagram
// rewriting and proof checking, as WebAssembly. Loaded lazily; no network.
import { readFileSync } from 'node:fs';
import { z } from 'zod';

let mod = null;
async function chyp() {
  if (mod) return mod;
  const m = await import('./chyp/chyp_rs.js');
  m.initSync({ module: readFileSync(new URL('./chyp/chyp_rs_bg.wasm', import.meta.url)) });
  return (mod = m);
}

const sessions = new Map();
let last = null;
const session = (name) => {
  const key = name ?? last;
  const s = key == null ? null : sessions.get(key);
  if (!s) throw new Error(key == null ? 'no theory loaded: call chyp_check first' : `no theory named "${key}"`);
  return s;
};

export function register(tool) {
  tool('chyp_check',
    'Parse and check a Chyp theory (string-diagram rewriting: gens, rules, rewrite proofs). Returns each part with its status and any errors, without diagrams; the theory is kept under a name for chyp_part and chyp_rule.',
    { source: z.string().describe('The theory text (.chyp syntax)'),
      name: z.string().optional().describe('Name to keep it under (default "theory")') },
    async ({ source, name }) => {
      const m = await chyp();
      const key = name ?? 'theory';
      sessions.get(key)?.free?.();
      const s = new m.Session(source);
      sessions.set(key, s);
      last = key;
      return { name: key, upstream: m.upstream_commit(), ...JSON.parse(s.check_all()) };
    });

  tool('chyp_part',
    'One checked part of a kept theory with its diagrams and the proof state after it. Diagram layouts make this large; ask for one part at a time.',
    { index: z.number().int().min(0), name: z.string().optional() },
    async ({ index, name }) => { await chyp(); return JSON.parse(session(name).part(index)); });

  tool('chyp_rule',
    'A named rule of a kept theory: its two sides as diagrams.',
    { rule: z.string(), name: z.string().optional() },
    async ({ rule, name }) => {
      await chyp();
      const r = JSON.parse(session(name).rule(rule));
      if (r === null) throw new Error(`no rule named "${rule}"`);
      return r;
    });
}
