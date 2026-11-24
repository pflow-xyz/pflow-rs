#!/usr/bin/env bash
# build-wasm-repro.sh: build the CID-checked engine (crates/pflow-engine) as one
# reproducible wasm32 module. The same pflow-rs commit gives the same bytes on
# any host (x86_64 Linux and arm64 macOS are checked: scripts/repro-across-hosts.sh).
#
#   scripts/build-wasm-repro.sh <outdir>
#
# writes  pflow_engine_bg.wasm  pflow_engine.js  pflow_engine.d.ts  pflow_engine_bg.wasm.d.ts
#         manifest.json  {engine, engine_version, pflow_rs_commit, toolchain, tools, flags, files: {name: {sha256, size}}}
#
# What is pinned, and where:
#   rustc         crates/pflow-engine/rust-toolchain.toml (exact; rustup installs it). cargo runs from that
#                 directory so rustup uses it, with --locked against the committed Cargo.lock.
#   wasm-bindgen  the lockfile's version; the CLI must equal it (cargo install wasm-bindgen-cli --version V --locked).
#                 Looked for as $WASM_BINDGEN, ~/.local/opt/wasm-bindgen-V/bin, then PATH.
#   wasm-opt      binaryen BINARYEN below, the GitHub release tarball for this platform, sha256-pinned here,
#                 downloaded once into $PFLOW_TOOLS (default ~/.cache/pflow-engine-tools).
#   profile       [profile.engine-repro] in the workspace Cargo.toml: opt-level z, fat LTO, codegen-units 1,
#                 panic abort, strip, no debuginfo, no incremental.
#   paths         --remap-path-prefix maps the checkout, the target dir, CARGO_HOME and the rust sysroot to
#                 fixed names, so no host path reaches a panic string. No crate in the engine's tree has a
#                 build.rs that embeds a path or a time (checked: cargo tree, below).
#   time          SOURCE_DATE_EPOCH is the commit's time; nothing in the build reads the clock.
#
# The manifest is itself deterministic (no host name, no build time): two hosts that agree write the same
# manifest.json, byte for byte. scripts/check-wasm-repro.sh rebuilds and compares against the recorded
# crates/pflow-engine/repro.json.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd -P)
out=${1:?usage: scripts/build-wasm-repro.sh <outdir>}
mkdir -p "$out"
out=$(cd "$out" && pwd -P)

BINARYEN=132
crate=crates/pflow-engine
die() { echo "build-wasm-repro: $*" >&2; exit 2; }

# --- the source ---------------------------------------------------------------
if git rev-parse --git-dir >/dev/null 2>&1; then
  commit=$(git rev-parse HEAD)
  epoch=$(git log -1 --format=%ct HEAD)
  if [ -n "$(git status --porcelain --untracked-files=no)" ]; then dirty=true; else dirty=false; fi
else
  commit=${PFLOW_RS_COMMIT:?not a git checkout: set PFLOW_RS_COMMIT and SOURCE_DATE_EPOCH}
  epoch=${SOURCE_DATE_EPOCH:?}
  dirty=unknown
fi
export SOURCE_DATE_EPOCH=$epoch

# --- rustc, from the crate's toolchain file -----------------------------------
channel=$(sed -nE 's/^channel = "([^"]+)"/\1/p' "$crate/rust-toolchain.toml")
[ -n "$channel" ] || die "no channel in $crate/rust-toolchain.toml"
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
command -v rustup >/dev/null || die "rustup not found (the toolchain file needs it)"
rustc_v=$(cd "$crate" && rustc --version)
case "$rustc_v" in "rustc $channel "*) ;; *) die "rustc is '$rustc_v', the toolchain file pins $channel";; esac
cargo_v=$(cd "$crate" && cargo --version)
sysroot=$(cd "$crate" && rustc --print sysroot)
(cd "$crate" && rustup target list --installed | grep -qx wasm32-unknown-unknown) \
  || (cd "$crate" && rustup target add wasm32-unknown-unknown)

