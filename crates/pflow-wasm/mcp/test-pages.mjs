// artifact_html pages and compose_canvas's footer: the page shell, the view
// registry, the #pflow-net round trip, and the page in a real browser under a
// sandbox-style CSP.
//   node crates/pflow-wasm/mcp/test-pages.mjs <built pflow-mcp dir>
//   (copy it next to the built package, or run from here: it resolves the SDK
//   through the package's node_modules)
import assert from 'node:assert/strict';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';

const dir = resolve(process.argv[2] ?? 'target/npm/pflow-mcp');
const need = createRequire(join(dir, 'package.json'));
const { Client } = await import(need.resolve('@modelcontextprotocol/sdk/client/index.js'));
const { StdioClientTransport } = await import(need.resolve('@modelcontextprotocol/sdk/client/stdio.js'));
const client = new Client({ name: 'test-pages', version: '0' });
await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(dir, 'server.mjs')] }));
const call = async (name, args = {}) => {
  const r = await client.callTool({ name, arguments: args });
  const text = r.content[0].text;
  return { error: !!r.isError, text, json: r.isError ? null : JSON.parse(text) };
};
const ok = (m) => console.log('ok', m);
const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
const onto = JSON.parse(readFileSync(join(dir, 'ontology', 'meta.json'), 'utf8'));

const coffee = { name: 'coffee', places: [{ id: 'beans', initial: 3 }, { id: 'cups' }], transitions: [{ id: 'brew', rate: 0.5 }],
  arcs: [{ from: 'beans', to: 'brew' }, { from: 'brew', to: 'cups' }] };
const loaded = await call('load_net', { model: coffee });
assert.ok(loaded.json.cid, 'coffee has a CID');

const out = join(dir, 'pages-test.html');
let r = await call('artifact_html', { net: 'coffee', path: out });
assert.ok(!r.error, r.text);
const html = readFileSync(out, 'utf8');
const coffeeFile = join(dir, 'coffee-page-test.html');
writeFileSync(coffeeFile, html);
assert.deepEqual(r.json.views, ['play']);

// the page's module parses
const script = html.split('<script type="module">')[1].split('</script>')[0];
const mjs = join(dir, 'pages-check.mjs');
writeFileSync(mjs, script);
const chk = spawnSync(process.execPath, ['--check', mjs], { encoding: 'utf8' });
rmSync(mjs);
assert.equal(chk.status, 0, chk.stderr);
ok('page module parses');

