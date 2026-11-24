#!/usr/bin/env bash
# repro-across-hosts.sh: build one pflow-rs commit, fetched from public GitHub, on several hosts,
# and require the same engine bytes from each.
#
#   scripts/repro-across-hosts.sh [commit] [host ...]     defaults: HEAD (must be pushed), gir workbit
#
# Each host clones https://github.com/pflow-xyz/pflow-rs into ~/pflow-engine-repro/pflow-rs (once),
# checks the commit out detached, and runs scripts/check-wasm-repro.sh there (CHECK=0: only
# build-wasm-repro.sh). Each host's manifest.json is copied back to target/repro-hosts/<host>.json and
# the files' sha256 compared. Exit 0 only when every host agrees (and, with CHECK=1, passes its check).
set -euo pipefail
cd "$(dirname "$0")/.."
commit=$(git rev-parse "${1:-HEAD}")
shift || true
hosts=("$@"); [ ${#hosts[@]} -gt 0 ] || hosts=(gir workbit)
mkdir -p target/repro-hosts
url=https://github.com/pflow-xyz/pflow-rs
for h in "${hosts[@]}"; do
  echo "== $h: $commit"
  ssh "$h" "set -e; [ -f ~/.cargo/env ] && . ~/.cargo/env; export PATH=\$HOME/.cargo/bin:/opt/homebrew/bin:\$PATH
    mkdir -p ~/pflow-engine-repro; cd ~/pflow-engine-repro
    [ -d pflow-rs/.git ] || git clone -q $url pflow-rs
    cd pflow-rs && git fetch -q origin $commit && git checkout -q --detach $commit
    if [ '${CHECK:-1}' = 1 ]; then scripts/check-wasm-repro.sh; fi
    scripts/build-wasm-repro.sh ~/pflow-engine-repro/out-$commit >/dev/null"
  scp -q "$h:pflow-engine-repro/out-$commit/manifest.json" "target/repro-hosts/$h.json"
done
python3 - "${hosts[@]}" <<'PY'
import json, sys
docs = {h: json.load(open("target/repro-hosts/%s.json" % h)) for h in sys.argv[1:]}
ref = next(iter(docs.values()))
ok = True
for h, d in docs.items():
    same = d["files"] == ref["files"]
    ok &= same
    print("%-10s engine %s  %s" % (h, d["engine_sha256"], "same" if same else "DIFFERS"))
print("repro-across-hosts: %s" % ("all hosts agree" if ok else "hosts disagree"))
sys.exit(0 if ok else 1)
PY
