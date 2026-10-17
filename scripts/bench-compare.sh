#!/bin/sh
# Compares two sets of criterion results by each case's fastest sample.
#
#   scripts/bench-compare.sh <base-dir> <head-dir> [threshold-percent]
#
# Each dir holds one or more criterion output trees (a CRITERION_HOME per
# round, say); every <case>/new/sample.json under it counts, and the case is
# named by the full_id in the benchmark.json beside it. A case's time is its
# fastest sample (sample time over iterations) across all of them: on a
# noisy machine the means swing, the minimum holds still (PERF.md).
#
# Prints a table and exits 1 if any case in both is slower than base by more
# than the threshold (default 15%), 2 on bad arguments or no results. Cases
# in only one side are listed but don't fail. Needs jq.
set -eu

usage() { echo "usage: $0 <base-dir> <head-dir> [threshold-percent]" >&2; exit 2; }
[ $# -eq 2 ] || [ $# -eq 3 ] || usage
base=$1
head=$2
threshold=${3:-15}
case $threshold in
  '' | *[!0-9.]* | .* | *. | *.*.* ) usage ;;
esac
for dir in "$base" "$head"; do
  [ -d "$dir" ] || { echo "$0: no such dir: $dir" >&2; exit 2; }
done

# One {"id", "ns"} per sample.json: the id from benchmark.json, the time its
# fastest sample's per-iteration nanoseconds.
fastest() {
  find "$1" -path '*/new/sample.json' -exec sh -c '
    for sample; do
      jq -n -c --slurpfile b "${sample%/sample.json}/benchmark.json" \
        --slurpfile s "$sample" \
        "{id: \$b[0].full_id, ns: ([\$s[0].times, \$s[0].iters] | transpose | map(.[0] / .[1]) | min)}"
    done' sh {} +
}

t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
fastest "$base" >"$t/base"
fastest "$head" >"$t/head"
[ -s "$t/base" ] && [ -s "$t/head" ] || {
  echo "$0: no criterion results (*/new/sample.json) in $base or $head" >&2
  exit 2
}

# Rows of status, id, base ns, head ns, change %, sorted by id.
jq -n -r --slurpfile base "$t/base" --slurpfile head "$t/head" \
  --argjson threshold "$threshold" '
  def fold: group_by(.id) | map({key: .[0].id, value: (map(.ns) | min)}) | from_entries;
  ($base | fold) as $b | ($head | fold) as $h
  | ([$b, $h] | map(keys) | add | unique)[]
  | . as $id
  | if ($b | has($id) | not) then ["new", $id, "", $h[$id], ""]
    elif ($h | has($id) | not) then ["gone", $id, $b[$id], "", ""]
    else ($h[$id] / $b[$id] - 1) * 100 as $change
      | [(if $change > $threshold then "SLOWER" else "ok" end), $id, $b[$id], $h[$id], $change]
    end
  | @tsv' >"$t/rows"

awk -F '\t' -v threshold="$threshold" '
  function time(ns) {
    if (ns == "") return "-"
    if (ns >= 1e9) return sprintf("%.2f s", ns / 1e9)
    if (ns >= 1e6) return sprintf("%.2f ms", ns / 1e6)
    if (ns >= 1e3) return sprintf("%.2f us", ns / 1e3)
    return sprintf("%.1f ns", ns)
  }
  BEGIN { printf "%-7s %12s %12s %8s  %s\n", "", "base", "head", "change", "case" }
  {
    change = $5 == "" ? "-" : sprintf("%+.1f%%", $5)
    printf "%-7s %12s %12s %8s  %s\n", $1, time($3), time($4), change, $2
    if ($1 == "SLOWER") slower[++n] = $2
  }
  END {
    if (n == 0) { printf "\nno case is more than %s%% slower than base\n", threshold; exit 0 }
    printf "\n%d case(s) more than %s%% slower than base:\n", n, threshold
    for (i = 1; i <= n; i++) print "  " slower[i]
    exit 1
  }' "$t/rows"
