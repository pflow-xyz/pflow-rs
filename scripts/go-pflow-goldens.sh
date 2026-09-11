#!/usr/bin/env bash
#
# go-pflow-goldens.sh — keep every golden copied byte-identical from
# go-pflow honest, ecosystem-wide, one lock for all of it (ROADMAP.md
# ground rule 2). Same shape as scripts/docs-sync.sh (prose) and
# bitwrap-io/stackedup-gg's pflow-js.sh (shared browser JS): a lock file
# records a sha256 per vendored golden plus the go-pflow commit it came
# from.
#
#   ./scripts/go-pflow-goldens.sh check   verify every golden still matches
#                                         the sha256 in go-pflow.lock.
#                                         Offline, no go-pflow checkout
#                                         needed.
#   ./scripts/go-pflow-goldens.sh sync    re-copy every golden from a
#                                         go-pflow checkout and rewrite the
#                                         lock. Point at it with GO_PFLOW=;
#                                         defaults to ../go-pflow. Refuses a
#                                         dirty go-pflow checkout — goldens
#                                         are vendored from a committed
#                                         state, never from a dirty tree
#                                         (the SSA vendoring was once done
#                                         from a dirty tree and had to be
#                                         redone).
#   ./scripts/go-pflow-goldens.sh status  report staleness against a
#                                         go-pflow checkout without changing
#                                         anything.
#
# A failing 'check' is never fixed by re-running 'sync' to make it pass —
# see ROADMAP.md ground rule 1. Re-sync only after a deliberate,
# understood change in go-pflow.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCK="$REPO_ROOT/go-pflow.lock"
GO_PFLOW="${GO_PFLOW:-$REPO_ROOT/../go-pflow}"

die() { printf '%s\n' "$*" >&2; exit 1; }
sha() { sha256sum "$1" | cut -d' ' -f1; }

[[ -f "$LOCK" ]] || die "go-pflow-goldens.sh: no $LOCK"
rows() { grep -vE '^\s*(#|$)' "$LOCK"; }

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
    [[ $fail -eq 0 ]] && echo "go-pflow-goldens: all vendored goldens match go-pflow.lock"
    exit $fail
    ;;
status)
    [[ -d "$GO_PFLOW" ]] || die "go-pflow-goldens.sh: no go-pflow checkout at $GO_PFLOW"
    while read -r want_sha local_path upstream_path; do
        up="$GO_PFLOW/$upstream_path"
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
    [[ -d "$GO_PFLOW" ]] || die "go-pflow-goldens.sh: no go-pflow checkout at $GO_PFLOW"
    git -C "$GO_PFLOW" diff --quiet && git -C "$GO_PFLOW" diff --cached --quiet \
        || die "go-pflow-goldens.sh: $GO_PFLOW has uncommitted changes; goldens are vendored from a committed state only"
    commit="$(git -C "$GO_PFLOW" rev-parse HEAD)"
    tmp="$(mktemp)"
    {
        echo "# go-pflow.lock — goldens copied byte-identical from go-pflow."
        echo "#"
        echo "# DO NOT HAND-EDIT A GOLDEN FILE. Regenerate it in go-pflow and re-run"
        echo "# ./scripts/go-pflow-goldens.sh sync. 'check' fails the build otherwise."
        echo "# A failing golden is a bug in Rust or a deliberate change in Go; it is"
        echo "# never fixed by regenerating (ROADMAP.md ground rule 1)."
        echo "#"
        echo "# source: github.com/pflow-xyz/go-pflow @ ${commit}"
        echo "#"
        echo "# <sha256>  <path in this repo>  <path in go-pflow>"
    } >"$tmp"
    while read -r _ local_path upstream_path; do
        up="$GO_PFLOW/$upstream_path"
        [[ -f "$up" ]] || die "go-pflow-goldens.sh: upstream $upstream_path not found"
        mkdir -p "$(dirname "$REPO_ROOT/$local_path")"
        cp "$up" "$REPO_ROOT/$local_path"
        echo "$(sha "$up")  $local_path  $upstream_path" >>"$tmp"
    done < <(rows)
    mv "$tmp" "$LOCK"
    echo "go-pflow-goldens: synced from go-pflow @ ${commit}"
    ;;
*)
    die "go-pflow-goldens.sh: unknown command '$cmd' (check|sync|status)"
    ;;
esac
