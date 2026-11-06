// The pflow.xyz/mcp tools, answered locally by pflow-hosted (crates/pflow-hosted)
// through the engine's `hosted` build. hosted-tools.json holds each tool's name
// (the hosted name without `pflow_`), description and input schema as the hosted
// server lists them, so a call written for pflow.xyz/mcp works here unchanged.
//
// One addition: wherever the hosted tool takes `net` (a pflow.xyz JSON-LD object),
// this server also takes the name of a net loaded with load_net (in either shape:
// load_net writes it as JSON-LD, crates/pflow-hosted/src/convert.rs), and with no
// `net` at all uses the most recently loaded one (edit excepted: there no net
// means an empty one, as hosted).
import { readFileSync } from 'node:fs';
import { Worker } from 'node:worker_threads';
import { z } from 'zod';

const read = (f) => JSON.parse(readFileSync(new URL(f, import.meta.url), 'utf8')).tools;
// the hosted server's tools, then this package's own (compose-tools.json: ahead of the hosted server)
const tools = [...read('./hosted-tools.json'), ...read('./compose-tools.json')];

const LOCAL_NOTE =
  ' Runs locally and offline on pflow-rs (its port of this pflow.xyz/mcp tool, with the same arguments and answer shape); where this says go-pflow, read the pflow-rs port of it.';

const netArg = z.union([z.string(), z.record(z.string(), z.any())]).optional()
  .describe('A pflow.xyz net (JSON-LD, the editor\'s format), or the name of a net loaded with load_net; defaults to the most recently loaded');

// A hosted JSON Schema property as a permissive zod type: the type only, so a
// bad value reaches the engine and is refused in the hosted tool's own words.
function zodOf(p = {}) {
  let t;
  if (Array.isArray(p.enum) && p.enum.every((e) => typeof e === 'string')) t = z.enum(p.enum);
  else if (p.type === 'string') t = z.string();
  else if (p.type === 'number') t = z.number();
  else if (p.type === 'integer') t = z.number();
  else if (p.type === 'boolean') t = z.boolean();
  else if (p.type === 'array') t = z.array(z.any());
  else if (p.type === 'object') t = z.record(z.string(), z.any());
  else t = z.any();
  return p.description ? t.describe(p.description) : t;
}

/** The pflow.xyz JSON-LD a `net` argument names: an object as is, a string a
 *  loaded net's (load_net writes every loaded net in that form). */
export function resolveNet(value, { jsonld, last }) {
  const named = (key) => {
    const e = jsonld.get(key);
    if (!e) throw new Error(`no net named "${key}"; loaded: ${[...jsonld.keys()].join(', ') || 'none'}`);
    if (!e.doc) throw new Error(`net "${key}" cannot be written as a pflow.xyz net: ${e.unsupported}`);
    return e.doc;
  };
  if (value == null) {
    if (last() == null) throw new Error('net: give a pflow.xyz net, or load one with load_net first');
    return named(last());
  }
  if (typeof value === 'string') return named(value);
  return value;
}

// ---- timeouts -------------------------------------------------------------
// reachability, validate and verify search a state space and can run for a long
// time (or trap). The engine is WebAssembly on this thread and a running call
// cannot be interrupted from JS, so these three run in a worker thread that
// holds its own copy of the engine: the server stays responsive, and on timeout
// the worker is terminated outright (which actually stops the search, where
// racing a timer against a synchronous call would not) and a fresh one is
// started for the next call. A wasm trap retires the worker too, since an
// instance that has trapped is not safe to reuse. One call runs at a time.
export const DEFAULT_TIMEOUT_MS = 10000;
export const MAX_TIMEOUT_MS = 120000;
export const timeoutArg = z.number().int().min(1).max(MAX_TIMEOUT_MS).optional()
  .describe(`Wall-clock limit in ms (default ${DEFAULT_TIMEOUT_MS}, max ${MAX_TIMEOUT_MS}). On timeout the answer is "unknown" with truncated:true and timed_out:true, and the search is stopped. The engine's own bound (max_states) still applies.`);

