#!/bin/sh
# Wrapper for the regatta.toml commands: puts cargo (rustup, ~/.cargo/bin) on PATH and,
# inside a verification snapshot (.worktrees/.verify/<id>), builds in that snapshot's own
# target/ instead of the shared one, then execs its arguments.
#
# A snapshot is built once and thrown away, so it skips incremental data and keeps only
# line tables (panics still get file:line). It also refuses to start with less than
# FASTEDIT_MIN_FREE_GIB (default 10) free, rather than fail halfway with ENOSPC (exit 75).
# Lanes (everything else) are left alone.
PATH="$HOME/.cargo/bin:$PATH"
export PATH
case "$PWD" in
*/.worktrees/.verify/*)
	export CARGO_TARGET_DIR="$PWD/target"
	export CARGO_INCREMENTAL=0
	export CARGO_PROFILE_DEV_DEBUG=line-tables-only
	min=${FASTEDIT_MIN_FREE_GIB:-10}
	df_line=$(df -Pk "$PWD" | tail -n 1)
	free_gib=$(echo "$df_line" | awk '{ print int($4 / 1048576) }')
	mount=$(echo "$df_line" | awk '{ $1 = $2 = $3 = $4 = $5 = ""; sub(/^ +/, ""); print }')
	if [ "$free_gib" -lt "$min" ]; then
		echo "ci-env: only $free_gib GiB free on $mount, need FASTEDIT_MIN_FREE_GIB=$min; prune the build cache (scripts/prune-build-cache.sh)" >&2
		exit 75
	fi
	;;
esac
exec "$@"
