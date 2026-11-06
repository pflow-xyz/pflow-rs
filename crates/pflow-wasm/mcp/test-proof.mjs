// The proof view: a generated page renders its section, every verdict on it is
// the one the `verify` MCP tool gives for the same net and budget, counterexample
// traces replay on the page's embedded engine, unknown stays unknown and says the
// search was bounded, Copy proof as text writes the record, and for a flattened
// composition the engine-free re-checker accepts the real record and rejects a
// tampered one.
//   node crates/pflow-wasm/mcp/test-proof.mjs <built pflow-mcp dir>
import assert from 'node:assert/strict';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';

const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const need = createRequire(join(dir, 'package.json'));
const { Client } = await import(need.resolve('@modelcontextprotocol/sdk/client/index.js'));
const { StdioClientTransport } = await import(need.resolve('@modelcontextprotocol/sdk/client/stdio.js'));
const client = new Client({ name: 'test-proof', version: '0' });
await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')] }));
const call = async (name, args = {}) => {
  const r = await client.callTool({ name, arguments: args });
  const text = r.content[0].text;
  return { error: !!r.isError, text, json: r.isError ? null : JSON.parse(text) };
};
const ok = (m) => console.log('ok', m);
const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
const PROPS = ['deadlock-free', 'bounded', 'live', 'l4-live', 'terminating', 'conserves'].map((kind) => ({ kind }));
const MAX = 5000;

// the packaged re-checker is the compose canvas's, byte for byte (when the repo is there to compare)
const repoVerify = new URL('../compose/verify.mjs', import.meta.url);
if (existsSync(repoVerify)) assert.equal(readFileSync(join(dir, 'views', 'verify-record.mjs'), 'utf8'), readFileSync(repoVerify, 'utf8'), 'views/verify-record.mjs drifted from compose/verify.mjs');
ok('verify-record.mjs is compose/verify.mjs');

const nets = {
  // two ways to finish: a deadlock, with a trace
  dead: { name: 'dead', places: [{ id: 'a', initial: 1 }, { id: 'b', initial: 1 }, { id: 'c' }], transitions: [{ id: 't1' }, { id: 't2' }],
    arcs: [{ from: 'a', to: 't1' }, { from: 'b', to: 't2' }, { from: 't1', to: 'c' }, { from: 't2', to: 'c' }] },
  // a cycle: proved deadlock-free, live; not terminating
  ring: { name: 'ring', places: [{ id: 'p', initial: 1 }, { id: 'q' }], transitions: [{ id: 't' }, { id: 'u' }],
    arcs: [{ from: 'p', to: 't' }, { from: 't', to: 'q' }, { from: 'q', to: 'u' }, { from: 'u', to: 'p' }] },
  // a source: unbounded, the search stops at its budget
  grow: { name: 'grow', places: [{ id: 'p' }], transitions: [{ id: 't' }], arcs: [{ from: 't', to: 'p' }] },
  // stops on the token-count limit, far below the state budget: the page must give the engine's reason
  tok: { name: 'tok', places: [{ id: 'a', initial: 1 }], transitions: [{ id: 't' }], arcs: [{ from: 'a', to: 't' }, { from: 't', to: 'a', weight: 2 }] },
};
const files = {}, tool = {}, cids = {};
for (const [k, model] of Object.entries(nets)) {
  cids[k] = (await call('load_net', { name: k, model })).json.cid;
  files[k] = join(dir, `proof-test-${k}.html`);
  const r = await call('artifact_html', { net: k, views: ['proof'], path: files[k] });
  assert.ok(!r.error, r.text);
  assert.deepEqual(r.json.views, ['proof']);
  tool[k] = (await call('verify', { net: k, properties: PROPS, max_states: MAX })).json;
}
assert.ok(tool.dead.verdicts.some((v) => v.status === 'refuted' && v.counterexample?.trace?.length), 'dead has a refuted verdict with a trace');
assert.ok(tool.grow.verdicts.some((v) => v.status === 'unknown'), 'grow has an unknown verdict');
assert.ok(tool.ring.verdicts.some((v) => v.status === 'proved'));
assert.match(tool.tok.truncated_reason ?? '', /token/, 'the engine says the token limit stopped the search');