// the shell: footer facts, buttons, editor link, no network
assert.ok(html.includes(loaded.json.cid), 'footer carries the CID');
assert.match(html, /document CID/);
assert.ok(html.includes(`pflow-mcp ${pkg.version}`) && html.includes(`ontology snapshot ${onto.version}`));
assert.match(html, /engine \d+\.\d+/);
assert.match(html, /seed <span id="f-seed">1</);
assert.ok(html.includes('id="pf-download"') && html.includes('id="pf-copy"'));
assert.ok(html.includes(`https://pflow.xyz/?cid=${loaded.json.cid}`));
assert.ok(!/<iframe|<script[^>]+src=|<link[^>]+href=|fetch\(|XMLHttpRequest/.test(html.replace(script, '')), 'no iframe, external script or fetch');
assert.ok(/prefers-color-scheme:dark/.test(html) && /name="viewport"/.test(html));
ok('shell: footer, buttons, editor link, themes');

// #pflow-net round-trips through load_net with the same CID
const block = html.match(/<script type="application\/json" id="pflow-net">([\s\S]*?)<\/script>/)[1];
const doc = JSON.parse(block);
r = await call('load_net', { name: 'coffee-again', model: doc });
assert.ok(!r.error, r.text);
assert.equal(r.json.cid, loaded.json.cid, 'the embedded net reloads with the same CID');
assert.deepEqual(r.json.enabled, loaded.json.enabled);
ok('#pflow-net round-trips through load_net with the same CID');

// views: default, repeats collapse, unknown refused with the known list
r = await call('artifact_html', { net: 'coffee', views: ['play', 'play'], path: out });
assert.deepEqual(r.json.views, ['play', 'play']);
r = await call('artifact_html', { net: 'coffee', views: ['play', 'nope'], path: out });
assert.ok(r.error && /unknown view "nope"/.test(r.text) && /known views: play/.test(r.text), r.text);
ok('unknown view refused with the known ones');

// a net with no CID says so
const places = Array.from({ length: 60 }, (_, i) => ({ id: `p${i}`, initial: i === 0 ? 1 : 0 }));
const big = { name: 'big', places, transitions: places.slice(1).map((p) => ({ id: `t${p.id}` })),
  arcs: places.slice(1).flatMap((p, i) => [{ from: `p${i}`, to: `t${p.id}` }, { from: `t${p.id}`, to: p.id }]) };
r = await call('load_net', { name: 'big', model: big });
assert.equal(r.json.cid, null);
await call('artifact_html', { net: 'big', path: out });
const bigHtml = readFileSync(out, 'utf8');
assert.match(bigHtml, /document CID: <span id="pf-cid"[^>]*>not computed</);
assert.ok(!bigHtml.includes('https://pflow.xyz/?cid='), 'no editor link without a CID');
ok('big net: document CID not computed');

// compose_canvas carries the footer
const cc = join(dir, 'canvas-test.html');
r = await call('compose_canvas', { path: cc });
const canvas = readFileSync(cc, 'utf8');
assert.ok(canvas.includes('class="pf-foot"') && canvas.includes(`pflow-mcp ${pkg.version}`) && !canvas.includes('document CID'));
ok('compose_canvas footer');

// the page in a browser under a sandbox CSP: play, fire, theme, phone width, no requests
const core = process.env.PLAYWRIGHT_CORE ?? '/home/myork/Workspace/tty-vevr-se/node_modules/playwright-core';
const chrome = process.env.CHROME ?? '/usr/bin/google-chrome';
if (!existsSync(core) || !existsSync(chrome)) console.log(`SKIP browser: need playwright-core (${core}) and a browser (${chrome})`);
else {
  const { chromium } = createRequire(import.meta.url)(core);
  const CSP = "default-src 'none'; script-src 'unsafe-inline' 'wasm-unsafe-eval'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'";
  const browser = await chromium.launch({ executablePath: chrome, args: ['--no-sandbox'] });
  try {
    for (const [label, file] of [['coffee', coffeeFile], ['canvas', cc]]) {
      const page = await browser.newPage({ viewport: { width: 375, height: 800 }, colorScheme: label === 'coffee' ? 'dark' : 'light' });
      const problems = [];
      page.on('console', (m) => { if (m.type() === 'error') problems.push(m.text()); });
      page.on('pageerror', (e) => problems.push(String(e)));
      const requests = [];
      page.on('request', (q) => { if (!q.url().startsWith('http://page.test/') && !q.url().startsWith('data:')) requests.push(q.url()); });
      const body = readFileSync(file, 'utf8');
      await page.route('http://page.test/p.html', (q) => q.fulfill({ status: 200, contentType: 'text/html', headers: { 'content-security-policy': CSP }, body }));
      await page.goto('http://page.test/p.html');
      if (label === 'coffee') {
        await page.waitForSelector('#marks .mark');
        assert.equal(await page.locator('#marks .mark b').first().innerText(), '3');
        await page.getByRole('button', { name: 'brew' }).click();
        assert.equal(await page.locator('#marks .mark b').first().innerText(), '2');
        await page.waitForSelector('#chart svg');
        await page.getByRole('button', { name: 'Run SSA' }).click();
        await page.locator('#seed').fill('7');
        assert.equal(await page.locator('#f-seed').innerText(), '7', 'footer follows the seed field');
        assert.equal(await page.evaluate(() => getComputedStyle(document.body).backgroundColor), 'rgb(22, 22, 26)', 'dark theme');
        await page.getByRole('button', { name: 'Copy as text' }).click();
        await page.waitForFunction(() => document.getElementById('pf-ta').value.includes('@context'));
      }
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `${label}: no horizontal scroll at 375px`);
      assert.deepEqual(requests, [], `${label}: no network requests`);
      assert.deepEqual(problems, [], `${label}: no console errors`);
      await page.close();
      ok(`${label} page under a sandbox CSP, phone width`);
    }
  } finally { await browser.close(); }
}
rmSync(out, { force: true }); rmSync(coffeeFile, { force: true }); rmSync(cc, { force: true });
await client.close();
