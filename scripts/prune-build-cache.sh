#!/bin/sh
# Frees disk in the shared regatta build cache by deleting stale incremental session
# directories (<cache>/debug/incremental/<crate>-<hash>/<session>). Each crate's newest
# session is always kept. Nothing outside debug/incremental is touched.
#
#   scripts/prune-build-cache.sh [--dry-run] [--older-than MINUTES] [--if-below GIB]
#
# --older-than defaults to 180. --if-below makes this a no-op while the volume has at
# least GIB free. The cache is the target-dir in .worktrees/.cargo/config.toml.
set -eu

older=180
dry=0
below=
while [ $# -gt 0 ]; do
	case "$1" in
	--dry-run) dry=1 ;;
	--older-than) older=${2:?--older-than needs minutes}; shift ;;
	--if-below) below=${2:?--if-below needs GiB}; shift ;;
	*) echo "usage: $0 [--dry-run] [--older-than MINUTES] [--if-below GIB]" >&2; exit 2 ;;
	esac
	shift
done
case "$older" in '' | *[!0-9]*) echo "prune-build-cache: --older-than must be a whole number of minutes" >&2; exit 2 ;; esac
case "$below" in *[!0-9]*) echo "prune-build-cache: --if-below must be a whole number of GiB" >&2; exit 2 ;; esac

# The cache is the target-dir in .worktrees/.cargo/config.toml, found by walking up from
# here (FASTEDIT_PRUNE_TARGET overrides it, for the test).
target=${FASTEDIT_PRUNE_TARGET:-}
if [ -z "$target" ]; then
	dir=$(cd "$(dirname "$0")" && pwd)
	config=
	while [ "$dir" != "/" ]; do
		if [ -f "$dir/.worktrees/.cargo/config.toml" ]; then config="$dir/.worktrees/.cargo/config.toml"; break; fi
		if [ -f "$dir/.cargo/config.toml" ] && [ "$(basename "$dir")" = ".worktrees" ]; then config="$dir/.cargo/config.toml"; break; fi
		dir=$(dirname "$dir")
	done
	if [ -z "$config" ]; then
		echo "prune-build-cache: can't find .worktrees/.cargo/config.toml above $0" >&2
		exit 1
	fi
	target=$(sed -n 's/^[[:space:]]*target-dir[[:space:]]*=[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p' "$config" | head -n 1)
	if [ -z "$target" ]; then
		echo "prune-build-cache: no target-dir in $config" >&2
		exit 1
	fi
fi
inc="$target/debug/incremental"
if [ ! -d "$inc" ]; then
	echo "prune-build-cache: $inc doesn't exist; nothing to prune"
	exit 0
fi

if [ -n "$below" ]; then
	free_gib=$(df -Pk "$inc" | awk 'NR == 2 { print int($4 / 1048576) }')
	if [ "$free_gib" -ge "$below" ]; then
		echo "prune-build-cache: $free_gib GiB free, at least $below; nothing to do"
		exit 0
	fi
fi

# Candidates: session dirs older than the cutoff, except the newest session of each crate.
list=$(mktemp)
trap 'rm -f "$list"' EXIT
# Only real directories are considered (symlinks and .lock files are skipped), so nothing
# outside $inc is reachable and a newer lock file can't hide the newest session.
for crate in "$inc"/*; do
	[ -d "$crate" ] && [ ! -L "$crate" ] || continue
	sessions=$(mktemp)
	find "$crate" -mindepth 1 -maxdepth 1 -type d >"$sessions"
	[ -s "$sessions" ] || { rm -f "$sessions"; continue; }
	newest=$(tr '\n' '\0' <"$sessions" | xargs -0 ls -dt | head -n 1)
	find "$crate" -mindepth 1 -maxdepth 1 -type d -mmin +"$older" ! -path "$newest" >>"$list"
	rm -f "$sessions"
done

kb=0
while IFS= read -r d; do
	[ -n "$d" ] || continue
	s=$(du -sk "$d" | awk '{ print $1 }')
	kb=$((kb + s))
	if [ "$dry" = 1 ]; then
		echo "would delete $d ($((s / 1024)) MiB)"
	else
		rm -rf "$d"
	fi
done <"$list"

gib=$(awk -v kb="$kb" 'BEGIN { printf "%.2f", kb / 1048576 }')
if [ "$dry" = 1 ]; then
	echo "prune-build-cache: would free $gib GiB ($(wc -l <"$list" | tr -d ' ') session dirs)"
else
	echo "prune-build-cache: freed $gib GiB ($(wc -l <"$list" | tr -d ' ') session dirs)"
fi
