#!/usr/bin/env node
// test.mjs: hold the built package to what it promises. No dependencies.
//
//   node crates/pflow-wasm/npm/test.mjs <package dir>
//
// The native tests (cargo test -p pflow-wasm) cover the logic. This runs what is
// actually published: the optimized module, through each entry, with the byte-exact
// SSA goldens, and with fetch disabled for the entry that promises not to use it.
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { gunzipSync, gzipSync } from 'node:zlib';

// The inline entry's gzipped size is what a page downloads. Raise this only on
// purpose: it exists to catch a dependency arriving unnoticed.
const INLINE_GZIP_BUDGET = 200 * 1024;

const pkgDir = resolve(process.argv[2] ?? 'target/npm/pflow-wasm');
const here = dirname(fileURLToPath(import.meta.url));
const fixtures = join(here, '..', '..', 'pflow-solver', 'tests', 'fixtures', 'ssa');
const pkg = JSON.parse(readFileSync(join(pkgDir, 'package.json'), 'utf8'));
const fileUrl = (name, query = '') => pathToFileURL(join(pkgDir, name)).href + query;

const CHAIN = {
  name: 'chain',
  places: [{ id: 'a', initial: 3 }, { id: 'b' }],
  transitions: [{ id: 't', rate: 0.5 }],
  arcs: [{ from: 'a', to: 't' }, { from: 't', to: 'b' }],
};

let passed = 0;
async function test(name, fn) {
  await fn();
  passed++;
  console.log(`ok   ${name}`);
}

/** Runs `fn` with fetch replaced by one that fails the test if it is called. */
async function withoutFetch(fn) {
  const real = globalThis.fetch;
  let calls = 0;
  globalThis.fetch = () => {
    calls++;
    throw new Error('fetch must not be called');
  };
  try {
    await fn();
  } finally {
    globalThis.fetch = real;
  }
  assert.equal(calls, 0, 'fetch was called');
}

function sameDoubles(got, want, where) {
  if (typeof want === 'number') {
    assert.ok(Object.is(got, want) || got === want, `${where}: ${got} !== ${want}`);
  } else if (Array.isArray(want)) {
    assert.equal(got.length, want.length, `${where}: length`);
    want.forEach((w, i) => sameDoubles(got[i], w, `${where}[${i}]`));
  } else {
    assert.deepEqual(Object.keys(got).sort(), Object.keys(want).sort(), `${where}: keys`);
    for (const k of Object.keys(want)) sameDoubles(got[k], want[k], `${where}.${k}`);
  }
}

await test('package.json: the inline entry is the default everywhere', () => {
  const inline = './pflow-wasm.inline.js';
  assert.equal(pkg.name, '@pflow-xyz/pflow-wasm');
  for (const field of ['main', 'module', 'browser', 'jsdelivr', 'unpkg']) assert.equal(pkg[field], inline, field);
  assert.equal(pkg.exports['.'].default, inline);
  assert.equal(pkg.exports['./split'].default, './pflow-wasm.js');
  assert.equal(pkg.license, 'MIT');
  assert.equal(pkg.publishConfig.access, 'public');
  const present = readdirSync(pkgDir);
  for (const f of pkg.files) assert.ok(present.includes(f), `${f} is in files but not built`);
});

await test('inline entry: no import.meta, no reference to the .wasm file', () => {
  const text = readFileSync(join(pkgDir, 'pflow-wasm.inline.js'), 'utf8');
  assert.ok(!text.includes('import.meta'));
  assert.ok(!text.includes('_bg.wasm'));
});

await test(`inline entry: gzipped within ${INLINE_GZIP_BUDGET} bytes`, () => {
  const size = gzipSync(readFileSync(join(pkgDir, 'pflow-wasm.inline.js')), { level: 9 }).length;
  assert.ok(size <= INLINE_GZIP_BUDGET, `${size} bytes`);
});

