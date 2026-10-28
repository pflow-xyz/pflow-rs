// node scripts/crmath/run-wasi.mjs <test.wasm> [args...]: run a cargo test
// binary built for wasm32-wasip1 under Node's WASI (no wasmtime needed).
import { readFile } from "node:fs/promises";
import { WASI } from "node:wasi";
import { argv, exit } from "node:process";

const wasi = new WASI({ version: "preview1", args: ["test", ...argv.slice(3)], env: {}, returnOnExit: true });
const mod = await WebAssembly.compile(await readFile(argv[2]));
const inst = await WebAssembly.instantiate(mod, wasi.getImportObject());
exit(wasi.start(inst));
