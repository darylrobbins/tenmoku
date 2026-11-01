#!/usr/bin/env bash
# Removes orphaned cargo build-dirs. With `[build] build-dir = "/Volumes/Builds/cargo/{workspace-path-hash}"`
# in ~/.cargo/config.toml, each workspace path gets <root>/<xx>/<hash> holding deps/, build/,
# .fingerprint/ and incremental/ (~38 GB for a full build). Regatta deletes lane, .verify and
# post-merge worktrees without a teardown hook, leaving their build-dirs behind; this sweeps them.
#
#   scripts/prune-build-dirs.sh [--dry-run]
#
# Root = the parent of the parent of this worktree's `cargo metadata` .build_directory. Does
# nothing when no external build-dir is configured (the build-dir is inside the workspace, as on
# Linux hosts and CI). Live = the .build_directory of every existing `git worktree list` entry
# plus every .worktrees/.verify/* dir with a Cargo.toml. A <root>/<xx>/<hash> dir is deleted only
# when it is not live and nothing under it changed in the last 2 hours (a worktree being created
# right now). Symlinks are never followed. Refuses a root that is /, $HOME or not absolute.
#
# Test hooks: FASTEDIT_BUILD_DIR overrides this worktree's build-dir (empty = none configured),
# FASTEDIT_LIVE_BUILD_DIRS names a file listing the live build-dirs instead of discovering them,
# FASTEDIT_PRUNE_RECENT_MIN overrides the 120 minute grace.
# Fails closed: any discovery or traversal error aborts the sweep or skips that directory.
# Needs bash for NUL-delimited reads (worktree paths may hold any character but NUL); runs under
# macOS's bash 3.2.
set -euo pipefail

dry=0
case "${1:-}" in
'') ;;
--dry-run) dry=1 ;;
*) echo "usage: $0 [--dry-run]" >&2; exit 2 ;;
esac
[ $# -le 1 ] || { echo "usage: $0 [--dry-run]" >&2; exit 2; }

PATH="$HOME/.cargo/bin:$PATH"
here=$(cd "$(dirname "$0")/.." && pwd)
recent=${FASTEDIT_PRUNE_RECENT_MIN:-120}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

abort() { echo "prune-build-dirs: $* ; skipping the sweep" >&2; exit 1; }

# Prints the build-dir cargo uses for the workspace at $1, or "-" when it is inside the
# workspace itself. Every step's status is checked on its own; empty output is an error.
build_dir_of() {
	local json out
	json="$tmp/metadata.json"
	(cd "$1" && cargo metadata --format-version 1 --no-deps) >"$json" 2>/dev/null || return 1
	out=$(jq -r '.workspace_root as $w | .build_directory as $b
		| if ($b | startswith($w + "/")) then "-" else $b end' "$json") || return 1
	[ -n "$out" ] && [ "$out" != null ] || return 1
	printf '%s\n' "$out"
}

if [ "${FASTEDIT_BUILD_DIR+x}" ]; then
	mine=${FASTEDIT_BUILD_DIR:--}
else
	command -v cargo >/dev/null 2>&1 && command -v jq >/dev/null 2>&1 || {
		echo "prune-build-dirs: cargo and jq are needed; skipping" >&2
		exit 0
	}
	mine=$(build_dir_of "$here") || abort "cargo metadata failed in $here"
fi
if [ "$mine" = "-" ]; then
	echo "prune-build-dirs: no external build-dir configured; nothing to do"
	exit 0
fi

root=$(dirname "$(dirname "$mine")")
case "$root" in
/*) ;;
*) echo "prune-build-dirs: refusing non-absolute root $root" >&2; exit 2 ;;
esac
case "$root/" in
*/./* | */../* | *//*) echo "prune-build-dirs: refusing root $root with ., .. or empty components" >&2; exit 2 ;;
esac
[ -d "$root" ] || { echo "prune-build-dirs: $root doesn't exist; nothing to do"; exit 0; }
# No component of the root may be a symlink: a link could point the sweep at unrelated data.
p=""
rest=${root#/}
while [ -n "$rest" ]; do
	comp=${rest%%/*}
	case "$rest" in */*) rest=${rest#*/} ;; *) rest="" ;; esac
	p="$p/$comp"
	if [ -L "$p" ]; then
		echo "prune-build-dirs: refusing root $root: $p is a symlink" >&2
		exit 2
	fi
done
home=$(cd -P "$HOME" 2>/dev/null && pwd -P || echo "$HOME")
if [ "$root" = / ] || [ "$root" = "$HOME" ] || [ "$root" = "$home" ]; then
	echo "prune-build-dirs: refusing root $root" >&2
	exit 2
fi

# True for an existing directory, false for a path that is absent; aborts when the path can't
# be inspected (e.g. an ancestor without search permission), which would look like "absent".
is_dir() {
	local err
	[ -d "$1" ] && return 0
	err=$(ls -d -- "$1" 2>&1 >/dev/null) || true
	case "$err" in
	*"No such file"* | "") return 1 ;;
	*) abort "cannot inspect $1: $err" ;;
	esac
}

