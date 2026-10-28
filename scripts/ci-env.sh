#!/bin/sh
# Wrapper for the regatta.toml commands: puts cargo (rustup, ~/.cargo/bin) on PATH and,
# inside a verification snapshot (.worktrees/.verify/<id>), builds in that snapshot's own
# target/, then execs its arguments.
#
# CARGO_BUILD_JOBS defaults to 6 (a caller's value wins) for lanes and snapshots alike: cargo's
# default of one job per core, times up to four lanes and a gate, swamped the machine (load
# ~70, swap full) without adding throughput.
#
# The workspace [profile.dev] builds non-incrementally everywhere (lane targets filling the disk).
# A snapshot is built once and thrown away, so it also repeats that below and keeps only
# line tables (panics still get file:line). It also refuses to start with less than
# FASTEDIT_MIN_FREE_GIB (default 10) free, rather than fail halfway with ENOSPC (exit 75).
#
# When sccache is on PATH and RUSTC_WRAPPER is unset, scripts/sccache-wrap.sh (sccache minus
# CARGO_TARGET_DIR, which would otherwise defeat sharing) becomes the rustc wrapper, so every
# lane and snapshot (each with its own target dir) compiles a registry or git dependency once.
# SCCACHE_DIR defaults under ~/Library/Caches (Time Machine skips it) and SCCACHE_CACHE_SIZE to
# 8G; both can be set by the caller. RUSTC_WRAPPER= (empty) turns it off.
PATH="$HOME/.cargo/bin:$PATH"
export PATH
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}"
if [ -z "${RUSTC_WRAPPER+x}" ] && command -v sccache >/dev/null 2>&1; then
	RUSTC_WRAPPER=$(cd "$(dirname "$0")" && pwd)/sccache-wrap.sh
	export RUSTC_WRAPPER
	export SCCACHE_DIR="${SCCACHE_DIR:-$HOME/Library/Caches/fastedit-sccache}"
	export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-8G}"
	# The server never idles out: a client mid-build got "Connection reset by peer" (os error 104)
	# when the server exited after its default 600 s idle timeout.
	export SCCACHE_IDLE_TIMEOUT="${SCCACHE_IDLE_TIMEOUT:-0}"
	# A client that still cannot talk to its server compiles locally instead of failing the build.
	export SCCACHE_IGNORE_SERVER_IO_ERROR="${SCCACHE_IGNORE_SERVER_IO_ERROR:-1}"
fi
case "$PWD" in
*/.worktrees/.verify/*)
	export CARGO_TARGET_DIR="$PWD/target"
	# Repeat the workspace [profile.dev] settings so a gate does not depend on the file.
	export CARGO_INCREMENTAL=0
	export CARGO_PROFILE_DEV_DEBUG=line-tables-only
	min=${FASTEDIT_MIN_FREE_GIB:-10}
	df_line=$(df -Pk "$PWD" | tail -n 1)
	free_gib=$(echo "$df_line" | awk '{ print int($4 / 1048576) }')
	mount=$(echo "$df_line" | awk '{ $1 = $2 = $3 = $4 = $5 = ""; sub(/^ +/, ""); print }')
	if [ "$free_gib" -lt "$min" ]; then
		echo "ci-env: only $free_gib GiB free on $mount, need FASTEDIT_MIN_FREE_GIB=$min; free space with scripts/prune-build-cache.sh in a lane, or remove old lane target/ dirs" >&2
		exit 75
	fi
	;;
esac
exec "$@"
