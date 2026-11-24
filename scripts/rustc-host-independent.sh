#!/usr/bin/env bash
# rustc-host-independent.sh: the RUSTC_WRAPPER scripts/build-wasm-repro.sh builds with.
#
# Cargo names each crate it compiles with `-C metadata=<hash>`, and the hash folds in every
# dependency's hash. A proc-macro or a build script is compiled for the *host*, so its hash differs
# between an x86_64 Linux and an arm64 macOS build host, and so does the hash of every wasm32 crate
# downstream of it (serde, serde_json, every pflow crate). That hash is the crate's StableCrateId:
# it reaches symbol names, which order the module's functions and data, so the same source gave
# different engine bytes on gir and workbit (2026-11-23; memchr, with no host-built dependency,
# had the same hash on both).
#
# For a wasm32 unit this wrapper replaces the hash with one computed from what is the same on every
# host: the crate name, its source path relative to the package (registry crates keep their
# name-version directory), the crate type, the edition and the --cfg flags. `-C extra-filename`
# (cargo's file names) is left alone. Host units are passed through untouched.
set -euo pipefail
rustc=$1; shift
args=("$@")
wasm=0
for a in "${args[@]}"; do [ "$a" = wasm32-unknown-unknown ] && wasm=1; done
if [ $wasm = 0 ]; then exec "$rustc" "${args[@]}"; fi
key=""
i=0; n=${#args[@]}; meta_at=-1
while [ $i -lt $n ]; do
  a=${args[$i]}
  case "$a" in
    --crate-name|--crate-type|--edition|--cfg)
      key="$key $a=${args[$((i+1))]}"; i=$((i+2)); continue ;;
    -C)
      case "${args[$((i+1))]}" in metadata=*) meta_at=$((i+1)) ;; esac
      i=$((i+2)); continue ;;
    *.rs)
      src=$a
      case "$src" in
        */registry/src/*) src=${src#*/registry/src/}; src=${src#*/} ;;   # <name>-<version>/src/lib.rs
        */git/checkouts/*) src=${src#*/git/checkouts/} ;;
        *) src=${src#"${PFLOW_REPRO_ROOT:-$PWD}"/} ;;                      # crates/<crate>/src/lib.rs
      esac
      key="$key src=$src" ;;
  esac
  i=$((i+1))
done
if [ $meta_at -ge 0 ]; then
  h=$(printf '%s' "$key" | { sha256sum 2>/dev/null || shasum -a 256; } | cut -c1-16)
  args[$meta_at]="metadata=$h"
fi
exec "$rustc" "${args[@]}"
