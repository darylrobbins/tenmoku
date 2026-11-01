#!/usr/bin/env bash
# Fixture test for prune-build-dirs.sh against a fake root: a live build-dir stays, an orphan
# goes, a recently touched orphan stays, --dry-run deletes nothing, no configured build-dir and
# unsafe roots do nothing.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
t=$(cd -P "$(mktemp -d)" && pwd -P)
trap 'chmod -R u+rwx "$t" 2>/dev/null; rm -rf "$t"' EXIT
root="$t/cargo"
mkdir -p "$root/aa/live" "$root/bb/orphan" "$root/cc/fresh" "$root/dd/linked" "$t/outside/keep"
echo x >"$root/aa/live/f"; echo x >"$root/bb/orphan/f"; echo x >"$root/cc/fresh/f"; echo x >"$t/outside/keep/f"
ln -s "$t/outside" "$root/dd/linked/link"
touch -t 202001010000 "$root/aa/live/f" "$root/aa/live" "$root/bb/orphan/f" "$root/bb/orphan" "$root/dd/linked" "$root/dd" "$root/bb" "$root/aa"
touch -h -t 202001010000 "$root/dd/linked/link"
printf '%s\n' "$root/aa/live" >"$t/live"
run() { FASTEDIT_BUILD_DIR="$root/aa/live" FASTEDIT_LIVE_BUILD_DIRS="$t/live" "$here/prune-build-dirs.sh" "$@" >/dev/null; }

run --dry-run
[ -d "$root/bb/orphan" ] || { echo "FAIL: dry run deleted"; exit 1; }
run
[ -d "$root/aa/live" ] || { echo "FAIL: live build-dir deleted"; exit 1; }
[ ! -e "$root/bb/orphan" ] || { echo "FAIL: orphan kept"; exit 1; }
[ -d "$root/cc/fresh" ] || { echo "FAIL: recently touched build-dir deleted"; exit 1; }
[ ! -e "$root/dd/linked" ] && [ -f "$t/outside/keep/f" ] || { echo "FAIL: symlink followed or orphan kept"; exit 1; }

mkdir -p "$root/ee/orphan2"; touch -t 202001010000 "$root/ee/orphan2" "$root/ee"
FASTEDIT_BUILD_DIR= FASTEDIT_LIVE_BUILD_DIRS="$t/live" "$here/prune-build-dirs.sh" >/dev/null
[ -d "$root/ee/orphan2" ] || { echo "FAIL: swept with no external build-dir"; exit 1; }
# A dot component, a parent traversal and a symlink to $HOME or / as the root are refused.
ln -s "$HOME" "$t/homelink"
for bad in / "$HOME" relative "$HOME/." "$t/cargo/.." "$t/homelink"; do
	if FASTEDIT_BUILD_DIR="$bad/x/y" FASTEDIT_LIVE_BUILD_DIRS="$t/live" "$here/prune-build-dirs.sh" >/dev/null 2>&1; then
		echo "FAIL: unsafe root $bad accepted"; exit 1
	fi
done
# A root reached through a symlinked ancestor is refused and nothing is deleted.
ln -s "$t" "$t/via"
mkdir -p "$root/ff/orphan3"; touch -t 202001010000 "$root/ff/orphan3" "$root/ff"
if FASTEDIT_BUILD_DIR="$t/via/cargo/aa/live" FASTEDIT_LIVE_BUILD_DIRS="$t/live" "$here/prune-build-dirs.sh" >/dev/null 2>&1; then
	echo "FAIL: symlinked ancestor root accepted"; exit 1
fi
[ -d "$root/ff/orphan3" ] && [ -d "$root/aa/live" ] || { echo "FAIL: symlinked ancestor root swept"; exit 1; }

