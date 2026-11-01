#!/usr/bin/env node
// test-csp.mjs: the package under a sandbox's Content-Security-Policy, in a real
// browser. test.mjs stubs fetch; only a browser enforces CSP.
//
//   node crates/pflow-wasm/npm/test-csp.mjs <package dir>
//
// The page is on one origin and the package on another, as with a CDN, and
// connect-src allows only the page's own origin. Skips, saying so, if there is no
// Chrome ($CHROME, default google-chrome).
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const pkgDir = resolve(process.argv[2] ?? 'target/npm/pflow-wasm');
const chrome = process.env.CHROME ?? 'google-chrome';
if (spawnSync(chrome, ['--version']).error) {
  console.log(`SKIP csp: no browser at ${chrome}`);
  process.exit(0);
}

const TYPES = { '.js': 'text/javascript', '.wasm': 'application/wasm' };
const requested = [];
let page = '';
let csp = '';
let onReport = () => {};

const server = createServer((req, res) => {
  const name = new URL(req.url, 'http://x').pathname.slice(1);
  if (name === 'page.html') {
    res.writeHead(200, { 'content-type': 'text/html', 'content-security-policy': csp }).end(page);
    return;
  }
  if (name === 'report') {
    let body = '';
    req.on('data', (chunk) => (body += chunk));
    req.on('end', () => {
      res.writeHead(204).end();
      onReport(body);
    });
    return;
  }
  const ext = name.slice(name.lastIndexOf('.'));
  if (!TYPES[ext]) {
    res.writeHead(404).end();
    return;
  }
  requested.push(name);
  res
    .writeHead(200, { 'content-type': TYPES[ext], 'access-control-allow-origin': '*' })
    .end(readFileSync(join(pkgDir, name)));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;
const pageOrigin = `http://127.0.0.1:${port}`;
const cdnOrigin = `http://localhost:${port}`;

/**
 * Loads `entry` from the CDN origin under a policy; returns what the page reported.
 * The page posts its result to its own origin, which connect-src allows, and this
 * waits for that post. Chrome's --virtual-time-budget with --dump-dom would race
 * the module's compile: on a slow machine the page is dumped before it has finished.
 */
async function run(entry, { wasmEval }) {
  requested.length = 0;
  csp = [
    "default-src 'none'",
    `script-src ${cdnOrigin} 'unsafe-inline'${wasmEval ? " 'wasm-unsafe-eval'" : ''}`,
    "connect-src 'self'",
  ].join('; ');
  page = `<!doctype html><script type="module">
    let result;
    try {
      const { default: init, Net } = await import("${cdnOrigin}/${entry}");
      await init();
      const net = new Net({ name: "n", places: [{ id: "a", initial: 1 }, { id: "b" }],
        transitions: [{ id: "t" }], arcs: [{ from: "a", to: "t" }, { from: "t", to: "b" }] });
      result = "OK " + JSON.stringify(net.fire("t"));
    } catch (e) {
      result = "FAILED " + e.name + ": " + e.message;
    }
    await fetch("/report", { method: "POST", body: result });
  </script>`;
  const profile = mkdtempSync(join(tmpdir(), 'pflow-wasm-chrome-'));
  const browser = spawn(
    chrome,
    ['--headless=new', '--no-sandbox', '--disable-gpu', `--user-data-dir=${profile}`, `${pageOrigin}/page.html`],
    // Its own process group: Chrome's children outlive a kill of the parent alone,
    // and keep writing to the profile.
    { stdio: 'ignore', detached: true },
  );
  const closed = new Promise((r) => browser.once('close', r));
  let timer;
  try {
    const body = await new Promise((done, fail) => {
      onReport = done;
      browser.once('error', fail);
      timer = setTimeout(() => fail(new Error(`no report from the page for ${entry} in 120s`)), 120000);
    });
    return { body, requested: [...requested] };
  } finally {
    clearTimeout(timer);
    try {
      process.kill(-browser.pid, 'SIGKILL');
    } catch {
      // Already gone.
    }
    await closed;
    // Best effort: a temp directory left behind is not a result of this test.
    try {
      rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
    } catch (e) {
      console.log(`note: left ${profile}: ${e.code}`);
    }
  }
}

try {
  const inline = await run('pflow-wasm.inline.js', { wasmEval: true });
  assert.ok(inline.body.startsWith('OK {"a":0,"b":1}'), `inline: ${inline.body}`);
  assert.deepEqual(inline.requested, ['pflow-wasm.inline.js'], 'inline made a further request');
  console.log('ok   inline entry runs cross-origin under connect-src self, one request only');

  // The reason the inline entry is the default: the split entry cannot work here.
  const split = await run('pflow-wasm.js', { wasmEval: true });
  assert.ok(split.body.startsWith('FAILED'), `split should be blocked: ${split.body}`);
  assert.ok(!split.requested.includes('pflow-wasm_bg.wasm'), 'the .wasm fetch was not blocked');
  console.log(`ok   split entry is blocked by connect-src, as expected: ${split.body}`);

  // What the README states: embedding avoids the fetch, not the compile.
  const noEval = await run('pflow-wasm.inline.js', { wasmEval: false });
  assert.ok(noEval.body.startsWith('FAILED'), `no wasm-unsafe-eval should fail: ${noEval.body}`);
  console.log(`ok   without wasm-unsafe-eval the module cannot compile: ${noEval.body}`);
} finally {
  server.close();
}
