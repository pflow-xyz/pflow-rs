#!/usr/bin/env node
// build.mjs: assemble @pflow-xyz/pflow-wasm from wasm-bindgen's output. No
// dependencies. `make wasm-npm` runs cargo, wasm-bindgen and wasm-opt first.
//
//   node crates/pflow-wasm/npm/build.mjs <wasm-bindgen out dir> <optimized .wasm> <package dir>
//
// Package layout:
//   pflow-wasm.inline.js   the default entry: glue + the module gzipped then base64, no fetch
//   pflow-wasm.js          the split entry: glue, fetches pflow-wasm_bg.wasm beside it
//   pflow-wasm_bg.wasm     the optimized module
//   pflow-wasm.d.ts, pflow-wasm.inline.d.ts, README.md, LICENSE, package.json
//
// wasm-bindgen's glue is edited as text, so each edit asserts that what it
// replaces is there: a new wasm-bindgen fails this build rather than producing
// an entry that quietly fetches.
//
// Why gzip inside the base64: gzip finds few matches in base64 text, because a
// repeat only stays one when it falls on a 3-byte boundary. Measured on this
// module, plain base64 is 224 KB gzipped and 183 KB in brotli; gzip-then-base64
// is 162 KB either way, and the file itself is 216 KB rather than 608 KB. The
// price is DecompressionStream, so the embedded path is async.
import { copyFileSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const [bindgenDir, wasmPath, outDir] = process.argv.slice(2);
if (!bindgenDir || !wasmPath || !outDir) {
  console.error('usage: build.mjs <wasm-bindgen out dir> <optimized .wasm> <package dir>');
  process.exit(2);
}

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..', '..');

const cargo = readFileSync(join(repo, 'Cargo.toml'), 'utf8');
const version = cargo.match(/\[workspace\.package\]\s*\nversion = "([^"]+)"/)?.[1];
if (!version) throw new Error('no [workspace.package] version in Cargo.toml');

/** `text` with the one occurrence of `from` replaced; throws if it is not there exactly once. */
function replaceOnce(text, from, to, what) {
  const parts = text.split(from);
  if (parts.length !== 2) {
    throw new Error(`${what}: expected one ${JSON.stringify(from)}, found ${parts.length - 1}`);
  }
  return parts.join(to);
}

const glue = readFileSync(join(bindgenDir, 'pflow-wasm.js'), 'utf8');
const EXPORT_LINE = 'export { initSync, __wbg_init as default };';
const DEFAULT_URL = "new URL('pflow-wasm_bg.wasm', import.meta.url)";
const core = replaceOnce(glue, EXPORT_LINE, '', 'glue exports');

const wrapper = `
/**
 * Loads the module. Pass a URL (string or URL), a Response, a WebAssembly.Module,
 * or the module's bytes (Uint8Array or ArrayBuffer), or a promise of any of them.
 * Safe to call twice.
 */
export default function init(source) {
  return __wbg_init(source === undefined ? undefined : { module_or_path: source });
}

/** init() without a promise, from bytes or a WebAssembly.Module. */
function syncInit(source) {
  return initSync({ module: source });
}
export { syncInit as initSync };
`;

const splitEntry = core + wrapper;

const wasm = readFileSync(wasmPath);
const inlineEntry =
  replaceOnce(core, DEFAULT_URL, 'wasmBytes()', 'default wasm URL') +
  wrapper +
  `
async function wasmBytes() {
  const text = atob(WASM_GZIP_BASE64);
  const gzipped = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) gzipped[i] = text.charCodeAt(i);
  const stream = new Blob([gzipped]).stream().pipeThrough(new DecompressionStream('gzip'));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

const WASM_GZIP_BASE64 = "${gzipSync(wasm, { level: 9 }).toString('base64')}";
`;
if (inlineEntry.includes('import.meta')) throw new Error('inline entry still refers to import.meta');

const dts = readFileSync(join(bindgenDir, 'pflow-wasm.d.ts'), 'utf8');
const dtsCut = dts.indexOf('export type InitInput');
if (dtsCut < 0) throw new Error('no InitInput in the generated .d.ts');
const types = (initDoc) =>
  `${dts.slice(0, dtsCut)}export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

/** ${initDoc} Safe to call twice. */
export default function init(source?: InitInput | Promise<InitInput>): Promise<unknown>;

/** init() without a promise, from bytes or a WebAssembly.Module. */
export function initSync(source: BufferSource | WebAssembly.Module): unknown;
`;

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, 'pflow-wasm.inline.js'), inlineEntry);
writeFileSync(join(outDir, 'pflow-wasm.js'), splitEntry);
copyFileSync(wasmPath, join(outDir, 'pflow-wasm_bg.wasm'));
writeFileSync(
  join(outDir, 'pflow-wasm.inline.d.ts'),
  types('Loads the module embedded in this file, with no network request, or the one you pass.'),
);
writeFileSync(
  join(outDir, 'pflow-wasm.d.ts'),
  types('Loads the module; by default it fetches pflow-wasm_bg.wasm beside this file.'),
);
writeFileSync(
  join(outDir, 'README.md'),
  readFileSync(join(here, 'README.md'), 'utf8').replaceAll('__VERSION__', version),
);
copyFileSync(join(repo, 'LICENSE'), join(outDir, 'LICENSE'));

const INLINE = './pflow-wasm.inline.js';
const pkg = {
  name: '@pflow-xyz/pflow-wasm',
  version,
  description:
    'The pflow-rs Petri net engine as WebAssembly: load a net, fire it, run the ODE and the byte-exact portable SSA. One self-contained ES module, no network requests.',
  type: 'module',
  license: 'MIT',
  homepage: 'https://pflow.xyz',
  repository: {
    type: 'git',
    url: 'git+https://github.com/pflow-xyz/pflow-rs.git',
    directory: 'crates/pflow-wasm',
  },
  bugs: { url: 'https://github.com/pflow-xyz/pflow-rs/issues' },
  keywords: ['petri-net', 'wasm', 'webassembly', 'ode-solver', 'gillespie', 'simulation', 'state-machine'],
  main: INLINE,
  module: INLINE,
  browser: INLINE,
  types: './pflow-wasm.inline.d.ts',
  jsdelivr: INLINE,
  unpkg: INLINE,
  exports: {
    '.': { types: './pflow-wasm.inline.d.ts', default: INLINE },
    './inline': { types: './pflow-wasm.inline.d.ts', default: INLINE },
    './split': { types: './pflow-wasm.d.ts', default: './pflow-wasm.js' },
    './pflow-wasm_bg.wasm': './pflow-wasm_bg.wasm',
    './package.json': './package.json',
  },
  sideEffects: false,
  files: [
    'pflow-wasm.inline.js',
    'pflow-wasm.inline.d.ts',
    'pflow-wasm.js',
    'pflow-wasm.d.ts',
    'pflow-wasm_bg.wasm',
    'LICENSE',
    'README.md',
  ],
  publishConfig: { access: 'public' },
};
writeFileSync(join(outDir, 'package.json'), JSON.stringify(pkg, null, 2) + '\n');

const line = (name) => {
  const bytes = readFileSync(join(outDir, name));
  return `  ${name.padEnd(24)} ${String(statSync(join(outDir, name)).size).padStart(9)} raw ${String(gzipSync(bytes, { level: 9 }).length).padStart(9)} gzip`;
};
console.log(`${pkg.name}@${version} in ${outDir}`);
for (const name of ['pflow-wasm.inline.js', 'pflow-wasm.js', 'pflow-wasm_bg.wasm']) console.log(line(name));