const WORKER_SRC = `
const { parentPort, workerData } = require('node:worker_threads');
(async () => {
  const m = await import(workerData.engine);
  await m.default();
  const isTrap = (e) => typeof WebAssembly !== 'undefined' && e instanceof WebAssembly.RuntimeError;
  parentPort.on('message', (j) => {
    try {
      let value;
      if (j.op === 'hosted') value = m.hosted(j.name, j.args);
      else if (j.op === 'reach') { const n = new m.Net(j.model); try { value = n.reachability(j.options); } finally { n.free?.(); } }
      else throw new Error('unknown job ' + j.op);
      parentPort.postMessage({ id: j.id, value });
    } catch (e) {
      parentPort.postMessage({ id: j.id, error: String(e?.message ?? e), trap: isTrap(e) });
    }
  });
  parentPort.postMessage({ ready: true });
})().catch((e) => parentPort.postMessage({ fatal: String(e?.message ?? e) }));
`;

let guardWorker = null;
let guardChain = Promise.resolve();
let jobId = 0;

function spawnWorker() {
  const w = new Worker(WORKER_SRC, { eval: true, workerData: { engine: new URL('./pflow-wasm-hosted.inline.js', import.meta.url).href } });
  w.unref();
  const slot = { w, pending: null, ready: null, dead: false };
  slot.ready = new Promise((resolve, reject) => {
    const die = (why) => {
      slot.dead = true;
      if (guardWorker === slot) guardWorker = null;
      const p = slot.pending; slot.pending = null;
      if (p) { clearTimeout(p.timer); p.reject(new Error(why)); }
      reject(new Error(why));
    };
    w.on('message', (m) => {
      if (m.ready) return resolve();
      if (m.fatal) return die(`engine worker failed to start: ${m.fatal}`);
      const p = slot.pending;
      if (!p || p.id !== m.id) return;
      slot.pending = null; clearTimeout(p.timer);
      if (m.trap) { slot.dead = true; if (guardWorker === slot) guardWorker = null; w.terminate(); }
      if ('error' in m) p.reject(new Error(m.error)); else p.resolve({ value: m.value });
    });
    w.on('error', (e) => die(`engine worker crashed: ${e?.message ?? e}`));
    w.on('exit', (c) => { if (!slot.dead) die(`engine worker exited (${c})`); });
  });
  slot.ready.catch(() => {});
  return slot;
}

/** Run one engine job ({op:'hosted', name, args} or {op:'reach', model, options})
 *  in the worker. Resolves {value}, or {timedOut:true} when it ran past timeoutMs. */
export function runGuarded(job, timeoutMs = DEFAULT_TIMEOUT_MS) {
  const run = async () => {
    if (!guardWorker || guardWorker.dead) guardWorker = spawnWorker();
    const slot = guardWorker;
    await slot.ready; // starting the engine is not charged to the call
    return new Promise((resolve, reject) => {
      const id = ++jobId;
      const timer = setTimeout(() => {
        slot.pending = null; slot.dead = true;
        if (guardWorker === slot) guardWorker = null;
        slot.w.terminate();
        resolve({ timedOut: true });
      }, timeoutMs);
      slot.pending = { id, resolve, reject, timer };
      slot.w.postMessage({ ...job, id });
    });
  };
  const p = guardChain.then(run, run);
  guardChain = p.catch(() => {});
  return p;
}

const why = (ms) => `timed out after ${ms} ms (timeout_ms); the search was stopped, so nothing is claimed either way`;

