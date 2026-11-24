// check-verdicts.mjs: run the engine's example set through the built WASM module and hold it to
// the recorded verdicts (the WASM build is the reference).
//
//   node check-verdicts.mjs <outdir> <cases.json> <recorded.json>            check
//   node check-verdicts.mjs <outdir> <cases.json> <recorded.json> --record   write recorded.json
//   node check-verdicts.mjs <outdir> <cases.json> -                          print the results
//
// <outdir> is scripts/build-wasm-repro.sh's output. The module is checked against its own
// manifest.json first, so a verdict is always tied to the engine sha256 that produced it.
import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [outdir, casesPath, recordedPath, flag] = process.argv.slice(2);
if (!outdir || !casesPath || !recordedPath) {
  console.error("usage: check-verdicts.mjs <outdir> <cases.json> <recorded.json|-> [--record]");
  process.exit(2);
}
const wasm = readFileSync(join(outdir, "pflow_engine_bg.wasm"));
const sha = createHash("sha256").update(wasm).digest("hex");
const manifest = JSON.parse(readFileSync(join(outdir, "manifest.json"), "utf8"));
if (manifest.engine_sha256 !== sha) {
  console.error(`check-verdicts: the module's sha256 is ${sha}, its manifest says ${manifest.engine_sha256}`);
  process.exit(1);
}
const engine = await import(pathToFileURL(resolve(outdir, "pflow_engine.js")).href);
engine.initSync({ module: wasm });

const casesText = readFileSync(casesPath, "utf8");
const root = dirname(resolve(casesPath));
const files = {};
for (const c of JSON.parse(casesText).cases) files[c.model] ??= readFileSync(join(root, c.model), "utf8");
const got = JSON.parse(engine.runCases(casesText, JSON.stringify(files)));

if (recordedPath === "-") {
  console.log(JSON.stringify(got, null, 1));
  process.exit(0);
}
if (flag === "--record") {
  const doc = {
    _comment:
      "What the WASM engine answered for verdicts/cases.json: the reference. Written by check-verdicts.mjs --record; " +
      "a change here is a change in engine output and needs a reason in the commit.",
    engine_sha256: sha,
    engine_version: manifest.engine_version,
    pflow_rs_commit: manifest.pflow_rs_commit,
    results: got.results,
  };
  writeFileSync(recordedPath, JSON.stringify(doc, null, 1) + "\n");
  console.log(`check-verdicts: recorded ${Object.keys(got.results).length} results from engine ${sha.slice(0, 16)}`);
  process.exit(0);
}
const want = JSON.parse(readFileSync(recordedPath, "utf8"));
let bad = 0;
const ids = new Set([...Object.keys(want.results), ...Object.keys(got.results)]);
for (const id of ids) {
  const a = JSON.stringify(want.results[id] ?? null);
  const b = JSON.stringify(got.results[id] ?? null);
  if (a !== b) {
    bad++;
    console.error(`check-verdicts: ${id}: recorded ${a}, got ${b}`);
  }
}
if (bad) {
  console.error(`check-verdicts: ${bad} of ${ids.size} differ (engine ${sha.slice(0, 16)}, recorded with ${String(want.engine_sha256).slice(0, 16)})`);
  process.exit(1);
}
console.log(`check-verdicts: ok, ${ids.size} verdicts match (engine ${sha.slice(0, 16)})`);
