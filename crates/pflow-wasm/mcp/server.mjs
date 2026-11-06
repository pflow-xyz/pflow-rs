#!/usr/bin/env node
// pflow-mcp: the pflow-rs Petri net engine (compiled to WebAssembly) as a local
// MCP server over stdio. No native build, no network after install.
//
//   npx @pflow-xyz/pflow-mcp
//
// Nets live in memory for the life of the process, keyed by name.
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import { readFileSync, writeFileSync } from 'node:fs';
import { buildPage, readMeta } from './pages.mjs';
import { register as registerHosted, reachabilityGuarded, timeoutArg, withMarking } from './hosted.mjs';
import { viewIds } from './views/index.mjs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
// The engine with the pflow.xyz/mcp tools built in (make wasm-mcp-engine). The
// browser build, ./pflow-wasm.inline.js, is only read as text by pages.mjs.
import init, { Net, hosted, version as engineVersion } from './pflow-wasm-hosted.inline.js';

await init();

const nets = new Map();
const models = new Map(); // name -> model object as loaded, for artifact_html
// name -> what the pflow.xyz-named tools read: { doc } (the JSON-LD form of the
// loaded net, sealed with its @id when the CID was computed) or { unsupported }
// (why it cannot be written as one), plus cid and layout.
const jsonld = new Map();
let last = null;

// The package has its own version (build.mjs); the engine's is version().
const pkgVersion = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')).version;
const meta = readMeta(engineVersion());
// Read by the client at connect time and given to the model. It steers Claude to
// the engine's own pages instead of a hand-written one, whose verdicts would be
// the model's judgment and not the engine's.
const INSTRUCTIONS = `pflow-mcp: a Petri net engine (WebAssembly, offline). Nets are pflow.xyz JSON-LD.

To show a net, a simulation or a proof, build the page with a tool and publish what it returns as a Claude artifact, with your own Artifact tool. This server only writes a local HTML file: it does not host, serve or publish anything, and it makes no network requests. Do not hand-write the page.
- One net: load_net (or edit to build one), then artifact_html. It writes a self-contained page (engine embedded, no network) and returns a file path. Pass that file's path to the Artifact tool and publish it unchanged. The HTML is not returned inline; do not ask for it with inline=true unless you must. artifact_html takes views (default ["play"]); pick them from what the person asked for:
  | the person says | views |
  | show, play, how does it behave | play |
  | is it safe, deadlock, prove, always holds | analysis + proof |
  | what if, compare, remove, double | whatif |
  | snap together, compose | compose_canvas (a composition, not a net view) |
  | share, send | every view that fits the question |
  Views not yet available are refused with the list of known ones: use those and say what is missing. Every page carries the net as JSON-LD (#pflow-net, Download net, Copy as text), so whoever gets the artifact can read the net back out of it and load_net it.
- A composition (PetriNetBundle: parts joined by typed links): bundle_example, bundle_validate, bundle_candidates and bundle_add_link to build it, then compose_canvas for the snap-together page. The page re-checks proofs itself, with no engine.

Verdicts come from the engine. Report a result of reachability, verify, validate, invariants, bundle_prove or bundle_check_proof only as the tool returned it, with its evidence or witness trace. A verdict of "unknown" stays "unknown": say the search was bounded, and never turn it into true. If you state a conclusion the engine did not return, call it your own judgment.

Nets with guards, stages or rate schedules are refused, not run as a different net. Nets live in memory for the life of the process.`;

const server = new McpServer({ name: 'pflow-mcp', version: pkgVersion }, { instructions: INSTRUCTIONS });

const ok = (value) => ({ content: [{ type: 'text', text: JSON.stringify(value, null, 2) }] });
const fail = (e) => ({ isError: true, content: [{ type: 'text', text: String(e?.message ?? e) }] });
const get = (name) => {
  const key = name ?? last;
  if (key == null) throw new Error('no net loaded: call load_net first');
  const net = nets.get(key);
  if (!net) throw new Error(`no net named "${key}"; loaded: ${[...nets.keys()].join(', ') || 'none'}`);
  return net;
};
const tool = (name, description, shape, fn) =>
  server.registerTool(name, { description, inputSchema: shape }, async (args) => {
    try { return ok(await fn(args)); } catch (e) { return fail(e); }
  });