/** reachability with a time limit: the engine's answer, or an unknown one. */
export async function reachabilityGuarded(model, options, timeoutMs = DEFAULT_TIMEOUT_MS) {
  const r = await runGuarded({ op: 'reach', model, options }, timeoutMs);
  if (!r.timedOut) return r.value;
  const u = `unknown (${why(timeoutMs)})`;
  return {
    complete: false, truncated: true, truncated_reason: why(timeoutMs), timed_out: true, timeout_ms: timeoutMs,
    bounded: u, deadlock_free: 'unknown', live: 'unknown', quasi_live: 'unknown',
    note: 'timed out: no verdict is true or false; raise timeout_ms or lower max_states',
  };
}

/** A hosted tool's answer when it ran out of time. */
function hostedTimeout(name, args, ms) {
  const reason = why(ms);
  if (name === 'verify') {
    const props = Array.isArray(args.properties) ? args.properties : [];
    return { ok: false, proved: 0, refuted: 0, unknown: props.length, state_count: 0, truncated: true, truncated_reason: reason, timed_out: true, timeout_ms: ms,
      verdicts: props.map((property) => ({ property, status: 'unknown', method: '', detail: reason })) };
  }
  return { valid: 'unknown', truncated: true, truncated_reason: reason, timed_out: true, timeout_ms: ms, info: [], note: 'timed out: validity is unknown' };
}

/** The current marking written into a model (array shape or pflow.xyz JSON-LD),
 *  so a worker can start from it. null when the model has neither shape. */
export function withMarking(model, marking) {
  const m = typeof model === 'string' ? JSON.parse(model) : model;
  if (m && Array.isArray(m.places)) return { ...m, places: m.places.map((p) => ({ ...p, initial: marking[p.id] ?? 0 })) };
  if (m && m.places && typeof m.places === 'object') {
    return { ...m, places: Object.fromEntries(Object.entries(m.places).map(([id, p]) => [id, { ...p, initial: [marking[id] ?? 0] }])) };
  }
  return null;
}

const GUARDED = new Set(['validate', 'verify']);

export function register(tool, { hosted, jsonld, last }) {
  for (const t of tools) {
    const props = t.inputSchema?.properties ?? {};
    const required = new Set(t.inputSchema?.required ?? []);
    const shape = {};
    for (const [k, p] of Object.entries(props)) {
      if (k === 'net') { shape.net = netArg; continue; }
      shape[k] = required.has(k) ? zodOf(p) : zodOf(p).optional();
    }
    let description = t.description + LOCAL_NOTE;
    if (GUARDED.has(t.name)) {
      shape.timeout_ms = timeoutArg;
      description += ` Local addition: timeout_ms (default ${DEFAULT_TIMEOUT_MS}, max ${MAX_TIMEOUT_MS}); a call that runs past it answers unknown with truncated:true and timed_out:true instead of blocking.`;
    }
    if (t.name === 'edit') {
      shape.cid = z.boolean().optional().describe('false skips sealing the result (its CID), which is about cubic in net size: 6 s at 100 places. Default true');
      description += ' Local addition: cid:false leaves the CID out of the answer.';
    }
    tool(t.name, description, shape, async (args) => {
      const { timeout_ms, ...a } = args;
      let entry = null;
      if ('net' in props && (t.name !== 'edit' || a.net != null)) {
        const key = typeof a.net === 'string' ? a.net : a.net == null ? last() : null;
        a.net = resolveNet(a.net, { jsonld, last });
        entry = key != null ? jsonld.get(key) : null;
      }
      // a loaded net's CID is computed once
      if (t.name === 'cid' && entry?.cid) return { cid: entry.cid };
      let out;
      if (GUARDED.has(t.name)) {
        const ms = timeout_ms ?? DEFAULT_TIMEOUT_MS;
        const r = await runGuarded({ op: 'hosted', name: t.name, args: a }, ms);
        out = r.timedOut ? hostedTimeout(t.name, a, ms) : r.value;
      } else out = hosted(t.name, a);
      if (t.name === 'cid' && entry && typeof out?.cid === 'string' && out.kind == null) entry.cid = out.cid;
      return out;
    });
  }
}
