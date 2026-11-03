#!/usr/bin/env node
// e2e.mjs: the built page in a real browser, under a sandbox-style CSP
// (inline script and wasm only; no connections), driving the canvas like a
// person: presets, adding parts, dragging a link between ports, re-checking.
//
//   PLAYWRIGHT_CORE=<path to playwright-core> node e2e.mjs <compose-canvas.html>
//   (CHROME defaults to google-chrome). Skips, saying so, with no browser.
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

const file = resolve(process.argv[2] ?? 'target/compose-canvas.html');
const core = process.env.PLAYWRIGHT_CORE ?? '/home/myork/Workspace/tty-vevr-se/node_modules/playwright-core';
const chrome = process.env.CHROME ?? '/usr/bin/google-chrome';
if (!existsSync(core) || !existsSync(chrome)) { console.log(`SKIP e2e: need playwright-core (${core}) and a browser (${chrome})`); process.exit(0); }
const { chromium } = createRequire(import.meta.url)(core);
const html = readFileSync(file, 'utf8');
const CSP = "default-src 'none'; script-src 'unsafe-inline' 'wasm-unsafe-eval'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'";

const browser = await chromium.launch({ executablePath: chrome, args: ['--no-sandbox'] });
const page = await browser.newPage({ viewport: { width: 1400, height: 1000 } });
const problems = [];
page.on('console', (m) => { if (m.type() === 'error') problems.push(m.text()); });
page.on('pageerror', (e) => problems.push(String(e)));
await page.route('http://page.test/c.html', (r) => r.fulfill({ status: 200, contentType: 'text/html', headers: { 'content-security-policy': CSP }, body: html }));
await page.goto('http://page.test/c.html');
const verdict = () => page.locator('.verdict').getAttribute('class');
const ok = (m) => console.log('ok', m);

await page.waitForSelector('.verdict');
assert.match(await verdict(), /v-consistent\b/);
ok('opens on the order pipeline: consistent');

// presets change the verdict
await page.getByRole('button', { name: 'Failing pool' }).click();
await page.waitForFunction(() => document.querySelector('.verdict')?.className.includes('needs_attention'));
ok('failing pool: needs attention');
await page.getByRole('button', { name: 'Rate vs pool (fails)' }).click();
await page.waitForFunction(() => document.querySelector('.verdict')?.className.includes('inconsistent'));
assert.match(await page.locator('#proof').innerText(), /broken/);
ok('rate vs pool: a join breaks a promise nothing accounts for');

// build by hand: two queues, drag out -> in
await page.getByRole('button', { name: 'Clear' }).click();
await page.locator('#palette button', { hasText: 'Bounded queue' }).click();
await page.locator('#palette button', { hasText: 'Bounded queue' }).click();
await page.waitForFunction(() => document.querySelectorAll('.card').length === 2);
const portBox = async (sub, port) => (await page.locator(`.port[data-sub="${sub}"][data-port="${port}"] .g`).boundingBox());
const out = await portBox('queue', 'out');
await page.mouse.move(out.x + out.width / 2, out.y + out.height / 2);
await page.mouse.down();
await page.waitForSelector('.port.lit');
const lit = await page.locator('.port.lit').count();
assert.ok(lit >= 1, 'legal ports light up');
const dim = await page.locator('.port.dim').count();
assert.ok(dim >= 1, 'illegal ports dim');
const target = await portBox('queue2', 'in');
await page.mouse.move(target.x + 3, target.y + 3, { steps: 6 });
await page.mouse.up();
await page.waitForFunction(() => document.querySelectorAll('.link.hit').length === 1);
await page.waitForFunction(() => /needs_attention|inconsistent/.test(document.querySelector('.verdict')?.className));
ok(`dragged a link; ${lit} ports lit, ${dim} dimmed; the proof updated`);

// the proof re-checks, twice over
await page.getByRole('button', { name: 'Re-check the record' }).click();
const rc = await page.locator('#recheckout').innerText();
assert.equal((rc.match(/passed/g) ?? []).length, 2, rc);
ok('re-check: engine and separate script both pass');

// the composite plays
const enabled = await page.locator('#fire button:not([disabled])').count();
assert.ok(enabled > 0);
await page.locator('#fire button:not([disabled])').first().click();
assert.match(await page.locator('#log').innerText(), /fired/);
ok('the composite fires');

// removing a link
const mid = await page.evaluate(() => {
  const p = document.querySelector('.link.hit'), pt = p.getPointAtLength(p.getTotalLength() / 2), m = p.getScreenCTM();
  return { x: pt.x * m.a + m.e, y: pt.y * m.d + m.f };
});
await page.mouse.click(mid.x, mid.y);
await page.waitForFunction(() => document.querySelectorAll('.link.hit').length === 0);
ok('a link is removed with a click');

// keyboard path: pick ports from lists
await page.locator('#inspect select').first().selectOption({ label: 'queue:out' });
const toOptions = await page.locator('#inspect select').nth(1).locator('option').count();
assert.ok(toOptions >= 2);
await page.locator('#inspect select').nth(1).selectOption({ index: 1 });
await page.getByRole('button', { name: 'Join', exact: true }).click();
await page.waitForFunction(() => document.querySelectorAll('.link.hit').length === 1);
ok('joining from the lists works without a pointer');

// the deadlock witness plays
await page.getByRole('button', { name: 'Order pipeline' }).click();
await page.waitForSelector('[data-witness]');
await page.locator('[data-witness]').click();
await page.waitForFunction(() => /nothing is enabled/.test(document.getElementById('log').textContent), null, { timeout: 8000 });
ok('the dead-end witness replays to a marking where nothing is enabled');

assert.deepEqual(problems, [], 'no console errors under the CSP');
ok('no console errors, no blocked requests');
await page.screenshot({ path: process.env.SHOT ?? '/tmp/compose-e2e.png', fullPage: true });
await browser.close();
