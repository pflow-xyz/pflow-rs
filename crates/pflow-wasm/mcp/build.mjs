#!/usr/bin/env node
// Assemble @pflow-xyz/pflow-mcp: server.mjs + the wasm package's inline entry.
//   node crates/pflow-wasm/mcp/build.mjs <built pflow-wasm package dir> <out dir>
// `make wasm-mcp` runs wasm-npm first.
import { copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const [wasmPkg, outDir] = process.argv.slice(2);
if (!wasmPkg || !outDir) { console.error('usage: build.mjs <pflow-wasm package dir> <out dir>  (CHYP_PKG, ONTOLOGY_DIR override the sibling checkouts)'); process.exit(2); }
const here = dirname(fileURLToPath(import.meta.url));
const version = JSON.parse(readFileSync(join(wasmPkg, 'package.json'), 'utf8')).version;

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });
for (const f of ['server.mjs', 'ontology.mjs', 'chyp.mjs']) copyFileSync(join(here, f), join(outDir, f));

// chyp-rs: `make wasm` in the chyp-rs checkout builds pkg/. Apache-2.0, so its
// LICENSE, NOTICE and THIRD-PARTY-NOTICES travel with it.
const chypPkg = process.env.CHYP_PKG ?? join(here, '..', '..', '..', '..', 'chyp-rs', 'pkg');
if (!existsSync(join(chypPkg, 'chyp_rs_bg.wasm'))) throw new Error(`no chyp-rs build at ${chypPkg}: run \`make wasm\` in chyp-rs`);
mkdirSync(join(outDir, 'chyp'));
for (const f of ['chyp_rs.js', 'chyp_rs_bg.wasm', 'LICENSE', 'NOTICE', 'THIRD-PARTY-NOTICES']) copyFileSync(join(chypPkg, f), join(outDir, 'chyp', f));

// The ontology snapshot (CC0): build/core.ttl plus generated/*.ttl, stamped.
const onto = process.env.ONTOLOGY_DIR ?? join(here, '..', '..', '..', '..', 'ontology-pflow-xyz');
if (!existsSync(join(onto, 'build', 'core.ttl'))) throw new Error(`no ontology checkout at ${onto}`);
mkdirSync(join(outDir, 'ontology'));
copyFileSync(join(onto, 'build', 'core.ttl'), join(outDir, 'ontology', 'core.ttl'));
for (const f of readdirSync(join(onto, 'generated')).filter((f) => f.endsWith('.ttl'))) copyFileSync(join(onto, 'generated', f), join(outDir, 'ontology', f));
const git = (...a) => { try { return execFileSync('git', ['-C', onto, ...a], { encoding: 'utf8' }).trim(); } catch { return null; } };
if (git('status', '--porcelain', '--', 'build', 'generated') && process.env.ALLOW_DIRTY !== '1') {
  throw new Error(`${onto}: build/ or generated/ has uncommitted changes; commit them or set ALLOW_DIRTY=1 (the snapshot is then marked dirty)`);
}
writeFileSync(join(outDir, 'ontology', 'meta.json'), JSON.stringify({
  version: readFileSync(join(onto, 'VERSION'), 'utf8').trim(), commit: git('rev-parse', '--short', 'HEAD'),
  dirty: git('status', '--porcelain', '--', 'build', 'generated') ? true : undefined,
  license: 'CC0-1.0', source: 'https://ontology.pflow.xyz/ns#', snapshot: true,
}, null, 2) + '\n');
copyFileSync(join(wasmPkg, 'pflow-wasm.inline.js'), join(outDir, 'pflow-wasm.inline.js'));
copyFileSync(join(wasmPkg, 'LICENSE'), join(outDir, 'LICENSE'));
copyFileSync(join(here, 'README.md'), join(outDir, 'README.md'));
writeFileSync(join(outDir, 'package.json'), JSON.stringify({
  name: '@pflow-xyz/pflow-mcp',
  version,
  description: 'pflow-rs Petri net engine as a local MCP server (WebAssembly, no native build)',
  license: 'MIT AND Apache-2.0 AND CC0-1.0', // engine / chyp-rs / ontology data
  repository: { type: 'git', url: 'git+https://github.com/pflow-xyz/pflow-rs.git', directory: 'crates/pflow-wasm/mcp' },
  type: 'module',
  bin: { 'pflow-mcp': 'server.mjs' },
  files: ['server.mjs', 'ontology.mjs', 'chyp.mjs', 'pflow-wasm.inline.js', 'chyp/', 'ontology/', 'LICENSE', 'README.md'],
  engines: { node: '>=18' },
  dependencies: { '@modelcontextprotocol/sdk': '^1.0.0', n3: '^1.17.0', zod: '^3.23.0' },
  publishConfig: { access: 'public' },
}, null, 2) + '\n');
console.log(`@pflow-xyz/pflow-mcp@${version} in ${outDir}`);
