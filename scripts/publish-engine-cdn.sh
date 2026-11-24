#!/usr/bin/env bash
# publish-engine-cdn.sh: publish the reproducible engine (scripts/build-wasm-repro.sh) to
# cdn.stackdump.com under its content address, the way pflow-native publishes its wasm
# (pflow-native scripts/publish-wasm.sh), with the build manifest beside it.
#
#   scripts/publish-engine-cdn.sh <outdir>            publish (as a draft: reachable by CID, not in the feed)
#   DRAFT=0 scripts/publish-engine-cdn.sh <outdir>    ... listed in the feed and search
#   DRY_RUN=1 scripts/publish-engine-cdn.sh <outdir>  print the CID and the landing page, write nothing
#
# <outdir> is a build-wasm-repro.sh output. It is refused unless every file's sha256 equals
# crates/pflow-engine/repro.json (the recorded manifest, which gir and workbit both reproduce) and its
# manifest says the tree was clean. Build it on a build host, not here, and copy it back:
#
#   scripts/repro-across-hosts.sh <commit> gir workbit
#   scp -r gir:pflow-engine-repro/out-<commit> target/engine-publish
#   scripts/publish-engine-cdn.sh target/engine-publish
#
# Published, one directory per engine:
#   pflow_engine_bg.wasm       the engine; the CID is bafyrei + the first 32 hex of its sha256
#   pflow_engine_bg.wasm.gz    gzip -9 -n and brotli q11 of the same bytes (nginx does not compress
#   pflow_engine_bg.wasm.br    wasm; a page fetches one and decodes it)
#   pflow_engine.js            the wasm-bindgen glue, which only works with this .wasm
#   pflow_engine.d.ts, pflow_engine_bg.wasm.d.ts
#   meta/manifest.json         the build manifest: commit, toolchain, tools, flags, every sha256. In a
#                              subdirectory because publish/cdn.py takes a top-level .json as the primary
#                              artifact, and the CID must name the engine, not its manifest.
# A page pins the URL and the .wasm's sha256 and refuses bytes that do not hash to it, so the CDN is a
# transport, never trusted for content. Paths under a CID never change; there is no "latest".
#
# The CDN's master is valoper (~/Workspace/cdn-stackdump-com/data/blobs); elsewhere this copies the stage
# there over ssh (CDN_HOST, default valoper.local) and publishes with pflow-xyz's publish/cdn.py
# (PFLOW_XYZ, default ../pflow-xyz), which stages, reindexes and mirrors to pflow.dev.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT=${1:?usage: scripts/publish-engine-cdn.sh <outdir>}
PFLOW_XYZ="${PFLOW_XYZ:-$ROOT/../pflow-xyz}"
CDN_HOST="${CDN_HOST:-valoper.local}"
CDN_TREE="$HOME/Workspace/cdn-stackdump-com/data/blobs"
DRAFT="${DRAFT:-1}"
die() { printf 'publish-engine-cdn: %s\n' "$*" >&2; exit 1; }
sha() { { sha256sum "$1" 2>/dev/null || shasum -a 256 "$1"; } | cut -d' ' -f1; }

[ -f "$OUT/manifest.json" ] || die "no $OUT/manifest.json: not a build-wasm-repro.sh output"
[ -f "$PFLOW_XYZ/publish/cdn.py" ] || die "no $PFLOW_XYZ/publish/cdn.py (set PFLOW_XYZ to a pflow-xyz checkout)"
python3 - "$OUT" "$ROOT/crates/pflow-engine/repro.json" <<'PY' || exit 1
import hashlib, json, os, sys
out, rec = sys.argv[1], json.load(open(sys.argv[2]))
m = json.load(open(os.path.join(out, "manifest.json")))
bad = []
if m.get("dirty") is not False:
    bad.append("the build's tree was not clean (dirty=%r)" % m.get("dirty"))
for f, w in rec["files"].items():
    data = open(os.path.join(out, f), "rb").read()
    got = hashlib.sha256(data).hexdigest()
    if got != w["sha256"]:
        bad.append("%s: sha256 %s, repro.json records %s" % (f, got, w["sha256"]))
if bad:
    sys.exit("publish-engine-cdn: refusing:\n  " + "\n  ".join(bad))
PY

COMMIT=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["pflow_rs_commit"])' "$OUT/manifest.json")
VERSION=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["engine_version"])' "$OUT/manifest.json")
RUSTC=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["toolchain"]["rustc"])' "$OUT/manifest.json")

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
D="$STAGE/pflow-engine"
mkdir -p "$D/meta"
cp "$OUT"/pflow_engine_bg.wasm "$OUT"/pflow_engine.js "$OUT"/pflow_engine.d.ts "$OUT"/pflow_engine_bg.wasm.d.ts "$D/"
cp "$OUT/manifest.json" "$D/meta/manifest.json"
# -n: no name or mtime in the gzip header, so the same .wasm gives the same .gz.
gzip -9 -n -c "$D/pflow_engine_bg.wasm" > "$D/pflow_engine_bg.wasm.gz"
python3 -c 'import sys
try:
    import brotli
except ImportError:
    sys.exit(0)