// static: the section, the module parses, nothing reaches the network, composes with play
for (const k of Object.keys(nets)) {
  const html = readFileSync(files[k], 'utf8');
  assert.ok(html.includes('id="pv-proof"') && html.includes('id="pv-copy"') && html.includes('Copy proof as text'), `${k}: section`);
  assert.ok(!html.includes('id="pv-recheck"'), `${k}: not a composition, no re-check section`);
  const script = html.split('<script type="module">')[1].split('</script>')[0];
  const mjs = join(dir, 'proof-check.mjs');
  writeFileSync(mjs, script);
  const chk = spawnSync(process.execPath, ['--check', mjs], { encoding: 'utf8' });
  rmSync(mjs);
  assert.equal(chk.status, 0, chk.stderr);
  assert.ok(!/<iframe|<script[^>]+src=|<link[^>]+href=|fetch\(|XMLHttpRequest/.test(html.replace(script, '')), `${k}: no external fetch`);
}
{
  const r = await call('artifact_html', { net: 'ring', views: ['play', 'proof'], path: files.ring });
  assert.deepEqual(r.json.views, ['play', 'proof']);
  const ids = [...readFileSync(files.ring, 'utf8').split('<script type="module">')[0].matchAll(/ id="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(ids.filter((x, i) => ids.indexOf(x) !== i), [], 'no duplicate element ids next to play');
  await call('artifact_html', { net: 'ring', views: ['proof'], path: files.ring });
}
ok('proof page: section present, module parses, no network, composes with play');

// the composition: the flat net of the default bundle, its real record, and a tampered one
const ex = (await call('bundle_example')).json.bundle;
const flat = (await call('bundle_net', { bundle: ex })).json;
const prove = (await call('bundle_prove', { bundle: ex })).json;
assert.equal((await call('bundle_check_proof', { bundle: ex, proof: prove })).json.valid, true);
await call('load_net', { name: 'comp', model: flat.net });
files.comp = join(dir, 'proof-test-comp.html');
assert.ok(!(await call('artifact_html', { net: 'comp', views: ['proof'], path: files.comp })).error);
tool.comp = (await call('verify', { net: 'comp', properties: PROPS, max_states: MAX })).json;
cids.comp = (await call('cid', { net: 'comp' })).json.cid;
assert.ok(readFileSync(files.comp, 'utf8').includes('id="pv-recheck"'), 'composition: re-check section');
const tampered = structuredClone(prove);
tampered.claims.find((c) => c.id === 'deadlock_free').certificate.trace = [];
// the packaged copy is byte-identical to compose/verify.mjs (checked above)
const nodeVerify = (await import(join(dir, 'views', 'verify-record.mjs'))).verifyRecord;
assert.equal(nodeVerify(flat.net, flat.members, prove).valid, true);
assert.equal(nodeVerify(flat.net, flat.members, tampered).valid, false);

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
    await page.waitForSelector('#pv-body .pv-v');
    return { page, problems, requests };
  };
  try {
    for (const k of Object.keys(tool)) {
      const { page, problems, requests } = await open(k, k === 'dead' ? 'dark' : 'light');
      const t = tool[k];
      const cards = await page.evaluate(() => [...document.querySelectorAll('#pv-body .pv-v')].map((e) => ({
        kind: e.dataset.kind, status: e.dataset.status, text: e.innerText, replay: e.querySelector('[data-replay]')?.dataset.replay ?? null })));
      assert.deepEqual(cards.map((c) => [c.kind, c.status]), t.verdicts.map((v) => [v.property.kind, v.status]), `${k}: verdicts are the tool's, in order`);
      t.verdicts.forEach((v, i) => {
        if (v.detail) assert.ok(cards[i].text.includes(v.detail), `${k}/${v.property.kind}: detail`);
        if (v.evidence) assert.ok(cards[i].text.includes(v.evidence), `${k}/${v.property.kind}: evidence`);
        if (v.counterexample) {
          for (const step of v.counterexample.trace) assert.ok(cards[i].text.includes(step), `${k}: trace step ${step}`);
          assert.equal(cards[i].replay, 'ok', `${k}/${v.property.kind}: the trace replays on the embedded engine to the stated marking`);
        } else assert.equal(cards[i].replay, null);
        if (v.status === 'unknown') assert.match(cards[i].text, /search was bounded/, `${k}/${v.property.kind}: unknown says the search was bounded`);
        else assert.ok(!/is not a pass/.test(cards[i].text));
        if (v.status === 'unknown' && t.truncated_reason) assert.ok(cards[i].text.includes(t.truncated_reason), `${k}/${v.property.kind}: unknown gives the engine's reason`);
        if (v.property.kind === 'deadlock-free') assert.match(cards[i].text, /reachability verdict in the analysis section/, `${k}: deadlock-free states its definition`);
      });
      const sum = await page.evaluate(() => ({ ...document.getElementById('pv-sum').dataset }));
      for (const s of ['proved', 'refuted', 'unknown']) assert.equal(sum[s], String(t[s]), `${k}: ${s} count`);
      assert.equal(sum.states, String(t.state_count));
      assert.equal(sum.truncated, String(t.truncated));
      // the copied record
      await page.click('#pv-copy');
      const text = await page.inputValue('#pv-text');
      assert.ok(text.startsWith('pflow proof record'), k);
      assert.ok(text.includes(cids[k]), `${k}: record carries the net CID`);
      assert.ok(text.includes(`pflow-wasm ${(await page.innerText('.pf-foot')).match(/engine (\S+)/)[1]}`) && text.includes(`pflow-mcp ${pkg.version}`), `${k}: record carries versions`);
      for (const v of t.verdicts) {
        assert.ok(text.includes(`${v.property.kind}: ${v.status.toUpperCase()}`), `${k}: record line for ${v.property.kind}`);
        if (v.evidence) assert.ok(text.includes(v.evidence));
        if (v.counterexample) assert.ok(text.includes(v.counterexample.trace.join(' -> ')));
      }
      assert.ok(text.includes(`summary: ${t.proved} proved, ${t.refuted} refuted, ${t.unknown} unknown`));
      assert.ok(text.includes(`max_states ${MAX}`));
      if (t.unknown) assert.match(text, /unknown is not a pass: the search was bounded/);
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `${k}: no horizontal scroll at 375px`);
      assert.deepEqual(requests, [], `${k}: no network requests`);
      assert.deepEqual(problems, [], `${k}: no console errors`);
      await page.close();
      ok(`${k}: page verdicts, evidence, traces and the copied record match the verify tool`);
    }
    // the composition's re-check, engine-free, in the page
    {
      const { page, problems, requests } = await open('comp');
      await page.fill('#pv-rec', JSON.stringify(prove));
      await page.fill('#pv-mem', JSON.stringify(flat));
      await page.click('#pv-run');
      const good = await page.evaluate(() => ({ st: document.getElementById('pv-rstat').textContent, bad: document.querySelectorAll('#pv-checks .pv-bad').length, good: document.querySelectorAll('#pv-checks .pv-ok').length }));
      const expect = nodeVerify(flat.net, flat.members, prove);
      assert.equal(good.bad, 0);
      assert.equal(good.good, expect.checks.length, 'the page runs the same checks as verify.mjs');
      assert.match(good.st, /passed/);
      await page.fill('#pv-rec', JSON.stringify(tampered));
      await page.click('#pv-run');
      const bad = await page.evaluate(() => ({ st: document.getElementById('pv-rstat').textContent, bad: document.querySelectorAll('#pv-checks .pv-bad').length }));
      assert.ok(bad.bad > 0 && /failed/.test(bad.st), 'a tampered record is rejected on the page');
      await page.fill('#pv-rec', 'not json');
      await page.click('#pv-run');
      assert.match(await page.innerText('#pv-rstat'), /Could not re-check/);
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'comp: no horizontal scroll at 375px');
      assert.deepEqual(requests, []); assert.deepEqual(problems, []);
      // verdicts on the composite are the tool's too
      const kinds = await page.evaluate(() => [...document.querySelectorAll('#pv-body .pv-v')].map((e) => [e.dataset.kind, e.dataset.status]));
      assert.deepEqual(kinds, tool.comp.verdicts.map((v) => [v.property.kind, v.status]));
      await page.close();
      ok('composition: verdicts match the tool; the engine-free re-checker accepts the engine record and rejects a tampered one');
    }
  } finally { await browser.close(); }
}
for (const f of Object.values(files)) rmSync(f, { force: true });
await client.close();
