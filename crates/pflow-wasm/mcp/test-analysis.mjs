// The analysis view: a generated page renders its sections, and every number on
// it is the one the MCP tool gives for the same net (invariants, incidence,
// reachability), including a witness replayed step by step in the page and an
// unbounded net whose verdicts stay "unknown".
//   node crates/pflow-wasm/mcp/test-analysis.mjs <built pflow-mcp dir>
import assert from 'node:assert/strict';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';

const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const need = createRequire(join(dir, 'package.json'));
const { Client } = await import(need.resolve('@modelcontextprotocol/sdk/client/index.js'));
const { StdioClientTransport } = await import(need.resolve('@modelcontextprotocol/sdk/client/stdio.js'));
const client = new Client({ name: 'test-analysis', version: '0' });
await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')] }));
const call = async (name, args = {}) => {
  const r = await client.callTool({ name, arguments: args });
  const text = r.content[0].text;
  return { error: !!r.isError, text, json: r.isError ? null : JSON.parse(text) };
};
const ok = (m) => console.log('ok', m);

const nets = {
  // beans -> brew -> cups: bounded, ends in a deadlock
  coffee: { name: 'coffee', places: [{ id: 'beans', initial: 3 }, { id: 'cups' }], transitions: [{ id: 'brew' }],
    arcs: [{ from: 'beans', to: 'brew' }, { from: 'brew', to: 'cups' }] },
  // p -> t -> q -> u -> p: live, bounded, T- and P-invariants
  ring: { name: 'ring', places: [{ id: 'p', initial: 1 }, { id: 'q' }], transitions: [{ id: 't' }, { id: 'u' }],
    arcs: [{ from: 'p', to: 't' }, { from: 't', to: 'q' }, { from: 'q', to: 'u' }, { from: 'u', to: 'p' }] },
  // t makes a token from nothing: unbounded, the search stops at its budget
  grow: { name: 'grow', places: [{ id: 'p' }], transitions: [{ id: 't' }], arcs: [{ from: 't', to: 'p' }] },
  // a ring that can also step into a sink: L1 but not L4 (a livelock-style region)
  leaky: { name: 'leaky', places: [{ id: 'a', initial: 1 }, { id: 'b' }, { id: 'c' }], transitions: [{ id: 'go' }, { id: 'back' }, { id: 'leak' }],
    arcs: [{ from: 'a', to: 'go' }, { from: 'go', to: 'b' }, { from: 'b', to: 'back' }, { from: 'back', to: 'a' }, { from: 'b', to: 'leak' }, { from: 'leak', to: 'c' }] },
};
const files = {};
for (const [k, model] of Object.entries(nets)) {
  assert.ok(!(await call('load_net', { name: k, model })).error);
  files[k] = join(dir, `analysis-test-${k}.html`);
  const r = await call('artifact_html', { net: k, views: ['analysis'], path: files[k] });
  assert.ok(!r.error, r.text);
  assert.deepEqual(r.json.views, ['analysis']);
}