await test('inline entry: init() needs no network; fire and simulate work', async () => {
  await withoutFetch(async () => {
    const { default: init, Net, version } = await import(fileUrl('pflow-wasm.inline.js'));
    await init();
    assert.equal(version(), pkg.version);

    const net = new Net(CHAIN);
    assert.deepEqual(net.places(), ['a', 'b']);
    assert.deepEqual(net.transitions(), ['t']);
    assert.deepEqual(net.enabled(), ['t']);
    assert.deepEqual(net.fire('t'), { a: 2, b: 1 });
    assert.deepEqual(net.marking(), { a: 2, b: 1 });

    const ode = net.simulate({ tEnd: 100, maxPoints: 20 });
    assert.equal(ode.method, 'tsit5');
    assert.equal(ode.series.a[0], 2);
    assert.ok(Math.abs(ode.final.a + ode.final.b - 3) < 1e-6, 'conserved');
    assert.ok(ode.final.a < 1e-3, 'drained');
    assert.deepEqual(net.marking(), { a: 2, b: 1 }, 'a run does not move the marking');

    net.reset();
    assert.deepEqual(net.marking(), { a: 3, b: 0 });

    // JSON text is as good as an object.
    assert.deepEqual(new Net(JSON.stringify(CHAIN)).marking(), { a: 3, b: 0 });
  });
});

await test('inline entry: errors arrive as JS errors and the module survives them', async () => {
  const { default: init, Net } = await import(fileUrl('pflow-wasm.inline.js'));
  await init();
  const net = new Net(CHAIN);
  for (let i = 0; i < 3; i++) net.fire('t');
  assert.throws(() => net.fire('t'), /a/);
  assert.throws(() => net.fire('nope'));
  assert.throws(() => net.simulate({ tEndd: 5 }), /tEndd/);
  assert.throws(() => new Net('not json'));
  assert.throws(() => new Net({ name: 'g', places: [], transitions: [{ id: 't', guard: 'x > 1' }], arcs: [] }), /guard/);
  assert.throws(() => new Net({ name: 'd', places: [{ id: 'p' }], transitions: [], arcs: [{ from: 'p', to: 'zz' }] }), /join/);
  assert.deepEqual(net.marking(), { a: 0, b: 3 });
});

await test('inline entry: Shape A (pflow.xyz) loads', async () => {
  const { default: init, Net } = await import(fileUrl('pflow-wasm.inline.js'));
  await init();
  const net = new Net({
    '@type': 'PetriNet',
    places: { a: { initial: [2], x: 0, y: 0 }, b: { initial: [0], x: 0, y: 0 } },
    transitions: { t: { x: 0, y: 0 } },
    arcs: [{ source: 'a', target: 't', weight: [1] }, { source: 't', target: 'b', weight: [1] }],
  });
  assert.deepEqual(net.fire('t'), { a: 1, b: 1 });
});

for (const name of ['chain', 'sir', 'dimer', 'coffeeshop', 'gates', 'timed']) {
  await test(`optimized module is byte-exact with the go-pflow SSA golden: ${name}`, async () => {
    const { default: init, Net } = await import(fileUrl('pflow-wasm.inline.js'));
    await init();
    const golden = JSON.parse(readFileSync(join(fixtures, `${name}.json`), 'utf8'));
    const got = new Net(golden.model).stochastic(golden.options);
    sameDoubles(got.times, golden.expected.times, `${name}.times`);
    sameDoubles(got.series, golden.expected.series, `${name}.series`);
    sameDoubles(got.final, golden.expected.final, `${name}.final`);
  });
}

await test('the embedded module is exactly the published .wasm', async () => {
  const text = readFileSync(join(pkgDir, 'pflow-wasm.inline.js'), 'utf8');
  const b64 = text.match(/const WASM_GZIP_BASE64 = "([^"]+)"/)?.[1];
  assert.ok(b64, 'no embedded module');
  assert.ok(gunzipSync(Buffer.from(b64, 'base64')).equals(readFileSync(join(pkgDir, 'pflow-wasm_bg.wasm'))));
});

await test('both entries: init accepts bytes, an ArrayBuffer and a promise, without fetch', async () => {
  const bytes = readFileSync(join(pkgDir, 'pflow-wasm_bg.wasm'));
  const copy = new Uint8Array(bytes);
  const forms = { u8: copy, buffer: copy.buffer, promise: Promise.resolve(copy) };
  for (const entry of ['pflow-wasm.inline.js', 'pflow-wasm.js']) {
    for (const [form, source] of Object.entries(forms)) {
      await withoutFetch(async () => {
        const { default: init, Net } = await import(fileUrl(entry, `?bytes-${form}`));
        await init(source);
        assert.deepEqual(new Net(CHAIN).marking(), { a: 3, b: 0 }, `${entry} ${form}`);
      });
    }
  }
});

