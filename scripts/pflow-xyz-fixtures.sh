#!/usr/bin/env bash
#
# pflow-xyz-fixtures.sh — keep every file copied byte-identical from
# pflow-xyz honest: the showcase inputs the parity tests replay
# (cafe.bundle.json, cafe-order.json, event-log.json), the cross-language
# CID contract (parity/golden.json + parity/fixtures/*.jsonld), and the
# go-pflow-generated ODE and learn goldens pflow-xyz hosts
# (parity/ode/*.json, parity/learn/goldens.json). Same shape as
# scripts/go-pflow-goldens.sh and scripts/docs-sync.sh: a lock file records a
# sha256 per vendored file plus the pflow-xyz commit it came from.
#
# These used to be read from a sibling ../pflow-xyz checkout, and the tests
# reading them printed "skip" and passed when it was missing — which it is
# in CI. Vendoring them makes those tests run everywhere; a missing file is
# a test failure, not a skip.
#
#   ./scripts/pflow-xyz-fixtures.sh check    verify every vendored file still
#                                            matches the sha256 in
#                                            pflow-xyz.lock. Offline, no
#                                            pflow-xyz checkout needed.
#   ./scripts/pflow-xyz-fixtures.sh verify   provenance check: a pflow-xyz
#                                            checkout (PFLOW_XYZ=) must be at
#                                            exactly the locked commit and
#                                            every upstream file must hash to
#                                            its locked sha256. Fails
#                                            otherwise. CI runs this against
#                                            a fresh clone of the public repo.
#   ./scripts/pflow-xyz-fixtures.sh sync     re-copy every file from a
#                                            pflow-xyz checkout and rewrite
#                                            the lock. Point at it with
#                                            PFLOW_XYZ=; defaults to
#                                            ../pflow-xyz. Refuses a dirty
#                                            checkout — files are vendored
#                                            from a committed state only.
#   ./scripts/pflow-xyz-fixtures.sh status   report staleness against a
#                                            pflow-xyz checkout without
#                                            changing anything.
#
# A failing 'check' (or a failing parity test) is never fixed by re-running
# 'sync' to make it pass — see ROADMAP.md ground rule 1. Re-sync only after
# a deliberate, understood change in pflow-xyz.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCK="$REPO_ROOT/pflow-xyz.lock"
PFLOW_XYZ="${PFLOW_XYZ:-$REPO_ROOT/../pflow-xyz}"

die() { printf '%s\n' "$*" >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then
    sha() { sha256sum "$1" | cut -d' ' -f1; }
else
    sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi

[[ -f "$LOCK" ]] || die "pflow-xyz-fixtures.sh: no $LOCK"
rows() { grep -vE '^\s*(#|$)' "$LOCK"; }
locked_commit() {
    sed -nE 's|^# source: github.com/pflow-xyz/pflow-xyz @ ([0-9a-f]{40})$|\1|p' "$LOCK"
}

cmd="${1:-check}"

case "$cmd" in
check)
    fail=0
    while read -r want_sha local_path upstream_path; do
        f="$REPO_ROOT/$local_path"
        [[ -f "$f" ]] || { echo "MISSING $local_path"; fail=1; continue; }
        got_sha="$(sha "$f")"
        if [[ "$got_sha" != "$want_sha" ]]; then
            echo "STALE $local_path (want $want_sha, got $got_sha)"
            fail=1
        fi
    done < <(rows)
    [[ $fail -eq 0 ]] && echo "pflow-xyz-fixtures: all vendored files match pflow-xyz.lock"
    exit $fail
    ;;
verify)
    [[ -d "$PFLOW_XYZ" ]] || die "pflow-xyz-fixtures.sh: no pflow-xyz checkout at $PFLOW_XYZ"
    want_commit="$(locked_commit)"
    [[ -n "$want_commit" ]] || die "pflow-xyz-fixtures.sh: no '# source: ... @ <40-hex commit>' line in $LOCK"
    got_commit="$(git -C "$PFLOW_XYZ" rev-parse HEAD)"
    [[ "$got_commit" == "$want_commit" ]] \
        || die "pflow-xyz-fixtures.sh: $PFLOW_XYZ is at $got_commit, lock pins $want_commit"
    fail=0
    while read -r want_sha local_path upstream_path; do
        up="$PFLOW_XYZ/$upstream_path"
        [[ -f "$up" ]] || { echo "UPSTREAM-MISSING $upstream_path (for $local_path)"; fail=1; continue; }
        up_sha="$(sha "$up")"
        if [[ "$up_sha" != "$want_sha" ]]; then
            echo "UPSTREAM-MISMATCH $upstream_path @ $want_commit is $up_sha, lock says $want_sha"
            fail=1
        fi
    done < <(rows)
    [[ $fail -eq 0 ]] && echo "pflow-xyz-fixtures: every locked file is byte-identical to pflow-xyz @ $want_commit"
    exit $fail
    ;;
status)
    [[ -d "$PFLOW_XYZ" ]] || die "pflow-xyz-fixtures.sh: no pflow-xyz checkout at $PFLOW_XYZ"
    while read -r want_sha local_path upstream_path; do
        up="$PFLOW_XYZ/$upstream_path"
        [[ -f "$up" ]] || { echo "$local_path: upstream $upstream_path missing"; continue; }
        up_sha="$(sha "$up")"
        if [[ "$up_sha" == "$want_sha" ]]; then
            echo "$local_path: in sync"
        else
            echo "$local_path: STALE — upstream is $up_sha, locked at $want_sha"
        fi
    done < <(rows)
    ;;
sync)
    [[ -d "$PFLOW_XYZ" ]] || die "pflow-xyz-fixtures.sh: no pflow-xyz checkout at $PFLOW_XYZ"
    git -C "$PFLOW_XYZ" diff --quiet && git -C "$PFLOW_XYZ" diff --cached --quiet \
        || die "pflow-xyz-fixtures.sh: $PFLOW_XYZ has uncommitted changes; files are vendored from a committed state only"
    commit="$(git -C "$PFLOW_XYZ" rev-parse HEAD)"
    tmp="$(mktemp)"
    {
        echo "# pflow-xyz.lock — files copied byte-identical from pflow-xyz."
        echo "#"
        echo "# DO NOT HAND-EDIT A VENDORED FILE. Change it in pflow-xyz and re-run"
        echo "# ./scripts/pflow-xyz-fixtures.sh sync. 'check' fails the build otherwise."
        echo "# A failing parity test is a bug in Rust or a deliberate change upstream;"
        echo "# it is never fixed by re-syncing (ROADMAP.md ground rule 1)."
        echo "#"
        echo "# source: github.com/pflow-xyz/pflow-xyz @ ${commit}"
        echo "#"
        echo "# <sha256>  <path in this repo>  <path in pflow-xyz>"
    } >"$tmp"
    while read -r _ local_path upstream_path; do
        up="$PFLOW_XYZ/$upstream_path"
        [[ -f "$up" ]] || die "pflow-xyz-fixtures.sh: upstream $upstream_path not found"
        mkdir -p "$(dirname "$REPO_ROOT/$local_path")"
        cp "$up" "$REPO_ROOT/$local_path"
        echo "$(sha "$up")  $local_path  $upstream_path" >>"$tmp"
    done < <(rows)
    mv "$tmp" "$LOCK"
    echo "pflow-xyz-fixtures: synced from pflow-xyz @ ${commit}"
    ;;
*)
    die "pflow-xyz-fixtures.sh: unknown command '$cmd' (check|verify|sync|status)"
    ;;
esac