const netName = z.string().optional().describe('Net name; defaults to the most recently loaded');
const rates = z.record(z.string(), z.number().nullable()).optional()
  .describe('Per-transition rate overrides. 0 switches the transition off (it never fires); null means unset (the net\'s own rate, else 1)');

const snapshot = (net) => ({
  places: net.places(), transitions: net.transitions(),
  marking: net.marking(), enabled: net.enabled(),
});

// Shared by load_net and load_example (examples.mjs): one path, so an example is
// a net like any other.
function loadNet({ model, name, cid }) {
  const obj = typeof model === 'string' ? JSON.parse(model) : model;
  const key = name ?? obj?.name ?? 'net';
  const net = new Net(model);
  let conv;
  try {
    conv = hosted('net_to_jsonld', cid === undefined ? { model: obj, name: key } : { model: obj, name: key, cid });
  } catch (e) { net.free?.(); throw e; }
  nets.get(key)?.free?.();
  nets.set(key, net);
  models.set(key, obj);
  jsonld.set(key, { doc: conv.net, cid: conv.cid, unsupported: conv.unsupported });
  last = key;
  const { net: _doc, places: _n, ...about } = conv;
  return { name: key, ...snapshot(net), ...about };
}

tool('load_net',
  'Load a Petri net: pflow.xyz JSON-LD, or the array shape (places: [{id, initial}], arcs: [{from, to, weight, type}]). Replaces any net with the same name. Every other tool then reads it by name, whichever shape it came in. Also answers shape, layout ("given", or "synthesized" when the net had no coordinates), and the net\'s CID: the CID names the document, layout and arc order included. It is computed here for up to 50 places and 250 places+transitions+arcs in all (cost follows the whole graph); a larger net answers cid null with cid_skipped, and the cid tool computes it on demand. A net that cannot be written as a pflow.xyz document (data places, constraints, timed transitions) answers unsupported and still runs locally. Nets with guards, stages or rate schedules, and arcs with a fractional weight, are refused.',
  { model: z.union([z.string(), z.record(z.string(), z.any())]).describe('The net as a JSON object or JSON text'),
    name: z.string().optional().describe('Name to keep it under; defaults to the model name, else "net"') },
  (args) => loadNet(args));

tool('list_nets', 'List loaded nets.', {},
  () => ({ current: last, nets: [...nets.keys()] }));

tool('state',
  'Places, transitions, current marking and enabled transitions. For any loaded net `info` adds the hosted pflow_info summary: name, colours, each place (initial, capacity, label), transition (rate, label) and arc; its marking and enabled are the initial ones.',
  { net: netName }, ({ net }) => {
    const out = snapshot(get(net));
    const entry = jsonld.get(net ?? last);
    if (entry?.doc) out.info = hosted('info', { net: entry.doc });
    else if (entry?.unsupported) out.info_unavailable = entry.unsupported;
    return out;
  });

tool('fire', 'Fire one enabled transition and return the new marking. Errors say why it is not enabled.',
  { transition: z.string(), net: netName },
  ({ transition, net }) => {
    const n = get(net);
    const marking = n.fire(transition);
    return { marking, enabled: n.enabled() };
  });

tool('reset', 'Return a net to its initial marking.', { net: netName },
  ({ net }) => { const n = get(net); n.reset(); return snapshot(n); });

tool('simulate',
  'Deterministic mass-action ODE (Tsit5) from the current marking. Does not move the marking.',
  { net: netName,
    tEnd: z.number().optional().describe('End time (default 100)'),
    maxPoints: z.number().int().optional().describe('Max samples (default 200)'),
    rates,
    contextual: z.enum(['refuse', 'selfloop', 'drop']).optional()
      .describe('How read and inhibitor arcs are treated (default refuse)') },
  ({ net, ...options }) => get(net).simulate(options));

tool('stochastic',
  'Gillespie SSA from the current marking; byte-exact with go-pflow at the same seed. Does not move the marking.',
  { net: netName,
    horizon: z.number().optional().describe('End time (default 10)'),
    samples: z.number().int().optional().describe('Time samples (default 101)'),
    realizations: z.number().int().optional().describe('Ensemble size (default 1)'),
    seed: z.number().int().optional().describe('RNG seed (default 1)'),
    rates },
  ({ net, ...options }) => get(net).stochastic(options));

