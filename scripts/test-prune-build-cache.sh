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

# Without the override: cargo metadata's external build_directory is pruned, target/ is not.
PATH="$HOME/.cargo/bin:$PATH"
ws="$t/ws"
mkdir -p "$ws/src" "$ws/scripts"
printf '[package]\nname = "p"\nversion = "0.0.0"\nedition = "2021"\n' >"$ws/Cargo.toml"
: >"$ws/src/lib.rs"
cp "$here/prune-build-cache.sh" "$ws/scripts/"
export CARGO_BUILD_BUILD_DIR="$t/bd/{workspace-path-hash}"
bd=$(cd "$ws" && cargo metadata --format-version 1 --no-deps | jq -r .build_directory)
tinc="$ws/target/debug/incremental"
mkdir -p "$bd/debug/incremental/c-1/s-old" "$bd/debug/incremental/c-1/s-new" "$tinc/c-1/s-old" "$tinc/c-1/s-new"
touch -t 202001010000 "$bd/debug/incremental/c-1/s-old" "$tinc/c-1/s-old"
touch -t 202001020000 "$bd/debug/incremental/c-1/s-new" "$tinc/c-1/s-new"
env -u CARGO_TARGET_DIR -u FASTEDIT_PRUNE_TARGET "$ws/scripts/prune-build-cache.sh" --older-than 60 >/dev/null
[ ! -d "$bd/debug/incremental/c-1/s-old" ] && [ -d "$bd/debug/incremental/c-1/s-new" ] || { echo "FAIL: external build-dir not pruned"; exit 1; }
[ -d "$tinc/c-1/s-old" ] || { echo "FAIL: target/ touched though build-dir is elsewhere"; exit 1; }
echo ok
