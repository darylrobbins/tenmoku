#!/bin/sh
# Fixture test for prune-build-cache.sh: stale sessions go, the newest session dir stays
# even when a newer .lock file exists, and a symlinked crate dir is left alone.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
inc="$t/target/debug/incremental"
mkdir -p "$inc/a-1" "$inc/b-1" "$t/outside/s-old"
mkdir "$inc/a-1/s-old1" "$inc/a-1/s-old2" "$inc/a-1/s-new" "$inc/b-1/s-only"
ln -s "$t/outside" "$inc/link-1"
touch -t 202001010000 "$inc/a-1/s-old1" "$inc/a-1/s-old2" "$inc/b-1/s-only" "$t/outside/s-old"
touch -t 202001020000 "$inc/a-1/s-new"
touch "$inc/a-1/s-new.lock" "$inc/b-1/s-only.lock"
run() { FASTEDIT_PRUNE_TARGET="$t/target" "$here/prune-build-cache.sh" "$@" >/dev/null; }
run --dry-run --older-than 60
[ -d "$inc/a-1/s-old1" ] || { echo "FAIL: dry run deleted"; exit 1; }
run --older-than 60
[ ! -d "$inc/a-1/s-old1" ] && [ ! -d "$inc/a-1/s-old2" ] || { echo "FAIL: stale kept"; exit 1; }
[ -d "$inc/a-1/s-new" ] && [ -d "$inc/b-1/s-only" ] || { echo "FAIL: newest deleted"; exit 1; }
[ -d "$t/outside/s-old" ] || { echo "FAIL: symlink target touched"; exit 1; }
echo ok