d = open(sys.argv[1], "rb").read()
open(sys.argv[1] + ".br", "wb").write(brotli.compress(d, quality=11))' "$D/pflow_engine_bg.wasm"
if [ ! -f "$D/pflow_engine_bg.wasm.br" ] && command -v node >/dev/null; then
  node -e 'const z=require("zlib"),fs=require("fs"),[,f]=process.argv,b=fs.readFileSync(f);fs.writeFileSync(f+".br",z.brotliCompressSync(b,{params:{[z.constants.BROTLI_PARAM_QUALITY]:11,[z.constants.BROTLI_PARAM_SIZE_HINT]:b.length}}))' "$D/pflow_engine_bg.wasm"
fi
cp "$PFLOW_XYZ/publish/cdn.py" "$STAGE/cdn.py"

WASM_SHA=$(sha "$D/pflow_engine_bg.wasm")
JS_SHA=$(sha "$D/pflow_engine.js")
GZ_SHA=$(sha "$D/pflow_engine_bg.wasm.gz")
BR_SHA=$([ -f "$D/pflow_engine_bg.wasm.br" ] && sha "$D/pflow_engine_bg.wasm.br" || echo none)
MAN_SHA=$(sha "$D/meta/manifest.json")
CID="bafyrei${WASM_SHA:0:32}"

cat > "$STAGE/body.md" <<EOF
The pflow-rs CID-checked engine (\`crates/pflow-engine\`), version $VERSION, built reproducibly by
\`scripts/build-wasm-repro.sh\` from [pflow-rs ${COMMIT:0:7}](https://github.com/pflow-xyz/pflow-rs/commit/$COMMIT)
with $RUSTC. The same commit gives these bytes on x86_64 Linux and arm64 macOS; rebuild it yourself
and compare with \`meta/manifest.json\`. Code MIT; models and content CC0.

It loads a model by CID: give it the model's canonical JSON-LD bytes and the CID you expect, and it
recomputes the CID (URDNA2015, as pflow.xyz) and refuses a mismatch. Then it runs check, validate,
verify, invariants, incidence, sde, simulate (ODE), ssa, reachability or fire, and answers an envelope
whose \`verdict_sha256\` anyone can re-derive with this engine.

\`\`\`js
const base = "https://cdn.stackdump.com/ipfs/$CID/";
const bytes = await (await fetch(base + "pflow_engine_bg.wasm")).arrayBuffer();
// sha256(bytes) must be $WASM_SHA
const engine = await import(base + "pflow_engine.js");
engine.initSync({ module: bytes });
const model = new engine.Model(modelBytes, modelCid);   // throws on a CID mismatch
const answer = JSON.parse(model.run("ssa", JSON.stringify({ seed: 42 })));
\`\`\`

| file | sha256 |
|---|---|
| pflow_engine_bg.wasm | \`$WASM_SHA\` |
| pflow_engine_bg.wasm.gz | \`$GZ_SHA\` |
| pflow_engine_bg.wasm.br | \`$BR_SHA\` |
| pflow_engine.js | \`$JS_SHA\` |
| meta/manifest.json | \`$MAN_SHA\` |
EOF

ARGS=(pflow-engine --source pflow-rs --schema WasmModule/v1
  --title "pflow-rs CID-checked engine $VERSION (${COMMIT:0:7})"
  --tag wasm --tag pflow-rs --tag reproducible --body body.md --wait
  --meta "commit=$COMMIT" --meta "version=$VERSION" --meta "rustc=$RUSTC"
  --meta "wasm_sha256=$WASM_SHA" --meta "js_sha256=$JS_SHA" --meta "gzip_sha256=$GZ_SHA"
  --meta "brotli_sha256=$BR_SHA" --meta "manifest_sha256=$MAN_SHA" --meta "license=MIT")
[ "$DRAFT" = 1 ] && ARGS+=(--draft)
[ "${DRY_RUN:-0}" = 1 ] && ARGS+=(--dry-run)

run_cdn() { python3 cdn.py "$@" | tee /dev/stderr | grep -q "$CID" || die "cdn.py did not address the engine as $CID (did a top-level .json become the primary?)"; }
if [ -d "$CDN_TREE" ]; then
  (cd "$STAGE" && run_cdn "${ARGS[@]}")
else
  REMOTE="$(ssh "$CDN_HOST" mktemp -d)"
  scp -q -r "$STAGE/." "$CDN_HOST:$REMOTE/"
  ssh "$CDN_HOST" "cd $REMOTE && python3 cdn.py $(printf '%q ' "${ARGS[@]}"); s=\$?; rm -rf $REMOTE; exit \$s" | tee /dev/stderr | grep -q "$CID" \
    || die "cdn.py did not address the engine as $CID"
fi

[ "${DRY_RUN:-0}" = 1 ] && exit 0
# What a page pins.
printf '{"url": "https://cdn.stackdump.com/ipfs/%s/", "commit": "%s", "version": "%s", "wasm_sha256": "%s", "js_sha256": "%s", "manifest_sha256": "%s"}\n' \
  "$CID" "$COMMIT" "$VERSION" "$WASM_SHA" "$JS_SHA" "$MAN_SHA"