tool('reachability',
  'Explore the reachable markings (pflow-rs reachability, untimed: rates play no part), with parity to pflow.xyz/mcp pflow_reachability: state and edge count, bounded or not, dead markings with the first, transitions that never fire, and three verdicts, each true, false or "unknown": deadlock_free (no reachable marking has nothing enabled, an empty one included), live (L4: from every reachable marking, every transition can still fire) and quasi_live (L1: every transition fires in some reachable marking, which a net with a deadlock can pass). witness is a firing sequence that replays from `from` to the dead marking, or into a closed region shutting transitions out, with never_fires naming them. When complete is false the search stopped at its bound (truncated says why): no verdict is true, and only a witness makes one false. Pass a loaded net by name, or a model inline. Does not move the marking.',
  { net: netName,
    model: z.union([z.string(), z.record(z.string(), z.any())]).optional()
      .describe('A net to analyze without loading it (pflow.xyz JSON object or text); overrides net'),
    max_states: z.number().int().min(1).max(20000).optional().describe('State budget (default 2000, ≤ 20000)'),
    max_tokens: z.number().int().min(1).optional()
      .describe('Tokens in one place past which the net is called unbounded (default 1000)'),
    from: z.enum(['initial', 'current']).optional()
      .describe('Start from the initial marking (default, as the hosted tool) or a loaded net\'s current one'),
    timeout_ms: timeoutArg },
  async ({ net, model, max_states, max_tokens, from, timeout_ms }) => {
    // Runs in a worker thread (hosted.mjs): a search past timeout_ms is stopped and answers unknown.
    const options = { maxStates: max_states, maxTokens: max_tokens, from };
    if (model != null) {
      if (from === 'current') throw new Error('from "current" needs a loaded net, not an inline model');
      return reachabilityGuarded(model, options, timeout_ms);
    }
    const loaded = get(net);
    let m = models.get(net ?? last);
    if (from === 'current') {
      // the worker has no marking of its own: hand it the current one as the initial
      m = withMarking(m, loaded.marking());
      if (!m) throw new Error('from "current": this net\'s shape cannot carry its marking to the search');
      options.from = 'initial';
    }
    return reachabilityGuarded(m, options, timeout_ms);
  });

// ---- artifact_html -------------------------------------------------------
// The page shell and the views live in pages.mjs and views/.

tool('artifact_html',
  `Write a self-contained interactive HTML page for a loaded net and return its file path; publish that file as an artifact (the HTML is not returned inline: it is ~250 KB). The pflow WebAssembly engine is embedded, so the page makes no network requests and runs inside a sandboxed artifact. Every page has a footer (document CID, package, engine and ontology versions, seed), the net as JSON-LD (a Download net button, a Copy as text button, and a #pflow-net block a later session can read and load_net), and a link to the same net in the public editor. views picks what the page shows (known: ${viewIds().join(', ')}); default ["play"] is the click-to-fire token game plus ODE/SSA charts.`,
  { net: netName,
    views: z.array(z.string()).optional().describe(`Views to include, in order (known: ${viewIds().join(', ')}); default ["play"]. An unknown id is refused.`),
    title: z.string().optional().describe('Page title (default: the net name)'),
    path: z.string().optional().describe('Where to write it (default: a temp file)'),
    inline: z.boolean().optional().describe('Also return the HTML text. Large; off by default.') },
  ({ net, views, title, path, inline }) => {
    const key = net ?? last;
    get(net);
    const model = models.get(key);
    const entry = jsonld.get(key) ?? {};
    const html = buildPage({
      net: { key, title: title ?? model?.name ?? key, model, doc: entry.doc, cid: entry.cid ?? null,
        cidNote: entry.cid ? 'The CID names the document: layout, arc order and name included.' : undefined },
      viewIds: views, meta, engine: { hosted, net: (n) => get(n ?? key) },
    });
    const file = path ?? join(tmpdir(), `pflow-${String(key).replace(/[^\w.-]+/g, '_')}.html`);
    writeFileSync(file, html);
    return { path: file, bytes: html.length, views: (views ?? ['play']), ...(inline ? { html } : {}) };
  });

import { register as registerOntology } from './ontology.mjs';
import { register as registerChyp } from './chyp.mjs';
import { register as registerCompose } from './compose.mjs';
import { register as registerExamples } from './examples.mjs';
registerOntology(tool);
registerChyp(tool);
registerHosted(tool, { hosted, jsonld, last: () => last });
registerCompose(tool, { meta });
registerExamples(tool, { loadNet, hosted });

await server.connect(new StdioServerTransport());