await test('both entries: initSync(bytes)', async () => {
  for (const entry of ['pflow-wasm.inline.js', 'pflow-wasm.js']) {
    const { initSync, Net } = await import(fileUrl(entry, '?sync'));
    initSync(new Uint8Array(readFileSync(join(pkgDir, 'pflow-wasm_bg.wasm'))));
    assert.deepEqual(new Net(CHAIN).marking(), { a: 3, b: 0 }, entry);
  }
});

await test('split entry: init() fetches the .wasm beside it; init(url) takes a string or URL', async () => {
  const server = createServer((req, res) => {
    const name = new URL(req.url, 'http://x').pathname.slice(1);
    const types = { 'pflow-wasm.js': 'text/javascript', 'pflow-wasm_bg.wasm': 'application/wasm' };
    if (!types[name]) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'content-type': types[name] }).end(readFileSync(join(pkgDir, name)));
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const wasmUrl = `http://127.0.0.1:${server.address().port}/pflow-wasm_bg.wasm`;
  try {
    for (const [form, source] of [['string', wasmUrl], ['url', new URL(wasmUrl)]]) {
      const { default: init, Net } = await import(fileUrl('pflow-wasm.js', `?${form}`));
      await init(source);
      assert.deepEqual(new Net(CHAIN).marking(), { a: 3, b: 0 }, form);
    }
    // No argument: the sibling file. Node cannot fetch file: URLs, so this
    // redirects the fetch to the server and checks what was asked for.
    const real = globalThis.fetch;
    let asked;
    globalThis.fetch = (u) => {
      asked = String(u);
      return real(wasmUrl);
    };
    try {
      const { default: init, Net } = await import(fileUrl('pflow-wasm.js', '?default'));
      await init();
      assert.ok(asked.endsWith('/pflow-wasm_bg.wasm'), asked);
      assert.deepEqual(new Net(CHAIN).marking(), { a: 3, b: 0 });
    } finally {
      globalThis.fetch = real;
    }
  } finally {
    server.close();
  }
});

await test('README: the quick start runs as written, with no network', async () => {
  const readme = readFileSync(join(pkgDir, 'README.md'), 'utf8');
  assert.ok(!readme.includes('__VERSION__'));
  const url = `https://cdn.jsdelivr.net/npm/${pkg.name}@${pkg.version}/pflow-wasm.inline.js`;
  const script = readme.match(/<script type="module">\n([\s\S]*?)<\/script>/)?.[1];
  assert.ok(script, 'no quick start script');
  assert.ok(script.includes(url), `quick start does not import ${url}`);

  const dir = mkdtempSync(join(tmpdir(), 'pflow-wasm-readme-'));
  const logged = [];
  const realLog = console.log;
  const out = { textContent: '' };
  globalThis.document = { getElementById: () => out };
  try {
    const path = join(dir, 'snippet.mjs');
    writeFileSync(path, script.replace(url, fileUrl('pflow-wasm.inline.js', '?readme')));
    console.log = (...args) => logged.push(args.join(' '));
    await withoutFetch(() => import(pathToFileURL(path).href));
  } finally {
    console.log = realLog;
    delete globalThis.document;
    rmSync(dir, { recursive: true, force: true });
  }
  assert.equal(logged.length, 1);
  assert.equal(out.textContent, logged[0]);
  const lines = out.textContent.split('\n');
  assert.equal(lines[0], `pflow-wasm ${pkg.version}`);
  assert.equal(lines[1], 'enabled: brew');
  assert.equal(lines[2], 'after brew: {"beans":2,"cups":1}');
  assert.match(lines[3], /^ODE at t=10: /);
  assert.match(lines[4], /^SSA mean at t=10: /);
  realLog(out.textContent.replace(/^/gm, '     '));
});

console.log(`${passed} passed`);
