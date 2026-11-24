#!/usr/bin/env bash
# check-wasm-repro.sh: rebuild the engine and hold it to the recorded manifest, then hold its
# answers to the recorded verdicts.
#
#   scripts/check-wasm-repro.sh [manifest.json]     default crates/pflow-engine/repro.json
#
# 1. scripts/build-wasm-repro.sh into a scratch dir; every file's sha256 must equal the manifest's,
#    and the toolchain and tool versions must too (a different rustc is a different engine).
# 2. node crates/pflow-engine/js/check-verdicts.mjs: the rebuilt module, run over
#    verdicts/cases.json, must answer verdicts/recorded.json exactly.
# 3. cargo test -p pflow-engine (native; SKIP_NATIVE=1 skips it): the same cases through the
#    native engine, exact except the ODE (see crates/pflow-engine/tests/verdicts.rs).
# Exit 0 only when all three hold. Run it on each of Matt's build hosts (gir, workbit) as the CI
# for the engine; scripts/repro-across-hosts.sh runs it on several at one commit.
set -euo pipefail
cd "$(dirname "$0")/.."
manifest=${1:-crates/pflow-engine/repro.json}
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
scripts/build-wasm-repro.sh "$out"

python3 - "$manifest" "$out/manifest.json" <<'PY'
import json, sys
want, got = (json.load(open(p)) for p in sys.argv[1:3])
bad = []
for k in ("toolchain", "tools", "flags"):
    if want.get(k) != got.get(k):
        bad.append("%s: recorded %s, now %s" % (k, json.dumps(want.get(k), sort_keys=True), json.dumps(got.get(k), sort_keys=True)))
for f, w in want["files"].items():
    g = got["files"].get(f)
    if g != w:
        bad.append("%s: recorded %s, rebuilt %s" % (f, w, g))
if bad:
    print("check-wasm-repro: the rebuild differs from %s (recorded at %s):" % (sys.argv[1], want.get("pflow_rs_commit")), file=sys.stderr)
    for b in bad: print("  " + b, file=sys.stderr)
    sys.exit(1)
print("check-wasm-repro: rebuild matches %s: engine %s" % (sys.argv[1], got["engine_sha256"]))
PY

node=${NODE:-$(command -v node || true)}
if [ -z "$node" ]; then
  for d in "$HOME"/.local/opt/node-*/bin "$HOME"/.nvm/versions/node/*/bin /opt/homebrew/bin; do
    if [ -x "$d/node" ]; then node=$d/node; fi
  done
fi
[ -n "$node" ] || { echo "check-wasm-repro: node not found (set NODE)" >&2; exit 2; }
"$node" crates/pflow-engine/js/check-verdicts.mjs "$out" crates/pflow-engine/verdicts/cases.json crates/pflow-engine/verdicts/recorded.json

if [ "${SKIP_NATIVE:-}" != 1 ]; then
  (cd crates/pflow-engine && cargo test --locked -q -p pflow-engine)
fi
echo "check-wasm-repro: ok"