# --- wasm-bindgen, the lockfile's ----------------------------------------------
want_wb=$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/[^0-9.]/,"",$0); print; exit}' Cargo.lock)
[ -n "$want_wb" ] || die "Cargo.lock has no wasm-bindgen"
wb=${WASM_BINDGEN:-}
if [ -z "$wb" ] && [ -x "$HOME/.local/opt/wasm-bindgen-$want_wb/bin/wasm-bindgen" ]; then wb=$HOME/.local/opt/wasm-bindgen-$want_wb/bin/wasm-bindgen; fi
if [ -z "$wb" ]; then wb=$(command -v wasm-bindgen || true); fi
[ -n "$wb" ] || die "wasm-bindgen CLI not found (cargo install wasm-bindgen-cli --version $want_wb --locked --root ~/.local/opt/wasm-bindgen-$want_wb)"
have_wb=$("$wb" --version | awk '{print $2}')
[ "$have_wb" = "$want_wb" ] || die "the lockfile pins wasm-bindgen $want_wb but $wb is $have_wb (cargo install wasm-bindgen-cli --version $want_wb --locked --root ~/.local/opt/wasm-bindgen-$want_wb)"

# --- wasm-opt, binaryen's release tarball, sha256-pinned ------------------------
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)           plat=x86_64-linux;  sum=195ddc94f9bc89f45abdabb0b9eea86023d727ba90eac8b35b80f2544fc30572 ;;
  Linux-aarch64)          plat=aarch64-linux; sum=c58562417836c5d0493d89bdefc434933bdc097db641b483df86bcfa557a107f ;;
  Darwin-arm64)           plat=arm64-macos;   sum=98aad827847af7ef990ed7098d885725c8e5b5aae75073403635617ae4e259aa ;;
  *) die "no pinned binaryen for $(uname -s)-$(uname -m)";;
esac
tools=${PFLOW_TOOLS:-$HOME/.cache/pflow-engine-tools}
bdir=$tools/binaryen-version_$BINARYEN-$plat
if [ ! -x "$bdir/bin/wasm-opt" ]; then
  mkdir -p "$tools"
  tgz=$tools/binaryen-version_$BINARYEN-$plat.tar.gz
  curl -fsSL -o "$tgz" "https://github.com/WebAssembly/binaryen/releases/download/version_$BINARYEN/binaryen-version_$BINARYEN-$plat.tar.gz"
  got=$( (sha256sum "$tgz" 2>/dev/null || shasum -a 256 "$tgz") | awk '{print $1}')
  [ "$got" = "$sum" ] || { rm -f "$tgz"; die "binaryen tarball sha256 $got, pinned $sum"; }
  rm -rf "$bdir.tmp" && mkdir -p "$bdir.tmp"
  tar -xzf "$tgz" -C "$bdir.tmp"
  mv "$bdir.tmp/binaryen-version_$BINARYEN" "$bdir" && rm -rf "$bdir.tmp" "$tgz"
fi
wasm_opt=$bdir/bin/wasm-opt
wasm_opt_v=$("$wasm_opt" --version)