// static: the sections are there, the module parses, nothing reaches the network
for (const k of Object.keys(nets)) {
  const html = readFileSync(files[k], 'utf8');
  for (const id of ['an-structure', 'an-incidence', 'an-reach', 'an-verdicts']) assert.ok(html.includes(`id="${id}"`), `${k}: ${id}`);
  const script = html.split('<script type="module">')[1].split('</script>')[0];
  const mjs = join(dir, 'analysis-check.mjs');
  writeFileSync(mjs, script);
  const chk = spawnSync(process.execPath, ['--check', mjs], { encoding: 'utf8' });
  rmSync(mjs);
  assert.equal(chk.status, 0, chk.stderr);
  assert.ok(!/<iframe|<script[^>]+src=|<link[^>]+href=|fetch\(|XMLHttpRequest/.test(html.replace(script, '')), `${k}: no external fetch`);
}
ok('analysis page: sections present, module parses, no network');

// the view composes with play, in either order, and the ids do not collide
let r = await call('artifact_html', { net: 'ring', views: ['play', 'analysis'], path: files.ring });
assert.deepEqual(r.json.views, ['play', 'analysis']);
{
  const ids = [...readFileSync(files.ring, 'utf8').split('<script type="module">')[0].matchAll(/ id="([^"]+)"/g)].map((m) => m[1]);
  const dup = ids.filter((x, i) => ids.indexOf(x) !== i);
  assert.deepEqual(dup, [], 'no duplicate element ids when combined with play');
}
await call('artifact_html', { net: 'ring', views: ['analysis'], path: files.ring });
ok('analysis composes with play');

// the engine's answers for each net, through the MCP tools
const tool = {};
for (const k of Object.keys(nets)) {
  tool[k] = {
    inv: (await call('invariants', { net: k })).json,
    inc: (await call('incidence', { net: k })).json,
    reach: (await call('reachability', { net: k })).json,
    reachSmall: (await call('reachability', { net: k, max_states: 5 })).json,
  };
}
assert.equal(tool.grow.reach.complete, false, 'grow is the unbounded fixture');
assert.equal(tool.grow.reach.live, 'unknown');
assert.equal(tool.leaky.reach.live, false);
assert.equal(tool.leaky.reach.quasi_live, true);
assert.equal(tool.coffee.reach.deadlock_free, false);
assert.equal(tool.ring.reach.live, true);

const core = process.env.PLAYWRIGHT_CORE ?? '/home/myork/Workspace/tty-vevr-se/node_modules/playwright-core';
const chrome = process.env.CHROME ?? '/usr/bin/google-chrome';
if (!existsSync(core) || !existsSync(chrome)) console.log(`SKIP browser: need playwright-core (${core}) and a browser (${chrome})`);
else {
  const { chromium } = createRequire(import.meta.url)(core);
  const CSP = "default-src 'none'; script-src 'unsafe-inline' 'wasm-unsafe-eval'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'";
  const browser = await chromium.launch({ executablePath: chrome, args: ['--no-sandbox'] });
  const open = async (k, scheme = 'light') => {
    const page = await browser.newPage({ viewport: { width: 375, height: 800 }, colorScheme: scheme });
    const problems = [], requests = [];
    page.on('console', (m) => { if (m.type() === 'error') problems.push(m.text()); });
    page.on('pageerror', (e) => problems.push(String(e)));
    page.on('request', (q) => { if (!q.url().startsWith('http://page.test/') && !q.url().startsWith('data:')) requests.push(q.url()); });
    const body = readFileSync(files[k], 'utf8');
    await page.route('http://page.test/p.html', (q) => q.fulfill({ status: 200, contentType: 'text/html', headers: { 'content-security-policy': CSP }, body }));
    await page.goto('http://page.test/p.html');
    await page.waitForSelector('#an-verdicts [data-k]');
    return { page, problems, requests };
  };
  const verdicts = (page) => page.evaluate(() => Object.fromEntries([...document.querySelectorAll('#an-verdicts [data-k], #an-counts [data-k]')].map((e) => [e.dataset.k, e.dataset.value])));
  try {
    for (const k of Object.keys(nets)) {
      const { page, problems, requests } = await open(k, k === 'coffee' ? 'dark' : 'light');
      const t = tool[k], v = await verdicts(page);
      // reachability: verdicts, counts and bounds are the tool's
      assert.equal(v['deadlock free'], String(t.reach.deadlock_free), `${k} deadlock_free`);
      assert.equal(v['quasi-live (L1)'], String(t.reach.quasi_live), `${k} quasi_live`);
      assert.equal(v['live (L4)'], String(t.reach.live), `${k} live`);
      assert.equal(v.states, String(t.reach.states), `${k} states`);
      assert.equal(v.edges, String(t.reach.edges), `${k} edges`);
      assert.equal(v['search complete'], String(t.reach.complete), `${k} complete`);
      assert.equal(v.bounded, typeof t.reach.bounded === 'string' ? 'unknown' : String(t.reach.bounded), `${k} bounded`);
      // invariants and conservation
      const eq = (list) => list.map((x) => x.equation);
      const shown = await page.evaluate(() => [...document.querySelectorAll('#an-p-list li')].map((e) => e.textContent));
      assert.deepEqual(shown, eq(t.inv.p_invariants), `${k} P-invariants`);
      const tshown = await page.evaluate(() => [...document.querySelectorAll('#an-t-list li')].map((e) => e.textContent));
      assert.deepEqual(tshown, t.inv.t_invariants.map((x) => x.firing_multiset), `${k} T-invariants`);
      const inv = await page.evaluate(() => Object.fromEntries([...document.querySelectorAll('#an-inv [data-k]')].map((e) => [e.dataset.k, e.dataset.value])));
      assert.equal(inv.conservative, String(t.inv.conservative));
      assert.equal(inv['structurally bounded'], String(t.inv.structurally_bounded));
      // incidence
      const cells = await page.evaluate(() => [...document.querySelectorAll('#an-matrix tbody tr')].map((tr) => [...tr.querySelectorAll('td')].map((td) => +td.textContent)));
      assert.deepEqual(cells, t.inc.matrix, `${k} incidence`);
      const heads = await page.evaluate(() => [...document.querySelectorAll('#an-matrix thead th')].slice(1).map((e) => e.textContent));
      assert.deepEqual(heads, t.inc.transitions);
      // witness: the trace is the tool's, and replaying it in the page ends where the engine said
      const trace = await page.evaluate(() => [...document.querySelectorAll('#an-tr span')].map((e) => e.textContent));
      if (t.reach.witness) {
        assert.deepEqual(trace, t.reach.witness.trace, `${k} witness trace`);
        for (let i = 0; i < trace.length; i++) await page.click('#an-step');
        assert.equal(await page.getAttribute('#an-replay-state', 'data-match'), 'true', `${k} replay ends at the engine's marking`);
        const marks = await page.evaluate(() => Object.fromEntries([...document.querySelectorAll('#an-replay-marks div')].map((d) => [d.querySelector('span').textContent, +d.querySelector('b').textContent])));
        for (const [p, n] of Object.entries(t.reach.witness.marking)) assert.equal(marks[p], n, `${k} replayed marking of ${p}`);
        assert.equal(await page.isDisabled('#an-step'), true);
        await page.click('#an-rewind');
        assert.equal(await page.locator('#an-tr span.done').count(), 0);
      } else assert.deepEqual(trace, [], `${k}: no witness, none shown`);
      // phone width, no requests, no errors
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `${k}: no horizontal scroll at 375px`);
      assert.deepEqual(requests, [], `${k}: no network requests`);
      assert.deepEqual(problems, [], `${k}: no console errors`);
      await page.close();
      ok(`${k}: page values match the tools (verdicts, counts, invariants, incidence, witness replay)`);
    }
    // unknown stays unknown, and says the search was bounded; a smaller bound changes the answer as the tool's does
    {
      const { page } = await open('grow');
      const v = await verdicts(page);
      assert.equal(v.live, undefined);
      assert.equal(v['live (L4)'], 'unknown');
      assert.equal(v['deadlock free'], 'unknown');
      assert.match(await page.innerText('#an-reach-note'), /search was bounded/);
      assert.match(await page.innerText('#an-verdicts [data-value=unknown] b'), /unknown/);
      await page.close();
    }
    {
      const { page } = await open('ring');
      await page.fill('#an-max', '1');
      await page.click('#an-run');
      const v = await verdicts(page), t = tool.ring;
      const small = (await call('reachability', { net: 'ring', max_states: 1 })).json;
      assert.equal(v['live (L4)'], String(small.live));
      assert.equal(v.states, String(small.states));
      assert.equal(v['search complete'], String(small.complete));
      if (!small.complete) assert.match(await page.innerText('#an-reach-note'), /search was bounded/);
      assert.ok(t);
      await page.close();
    }
    ok('unknown verdicts stay unknown and say the search was bounded; re-running with another bound matches the tool');
  } finally { await browser.close(); }
}
for (const f of Object.values(files)) rmSync(f, { force: true });
await client.close();