# Real discovery in a git repo with a worktree whose path holds a newline and a non-ASCII
# character; cargo's build-dir comes from CARGO_BUILD_BUILD_DIR.
repo="$t/repo"
mkdir -p "$repo/src"
printf '[package]\nname = "p"\nversion = "0.0.0"\nedition = "2021"\n' >"$repo/Cargo.toml"
: >"$repo/src/lib.rs"
git -C "$repo" init -q
git -C "$repo" -c user.name=t -c user.email=t@t add -A
git -C "$repo" -c user.name=t -c user.email=t@t commit -qm init
wt="$t/wt
é"
git -C "$repo" worktree add -q --detach "$wt"
croot="$t/croot"
export CARGO_BUILD_BUILD_DIR="$croot/{workspace-path-hash}"
bd() { (cd "$1" && cargo metadata --format-version 1 --no-deps) | jq -r .build_directory; }
mkdir -p "$repo/scripts"; cp "$here/prune-build-dirs.sh" "$repo/scripts/"
run_real() { "$repo/scripts/prune-build-dirs.sh" "$@"; }
b_main=$(bd "$repo"); b_wt=$(bd "$wt")
mkdir -p "$b_main/x" "$b_wt/x" "$croot/ab/orphan" "$croot/ab/locked/sub"
echo x >"$croot/ab/locked/sub/f"; touch -t 202001010000 "$croot/ab/orphan" "$croot/ab/locked/sub/f" "$croot/ab/locked/sub" "$croot/ab/locked" "$b_main/x" "$b_wt/x" "$b_main" "$b_wt"
chmod 000 "$croot/ab/locked/sub"
run_real >/dev/null
[ ! -e "$croot/ab/orphan" ] || { echo "FAIL: real orphan kept"; exit 1; }
[ -d "$b_main" ] && [ -d "$b_wt" ] || { echo "FAIL: live build-dir (newline worktree) swept"; exit 1; }
[ -d "$croot/ab/locked" ] || { echo "FAIL: unscannable dir deleted"; exit 1; }
chmod 755 "$croot/ab/locked/sub"

# An unreadable worktree or .verify dir aborts the sweep with every orphan kept.
mkdir -p "$croot/ef/orphan5" "$repo/.worktrees/.verify/v1"; touch -t 202001010000 "$croot/ef/orphan5" "$croot/ef"
chmod 000 "$wt"
if run_real >/dev/null 2>&1; then echo "FAIL: swept with an unreadable worktree"; exit 1; fi
chmod 755 "$wt"
chmod 000 "$repo/.worktrees/.verify/v1"
if run_real >/dev/null 2>&1; then echo "FAIL: swept with an unreadable .verify dir"; exit 1; fi
chmod 755 "$repo/.worktrees/.verify/v1"
[ -d "$croot/ef/orphan5" ] || { echo "FAIL: orphan swept after access failure"; exit 1; }

# An inaccessible parent of a registered worktree, or of .verify, aborts too.
wt2="$t/holder/wt2"
mkdir -p "$t/holder"
git -C "$repo" worktree add -q --detach "$wt2"
chmod 000 "$t/holder"
if run_real >/dev/null 2>&1; then echo "FAIL: swept with an inaccessible worktree parent"; exit 1; fi
chmod 755 "$t/holder"
chmod 000 "$repo/.worktrees"
if run_real >/dev/null 2>&1; then echo "FAIL: swept with an inaccessible .worktrees"; exit 1; fi
chmod 755 "$repo/.worktrees"
[ -d "$croot/ef/orphan5" ] || { echo "FAIL: orphan swept after ancestor access failure"; exit 1; }

# A live worktree whose cargo metadata fails aborts the sweep with every orphan kept.
mkdir -p "$croot/cd/orphan4"; touch -t 202001010000 "$croot/cd/orphan4" "$croot/cd"
echo 'not toml [' >"$wt/Cargo.toml"
if run_real >/dev/null 2>&1; then echo "FAIL: swept despite a failing cargo metadata"; exit 1; fi
[ -d "$croot/cd/orphan4" ] || { echo "FAIL: orphan swept after discovery failure"; exit 1; }
echo ok