# --- the build ------------------------------------------------------------------
target=${PFLOW_ENGINE_TARGET_DIR:-$root/target/engine-repro}
mkdir -p "$target"
target=$(cd "$target" && pwd -P)
cargo_home=$(cd "${CARGO_HOME:-$HOME/.cargo}" && pwd -P)
# rustc applies the last matching remap: the more specific prefixes come after the ones that contain them.
remaps="--remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$sysroot=/rust --remap-path-prefix=$root=/pflow-rs --remap-path-prefix=$target=/target"
unset RUSTC_WRAPPER CARGO_BUILD_RUSTC_WRAPPER CARGO_ENCODED_RUSTFLAGS CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS RUSTDOCFLAGS
export RUSTFLAGS="$remaps"
export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$target"
# No build script in the engine's tree may embed a path or a clock: list the crates that have one.
tree=$(cd "$crate" && cargo tree --locked -q -p pflow-engine --target wasm32-unknown-unknown -e normal --prefix none | awk '{print $1}' | sort -u | tr '\n' ' ')
build_rs=$(cd "$crate" && cargo metadata --locked -q --format-version 1 | python3 -c '
import json, sys
tree = set(sys.argv[1].split())
d = json.load(sys.stdin)
print(" ".join(sorted({p["name"] for p in d["packages"] if p["name"] in tree
                       and any("custom-build" in t["kind"] for t in p["targets"])})))' "$tree")
(cd "$crate" && cargo build --locked --lib --profile engine-repro --target wasm32-unknown-unknown -p pflow-engine)
raw=$target/wasm32-unknown-unknown/engine-repro/pflow_engine.wasm

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
"$wb" --target web --out-dir "$tmp" --out-name pflow_engine --remove-name-section --remove-producers-section "$raw"
features="--enable-bulk-memory --enable-mutable-globals --enable-nontrapping-float-to-int --enable-reference-types --enable-sign-ext --enable-multivalue"
# shellcheck disable=SC2086
"$wasm_opt" -Oz $features --strip-debug --strip-producers "$tmp/pflow_engine_bg.wasm" -o "$tmp/pflow_engine_bg.opt.wasm"
mv "$tmp/pflow_engine_bg.opt.wasm" "$tmp/pflow_engine_bg.wasm"

files="pflow_engine_bg.wasm pflow_engine.js pflow_engine.d.ts pflow_engine_bg.wasm.d.ts"
for f in $files; do cp "$tmp/$f" "$out/$f"; done

version=$(awk '/^\[workspace.package\]/{w=1; next} /^\[/{w=0} w && /^version = /{gsub(/"/,"",$3); print $3; exit}' Cargo.toml)
python3 - "$out" "$version" "$commit" "$dirty" "$epoch" "$rustc_v" "$cargo_v" "$have_wb" "$wasm_opt_v" "$BINARYEN" "$build_rs" "$features" $files <<'PY'
import hashlib, json, os, sys
out, version, commit, dirty, epoch, rustc, cargo, wb, wo, bver, build_rs, features, *files = sys.argv[1:]
doc = {
    "engine": "pflow-engine",
    "engine_version": version,
    "pflow_rs_commit": commit,
    "dirty": {"true": True, "false": False}.get(dirty, dirty),
    "source_date_epoch": int(epoch),
    "toolchain": {"rustc": rustc, "cargo": cargo, "target": "wasm32-unknown-unknown",
                  "file": "crates/pflow-engine/rust-toolchain.toml"},
    "tools": {"wasm_bindgen": wb, "wasm_opt": wo, "binaryen_release": "version_" + bver},
    "flags": {
        "cargo": "cargo build --locked --lib --profile engine-repro --target wasm32-unknown-unknown -p pflow-engine",
        "profile": {"opt-level": "z", "lto": "fat", "codegen-units": 1, "panic": "abort", "strip": True,
                    "debug": False, "incremental": False},
        "rustflags": "--remap-path-prefix=<CARGO_HOME>=/cargo --remap-path-prefix=<sysroot>=/rust "
                     "--remap-path-prefix=<checkout>=/pflow-rs --remap-path-prefix=<target>=/target",
        "wasm_bindgen": "--target web --out-name pflow_engine --remove-name-section --remove-producers-section",
        "wasm_opt": "-Oz " + features + " --strip-debug --strip-producers",
        "build_scripts_in_tree": build_rs.split(),
    },
    "files": {},
}
for f in files:
    data = open(os.path.join(out, f), "rb").read()
    doc["files"][f] = {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}
doc["engine_sha256"] = doc["files"]["pflow_engine_bg.wasm"]["sha256"]
with open(os.path.join(out, "manifest.json"), "w") as fh:
    json.dump(doc, fh, indent=2, sort_keys=True)
    fh.write("\n")
print("build-wasm-repro: engine sha256 %s (%d bytes), commit %s%s" % (
    doc["engine_sha256"], doc["files"]["pflow_engine_bg.wasm"]["size"], commit[:12], "" if dirty == "false" else " (dirty: %s)" % dirty))
PY
