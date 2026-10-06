#!/usr/bin/env bash
# Checks for the fastedit-task Paddock workflow (.paddock/workflows/fastedit-task.yaml).
#   red <crate>...    the last commit is a `tdd-lock:` commit, the tests build, and some fail
#   green <crate>...  locked tests unchanged, every test that failed at red now passes, all pass
#   guard             no test file changed after the `tdd-lock:` commit that last touched it
#   ci                a clean tree, guard, then the Regatta gate's fmt/vet/test/no_gpui/syntax steps
# Must run under macOS's bash 3.2: no associative arrays, no empty arrays under set -u.
set -euo pipefail

PATH="$HOME/.cargo/bin:$PATH"
cd "$(git rev-parse --show-toplevel)"
# Paddock worktrees live outside .worktrees/, so each builds in its own target/. Pin it so an
# inherited CARGO_TARGET_DIR can't put parallel items in one target (stale-binary race).
export CARGO_TARGET_DIR="$PWD/target"

BASE_REF="${BASE_REF:-main}"
TEST_RE='(^|/)(tests|benches)/|(^|/)tests?\.rs$|_tests?\.rs$'
JUNIT=target/nextest/ci/junit.xml
STATE="$(git rev-parse --git-dir)/paddock-tdd"

die() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

require_clean() {
  [ -z "$(git status --porcelain)" ] || die "$1; git status is not clean"
}

# Prints "<binary-id> <test name>" for each test in the last report that failed or passed.
junit_ids() {
  python3 - "$JUNIT" "$1" <<'PY'
import sys, xml.etree.ElementTree as ET
path, want = sys.argv[1], sys.argv[2]
for tc in ET.parse(path).iter("testcase"):
    tags = {c.tag for c in tc}
    if "skipped" in tags:
        continue
    failed = bool(tags & {"failure", "error"})
    if failed == (want == "failed"):
        print(f"{tc.get('classname')} {tc.get('name')}")
PY
}

guard() {
  local base commits locks bad=0 c f
  git rev-parse --verify --quiet "$BASE_REF^{commit}" >/dev/null || die "unknown base ref: $BASE_REF"
  base=$(git merge-base HEAD "$BASE_REF")
  commits=$(git log --reverse --format='%H %s' "$base..HEAD" | awk '$2 ~ /^tdd-lock:/ {print $1}')
  # "<commit> <file>" per locked file, from the latest tdd-lock commit that touched it.
  locks=$(for c in $commits; do
    git diff-tree --no-commit-id --name-only -r "$c" | { grep -E "$TEST_RE" || true; } | sed "s|^|$c |"
  done | awk '{last[$2] = $1} END {for (f in last) print last[f], f}')
  while read -r c f; do
    [ -n "$c" ] || continue
    git cat-file -e "$c:$f" 2>/dev/null || continue
    if ! git diff --quiet "$c" -- "$f"; then
      echo "changed after tdd-lock commit $(git rev-parse --short "$c"): $f" >&2
      bad=1
    fi
  done <<<"$locks"
  [ "$bad" = 0 ] || die "locked test files changed; restore them with 'git checkout <commit> -- <file>' and change the implementation instead"
  echo "GUARD OK: $(grep -c . <<<"$locks" || true) locked test files"
}

cmd="${1:-}"
[ $# -gt 0 ] && shift
case "$cmd" in
red | green)
  [ $# -gt 0 ] || die "usage: $0 $cmd <crate>..."
  pkgs=()
  for c in "$@"; do
    [[ $c =~ ^[A-Za-z0-9_-]+$ ]] || die "not a cargo package name: $c"
    pkgs+=(-p "$c")
  done
  ;;
esac

case "$cmd" in
red)
  require_clean "commit the tests and stubs first"
  subject=$(git log -1 --format=%s)
  [[ $subject == tdd-lock:* ]] || die "the last commit must be the test commit, subject 'tdd-lock: <item title>' (it is: $subject)"
  [ -n "$(git diff-tree --no-commit-id --name-only -r HEAD | grep -E "$TEST_RE" || true)" ] ||
    die "the tdd-lock commit has no test files; tests go in *_tests.rs, tests.rs, tests/ or benches/"
  cargo nextest run --profile ci --no-run "${pkgs[@]}" ||
    die "the tests must compile; add stubs so they fail on assertions, not on build errors"
  rm -f "$JUNIT"
  if cargo nextest run --profile ci --status-level fail "${pkgs[@]}"; then
    die "every test passes; the new tests must fail before the implementation exists"
  fi
  [ -f "$JUNIT" ] || die "nextest wrote no report at $JUNIT"
  failing=$(junit_ids failed)
  [ -n "$failing" ] || die "nextest failed but no test failed; see the output above"
  mkdir -p "$STATE"
  git rev-parse HEAD >"$STATE/red-sha"
  printf '%s\n' "$failing" >"$STATE/failing"
  printf 'RED OK: %s failing at %s\n%s\n' "$(grep -c . <<<"$failing")" "$(git rev-parse --short HEAD)" "$failing"
  ;;
green)
  [ -f "$STATE/red-sha" ] || die "no red record in this worktree; the red check must pass first"
  red=$(cat "$STATE/red-sha")
  later=$(git log --format='%h %s' "$red..HEAD" | awk '$2 ~ /^tdd-lock:/')
  [ -z "$later" ] || die "no tdd-lock commits while implementing: $later"
  guard
  require_clean "commit your changes; uncommitted work is not merged"
  rm -f "$JUNIT"
  cargo nextest run --profile ci --status-level fail --no-tests=fail "${pkgs[@]}" ||
    die "tests fail; see the output above"
  missing=$(comm -23 <(sort "$STATE/failing") <(junit_ids passed | sort))
  [ -z "$missing" ] || die "tests that failed at red did not run and pass:
$missing"
  echo "GREEN OK: $(grep -c . "$STATE/failing") red tests now pass"
  ;;
guard)
  guard
  ;;
ci)
  require_clean "commit your changes"
  guard
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo nextest run --workspace --profile ci
  scripts/check-no-gpui.sh
  scripts/check-syntax-features.sh
  echo "CI OK"
  ;;
*)
  die "usage: $0 red|green <crate>... | guard | ci"
  ;;
esac
