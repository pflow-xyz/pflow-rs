// The whatif view: a generated page renders its sections under a sandbox CSP,
// and the numbers it computes at page time (SSA both arms on one seed, ODE, the
// difference) equal what the what_if tool returns for the same net and seed.
//   node crates/pflow-wasm/mcp/test-whatif.mjs <built pflow-mcp dir>
import assert from 'node:assert/strict';
import { existsSync, readFileSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';

const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const need = createRequire(join(dir, 'package.json'));
const { Client } = await import(need.resolve('@modelcontextprotocol/sdk/client/index.js'));
const { StdioClientTransport } = await import(need.resolve('@modelcontextprotocol/sdk/client/stdio.js'));
const client = new Client({ name: 'test-whatif', version: '0' });
await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')] }));
const call = async (name, args = {}) => {
  const r = await client.callTool({ name, arguments: args });
  const text = r.content[0].text;
  return { error: !!r.isError, text, json: r.isError ? null : JSON.parse(text) };
};
const ok = (m) => console.log('ok', m);

const net = { name: 'shop', places: [{ id: 'beans', initial: 6 }, { id: 'cups' }, { id: 'sold' }],
  transitions: [{ id: 'brew', rate: 0.5 }, { id: 'sell', rate: 0.3 }],
  arcs: [{ from: 'beans', to: 'brew' }, { from: 'brew', to: 'cups' }, { from: 'cups', to: 'sell' }, { from: 'sell', to: 'sold' }] };
assert.ok(!(await call('load_net', { model: net })).error);
const out = join(dir, 'whatif-test.html');
let r = await call('artifact_html', { net: 'shop', views: ['whatif'], path: out });
assert.ok(!r.error, r.text);
assert.deepEqual(r.json.views, ['whatif']);
const html = readFileSync(out, 'utf8');
assert.ok(html.includes('data-view="whatif"') && html.includes('id="wi-table"') && html.includes('id="wi-run"'));
assert.ok(!/<iframe|<script[^>]+src=|fetch\(|XMLHttpRequest/.test(html.replace(/<script type="module">[\s\S]*?<\/script>/, '')));
ok('page carries the whatif sections');

// the tool's answer for the default scenario of the view (beans +1, t 10, 16 runs, seed 7)
const tool = await call('what_if', { net: 'shop', place: 'beans', delta: 1, t_end: 10, runs: 16, seed: 7, samples: 80 });
assert.ok(!tool.error, tool.text);
const finalMean = (arm, place) => tool.json.finals[arm].rows.find((x) => x[0] === place)[1];

const core = process.env.PLAYWRIGHT_CORE ?? '/home/myork/Workspace/tty-vevr-se/node_modules/playwright-core';
const chrome = process.env.CHROME ?? '/usr/bin/google-chrome';
if (!existsSync(core) || !existsSync(chrome)) console.log(`SKIP browser: need playwright-core (${core}) and a browser (${chrome})`);
else {
  const { chromium } = createRequire(import.meta.url)(core);
  const CSP = "default-src 'none'; script-src 'unsafe-inline' 'wasm-unsafe-eval'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'";
  const browser = await chromium.launch({ executablePath: chrome, args: ['--no-sandbox'] });
  try {
    const page = await browser.newPage({ viewport: { width: 375, height: 800 }, colorScheme: 'dark' });
    const problems = [], requests = [];
    page.on('console', (m) => { if (m.type() === 'error') problems.push(m.text()); });
    page.on('pageerror', (e) => problems.push(String(e)));
    page.on('request', (q) => { if (!q.url().startsWith('http://page.test/') && !q.url().startsWith('data:')) requests.push(q.url()); });
    await page.route('http://page.test/p.html', (q) => q.fulfill({ status: 200, contentType: 'text/html', headers: { 'content-security-policy': CSP }, body: html }));
    await page.goto('http://page.test/p.html');
    await page.waitForSelector('#wi-diff[data-ready="1"]');
    assert.equal(await page.locator('#wi-ssa-a svg, #wi-ssa-b svg, #wi-ode-a svg, #wi-ode-b svg').count(), 4, 'four charts');
    assert.equal(await page.locator('#wi-b-name').innerText(), '+1 beans');
    const cells = await page.evaluate(() => [...document.querySelectorAll('#wi-table tbody tr')].map((tr) => ({
      place: tr.dataset.place, a: +tr.querySelector('.wi-fa').dataset.v, b: +tr.querySelector('.wi-fb').dataset.v,
      ds: +tr.querySelector('.wi-ds').dataset.v })));
    assert.deepEqual(cells.map((c) => c.place), ['beans', 'cups', 'sold']);
    for (const c of cells) {
      assert.equal(c.a, finalMean(0, c.place), `${c.place}: baseline final mean equals what_if's`);
      assert.equal(c.b, finalMean(1, c.place), `${c.place}: changed final mean equals what_if's`);
      assert.equal(c.ds, c.b - c.a, `${c.place}: difference is changed minus as-is`);
    }
    ok('page values equal the what_if tool at seed 7');
    // seed follows the footer; a transition switched off holds its downstream at zero
    await page.locator('#wi-kind').selectOption('off');
    await page.locator('#wi-trans').selectOption('brew');
    await page.getByRole('button', { name: 'Run both arms' }).click();
    assert.equal(await page.locator('#wi-b-name').innerText(), 'brew off (rate 0)');
    const sold = await page.evaluate(() => +document.querySelector('#wi-table tr[data-place=sold] .wi-fb').dataset.v);
    assert.equal(sold, 0, 'with brew off nothing is sold');
    const off = await call('stochastic', { net: 'shop', horizon: 10, samples: 80, realizations: 16, seed: 7, rates: { brew: 0 } });
    assert.equal(await page.evaluate(() => +document.querySelector('#wi-table tr[data-place=beans] .wi-fb').dataset.v), off.json.final.beans);
    assert.equal(await page.locator('#f-seed').innerText(), '7');
    // unremovable tokens: an error, not a number
    await page.locator('#wi-kind').selectOption('tokens');
    await page.locator('#wi-delta').fill('-50');
    await page.getByRole('button', { name: 'Run both arms' }).click();
    assert.match(await page.locator('#wi-msg').innerText(), /cannot remove 50/);
    await page.locator('#wi-delta').fill('1');
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'no horizontal scroll at 375px');
    assert.deepEqual(requests, [], 'no network requests');
    assert.deepEqual(problems, [], 'no console errors');
    ok('transition off, refusal, phone width, no network, no console errors');
    // the hidden attribute wins over the label's flex display, and the whatif-only legend has swatches
    await page.locator('#wi-kind').selectOption('tokens');
    assert.equal(await page.evaluate(() => getComputedStyle(document.getElementById('wi-tl')).display), 'none', 'transition picker hidden for a place change');
    await page.locator('#wi-kind').selectOption('off');
    assert.equal(await page.evaluate(() => getComputedStyle(document.getElementById('wi-pl')).display), 'none', 'place picker hidden for a transition change');
    assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector('.sw')).width), '10px', 'legend swatches are styled without the play view');
    // a multi-colour net and a net with nothing to change each render a page, not a thrown error
    const cases = {
      colour: { '@context': 'https://pflow.xyz/schema', '@type': 'PetriNet', '@version': '1.1', name: 'col', token: ['https://pflow.xyz/tokens/black', 'https://pflow.xyz/tokens/red'],
        places: { a: { '@type': 'Place', initial: [2, 1], capacity: [0, 0], x: 1, y: 1 }, b: { '@type': 'Place', initial: [0, 0], capacity: [0, 0], x: 2, y: 1 } },
        transitions: { t: { '@type': 'Transition', x: 1, y: 2 } },
        arcs: [{ '@type': 'Arrow', source: 'a', target: 't', weight: [1, 1] }, { '@type': 'Arrow', source: 't', target: 'b', weight: [1, 1] }] },
      empty: { name: 'empty', places: [], transitions: [], arcs: [] },
    };
    for (const [k, model] of Object.entries(cases)) {
      const lr = await call('load_net', { model });
      assert.ok(!lr.error, `${k}: ${lr.text}`);
      const f = join(dir, `whatif-${k}.html`);
      const ar = await call('artifact_html', { net: model.name, views: ['whatif'], path: f });
      assert.ok(!ar.error, ar.text);
      const p2 = await browser.newPage({ viewport: { width: 375, height: 800 } });
      await p2.route('http://page.test/q.html', (q) => q.fulfill({ status: 200, contentType: 'text/html', headers: { 'content-security-policy': CSP }, body: readFileSync(f, 'utf8') }));
      await p2.goto('http://page.test/q.html');
      await p2.waitForTimeout(1500);
      const msg = await p2.locator('#wi-msg').innerText();
      if (k === 'colour') {
        assert.doesNotMatch(msg, /undefined|Cannot read/, 'multi-colour: no thrown error');
        assert.ok(await p2.locator('#wi-table tbody tr').count() >= 2, 'multi-colour: the table has rows');
        assert.ok(await p2.locator('#wi-ssa-a svg').count() === 1, 'multi-colour: charts drawn');
        assert.doesNotMatch(await p2.locator('#wi-place').innerHTML(), /a\.black/, 'a colour component is not offered as a place');
        assert.match(await p2.locator('#wi-note').innerText(), /several colours/);
      } else {
        assert.match(msg, /Nothing to change/, 'empty net: says there is nothing to change');
        assert.doesNotMatch(msg, /rates/);
      }
      rmSync(f, { force: true });
      await p2.close();
    }
    ok('multi-colour and empty nets, hidden controls, legend swatches');
  } finally { await browser.close(); }
}
rmSync(out, { force: true });
await client.close();
