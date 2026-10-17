#!/bin/sh
# Fixture test for bench-compare.sh: a case whose fastest sample, over all
# rounds, is slower than base by more than the threshold fails the compare
# and is named; equal, faster, within-threshold and one-slow-round cases pass.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT

# case <side/round> <full_id> <per-iteration ns>...: a criterion case dir
# whose samples ran 10 iterations each at those times.
case_() {
  dir="$t/$1/$(echo "$2" | tr ' ' _)/new"
  id=$2
  shift 2
  mkdir -p "$dir"
  printf '{"full_id":"%s","directory_name":"x"}\n' "$id" >"$dir/benchmark.json"
  printf '%s\n' "$@" | jq -s -c '{sampling_mode: "Linear", iters: map(10), times: map(. * 10)}' \
    >"$dir/sample.json"
}
for side in base head; do
  for round in r1 r2; do
    case_ "$side/$round" "edit/equal" 100 120
    # A criterion "base" dir from a previous run: not a result, ignored.
    mkdir -p "$t/$side/$round/edit/equal/base"
    echo '{"iters":[1],"times":[1]}' >"$t/$side/$round/edit/equal/base/sample.json"
  done
done
case_ base/r1 "edit/slower" 100 110
case_ base/r2 "edit/slower" 105
case_ head/r1 "edit/slower" 130 140
case_ head/r2 "edit/slower" 125
case_ base/r1 "parse/10k lines" 1000000
case_ head/r1 "parse/10k lines" 800000
case_ base/r1 "edit/within" 100
case_ head/r1 "edit/within" 110
# Slow in one head round only: the fastest sample is what counts.
case_ base/r1 "edit/noisy" 200
case_ head/r1 "edit/noisy" 900
case_ head/r2 "edit/noisy" 200

fail() { echo "FAIL: $1"; cat "$t/out"; exit 1; }
compare() { "$here/bench-compare.sh" "$@" >"$t/out" 2>&1; }

if compare "$t/base" "$t/head"; then fail "a 25% regression passed"; fi
grep -q 'edit/slower$' "$t/out" || fail "the slower case is not named"
grep -q '^SLOWER .*edit/slower$' "$t/out" || fail "the slower case is not marked"
[ "$(grep -c '^SLOWER' "$t/out")" -eq 1 ] || fail "a case other than edit/slower is marked"
grep -q '^ok .*parse/10k lines$' "$t/out" || fail "the faster case is not ok"

compare "$t/base" "$t/head" 30 || fail "a 25% regression failed a 30% threshold"

rm -rf "$t"/head/*/edit/slower
compare "$t/base" "$t/head" || fail "equal and faster cases failed"
grep -q '^gone .*edit/slower$' "$t/out" || fail "a case missing from head is not listed"

if compare "$t/base" "$t/head" 1x; then fail "a bad threshold was accepted"; fi
if compare "$t/base" "$t/empty"; then fail "a missing dir was accepted"; fi
echo ok