# The live build-dirs, as an array (paths may contain any character but NUL).
live=("$mine")
if [ -n "${FASTEDIT_LIVE_BUILD_DIRS:-}" ]; then
	while IFS= read -r l || [ -n "$l" ]; do live+=("$l"); done <"$FASTEDIT_LIVE_BUILD_DIRS"
else
	wts=()
	git -C "$here" worktree list --porcelain -z >"$tmp/wt" || abort "git worktree list failed"
	while IFS= read -r -d '' rec; do
		case "$rec" in "worktree "*) wts+=("${rec#worktree }") ;; esac
	done <"$tmp/wt"
	common=$(git -C "$here" rev-parse --path-format=absolute --git-common-dir) || abort "git rev-parse failed"
	# A directory that exists but can't be read hides its Cargo.toml, so it aborts the sweep
	# instead of counting as "not a workspace".
	for vd in "$(dirname "$common")/.worktrees/.verify" "$here/.worktrees/.verify"; do
		is_dir "$vd" || continue
		[ -r "$vd" ] && [ -x "$vd" ] || abort "cannot read $vd"
		for v in "$vd"/*; do
			[ -d "$v" ] || continue
			[ -r "$v" ] && [ -x "$v" ] || abort "cannot read $v"
			[ -f "$v/Cargo.toml" ] && wts+=("$v")
		done
	done
	for wt in "${wts[@]}"; do
		is_dir "$wt" || continue
		[ -r "$wt" ] && [ -x "$wt" ] || abort "cannot read worktree $wt"
		[ -f "$wt/Cargo.toml" ] || continue
		# A live worktree whose build-dir is unknown must not let its build-dir be swept.
		b=$(build_dir_of "$wt") || abort "cargo metadata failed in $wt"
		[ "$b" = "-" ] || live+=("$b")
	done
fi

is_live() {
	local l
	for l in "${live[@]}"; do [ "$l" = "$1" ] && return 0; done
	return 1
}

kb=0
n=0
for xx in "$root"/??; do
	[ -d "$xx" ] && [ ! -L "$xx" ] || continue
	for d in "$xx"/*; do
		[ -d "$d" ] && [ ! -L "$d" ] || continue
		is_live "$d" && continue
		# find does not follow symlinks; any entry modified recently keeps the dir. A failed
		# traversal keeps it too.
		find "$d" -mmin -"$recent" -print -quit >"$tmp/recent" 2>/dev/null || { echo "prune-build-dirs: cannot scan $d; keeping it" >&2; continue; }
		[ ! -s "$tmp/recent" ] || continue
		s=$(du -sk "$d" 2>/dev/null | awk '{ print $1 }') || s=
		[ -n "$s" ] || { echo "prune-build-dirs: cannot size $d; keeping it" >&2; continue; }
		kb=$((kb + s))
		n=$((n + 1))
		if [ "$dry" = 1 ]; then
			echo "would remove $d ($((s / 1048576)) GiB)"
		else
			rm -rf "$d"
		fi
	done
	[ "$dry" = 1 ] || rmdir "$xx" 2>/dev/null || true
done

gib=$(awk -v kb="$kb" 'BEGIN { printf "%.2f", kb / 1048576 }')
if [ "$dry" = 1 ]; then
	echo "prune-build-dirs: would free $gib GiB ($n build-dirs)"
else
	echo "prune-build-dirs: freed $gib GiB ($n build-dirs)"
fi
