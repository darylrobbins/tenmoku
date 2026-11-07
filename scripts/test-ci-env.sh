#!/usr/bin/env bash
# Self-test for ci-env.sh: test-thread default and override, CARGO_BUILD_JOBS, lane niceness,
# and the stderr summary line. FASTEDIT_BUILD_DIR= (none configured) keeps the .verify probe
# from running the real prune-build-dirs.sh sweep.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
t=$(cd -P "$(mktemp -d)" && pwd -P)
trap 'rm -rf "$t"' EXIT
mkdir -p "$t/.worktrees/lane1" "$t/.worktrees/.verify/x"
fail() { echo "FAIL: $*"; exit 1; }
# probe <dir> [VAR=val ...]: prints "<NEXTEST_TEST_THREADS> <CARGO_BUILD_JOBS> <nice>"; stderr to $t/err.
probe() {
	local dir=$1; shift
	(cd "$dir" && env -u NEXTEST_TEST_THREADS -u CARGO_BUILD_JOBS -u FASTEDIT_LANE_NICE FASTEDIT_BUILD_DIR= FASTEDIT_MIN_FREE_GIB=0 "$@" \
		"$here/ci-env.sh" sh -c 'echo "$NEXTEST_TEST_THREADS $CARGO_BUILD_JOBS $(ps -o nice= -p $$ | tr -d " ")"' 2>"$t/err")
}
base=$(ps -o nice= -p $$ | tr -d ' ')
cores=$(getconf _NPROCESSORS_ONLN)
want=$((cores / 2)); [ "$want" -ge 2 ] || want=2

read -r thr jobs nice < <(probe "$t")
[ "$thr" = "$want" ] || fail "default test threads $thr, want $want"
[ "$jobs" = 6 ] || fail "CARGO_BUILD_JOBS $jobs, want 6"
[ "$nice" = "$base" ] || fail "outside .worktrees nice $nice, want $base"
grep -Eq '^ci-env: test-threads=[0-9]+ jobs=[0-9]+ nice=[0-9]+ load=' "$t/err" || fail "stderr line: $(cat "$t/err")"

read -r thr _ _ < <(probe "$t" NEXTEST_TEST_THREADS=3)
[ "$thr" = 3 ] || fail "caller test threads not kept: $thr"

read -r _ _ nice < <(probe "$t/.worktrees/lane1")
[ "$nice" = "$((base + 10))" ] || fail "lane nice $nice, want $((base + 10))"
grep -q "nice=$((base + 10)) " "$t/err" || fail "lane stderr nice: $(cat "$t/err")"

read -r _ _ nice < <(probe "$t/.worktrees/lane1" FASTEDIT_LANE_NICE=0)
[ "$nice" = "$base" ] || fail "FASTEDIT_LANE_NICE=0 nice $nice, want $base"

read -r _ _ nice < <(probe "$t/.worktrees/.verify/x")
[ "$nice" = "$base" ] || fail ".verify nice $nice, want $base"
echo "ok"
