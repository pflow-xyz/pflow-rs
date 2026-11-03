// The pflow.xyz/mcp tools, answered locally by pflow-hosted (crates/pflow-hosted)
// through the engine's `hosted` build. hosted-tools.json holds each tool's name
// (the hosted name without `pflow_`), description and input schema as the hosted
// server lists them, so a call written for pflow.xyz/mcp works here unchanged.
//
// One addition: wherever the hosted tool takes `net` (a pflow.xyz JSON-LD object),
// this server also takes the name of a net loaded with load_net, and with no
// `net` at all uses the most recently loaded one (edit excepted: there no net
// means an empty one, as hosted).
import { readFileSync } from 'node:fs';
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

/** The model a `net` argument names: an object as is, a string a loaded net's. */
export function resolveNet(value, { models, last }) {
  if (value == null) {
    if (last() == null) throw new Error('net: give a pflow.xyz net, or load one with load_net first');
    return models.get(last());
  }
  if (typeof value === 'string') {
    if (!models.has(value)) throw new Error(`no net named "${value}"; loaded: ${[...models.keys()].join(', ') || 'none'}`);
    return models.get(value);
  }
  return value;
}

export function register(tool, { hosted, models, last }) {
  for (const t of tools) {
    const props = t.inputSchema?.properties ?? {};
    const required = new Set(t.inputSchema?.required ?? []);
    const shape = {};
    for (const [k, p] of Object.entries(props)) {
      if (k === 'net') { shape.net = netArg; continue; }
      shape[k] = required.has(k) ? zodOf(p) : zodOf(p).optional();
    }
    tool(t.name, t.description + LOCAL_NOTE, shape, (args) => {
      const a = { ...args };
      if ('net' in props && (t.name !== 'edit' || a.net != null)) a.net = resolveNet(a.net, { models, last });
      return hosted(t.name, a);
    });
  }
}
