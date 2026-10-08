#!/usr/bin/env bash
# Checks for the fastedit-task Paddock workflow (.paddock/workflows/fastedit-task.yaml).
#   red <crate>...    the last commit is a `tdd-lock:` commit, the tests build, and some fail
#   relock <crate>... like red, but none need fail: tests rewritten after a person's decision
#   green <crate>...  locked tests unchanged, every test that failed at red now passes, all pass
#   guard             no test file changed after the `tdd-lock:` commit that last touched it
#   ci                a clean tree, guard, then the Regatta gate's fmt/vet/test/no_gpui/syntax steps
#   land              bring in the base's new commits (rerunning ci if any), then squash the branch
#                     onto the local base: update-ref when nothing has it checked out, else a
#                     fast-forward in its clean checkout (as Regatta lands). Nothing is pushed.
# Every command refuses to run in the main checkout: workflow steps work only in worktrees.
# Must run under macOS's bash 3.2: no associative arrays, no empty arrays under set -u.
set -euo pipefail

PATH="$HOME/.cargo/bin:$PATH:/Applications/Paddock.app/Contents/Resources/bin"
cd "$(git rev-parse --show-toplevel)"
[ "$(git rev-parse --path-format=absolute --git-dir)" != "$(git rev-parse --path-format=absolute --git-common-dir)" ] ||
  { echo "FAIL: $PWD is the main checkout; run this in the workflow's worktree" >&2; exit 1; }
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

ci() {
  require_clean "commit your changes"
  guard
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo nextest run --workspace --profile ci
  scripts/check-no-gpui.sh
  scripts/check-syntax-features.sh
  echo "CI OK"
}

# The title of the board task whose run has this branch checked out, or nothing.
task_title() {
  paddock --json workflow runs 2>/dev/null | python3 -c '
import json, subprocess, sys
runs = json.load(sys.stdin)
task = next((r.get("taskId") for r in runs if r.get("branch") == sys.argv[1]), None)
if task:
    out = subprocess.run(["paddock", "--json", "task", "show", task], capture_output=True, text=True)
    print(json.loads(out.stdout).get("title", "") if out.returncode == 0 else "")
' "$1" 2>/dev/null || true
}

# The worktree that has branch $1 checked out, or nothing.
checkout_of() {
  git worktree list --porcelain | awk -v ref="refs/heads/$1" '
    /^worktree / { path = substr($0, 10) }
    $0 == "branch " ref { print path; exit }'
}

# Records the tdd-lock commit at HEAD and the tests failing now, for the green check.
lock() {
  require_clean "commit the tests and stubs first"
  subject=$(git log -1 --format=%s)
  [[ $subject == tdd-lock:* ]] || die "the last commit must be the test commit, subject 'tdd-lock: <item title>' (it is: $subject)"
  [ -n "$(git diff-tree --no-commit-id --name-only -r HEAD | grep -E "$TEST_RE" || true)" ] ||
    die "the tdd-lock commit has no test files; tests go in *_tests.rs, tests.rs, tests/ or benches/"
  cargo nextest run --profile ci --no-run "${pkgs[@]}" ||
    die "the tests must compile; add stubs so they fail on assertions, not on build errors"
  rm -f "$JUNIT"
  if cargo nextest run --profile ci --status-level fail "${pkgs[@]}"; then
    failing=""
  else
    [ -f "$JUNIT" ] || die "nextest wrote no report at $JUNIT"
    failing=$(junit_ids failed)
    [ -n "$failing" ] || die "nextest failed but no test failed; see the output above"
  fi
  mkdir -p "$STATE"
  git rev-parse HEAD >"$STATE/red-sha"
  printf '%s' "$failing" >"$STATE/failing"
}

cmd="${1:-}"
[ $# -gt 0 ] && shift
case "$cmd" in
red | relock | green)
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
  lock
  [ -n "$failing" ] || die "every test passes; the new tests must fail before the implementation exists"
  printf 'RED OK: %s failing at %s\n%s\n' "$(grep -c . <<<"$failing")" "$(git rev-parse --short HEAD)" "$failing"
  ;;
relock)
  lock
  printf 'RELOCK OK: %s failing at %s\n%s\n' "$(grep -c . <<<"$failing" || true)" "$(git rev-parse --short HEAD)" "$failing"
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
  missing=$(comm -23 <(grep . "$STATE/failing" | sort) <(junit_ids passed | sort))
  [ -z "$missing" ] || die "tests that failed at red did not run and pass:
$missing"
  echo "GREEN OK: $(grep -c . "$STATE/failing" || true) red tests now pass"
  ;;
guard)
  guard
  ;;
ci)
  ci
  ;;
land)
  require_clean "commit your changes"
  branch=$(git symbolic-ref --short HEAD)
  base=$(git rev-parse --verify "refs/heads/$BASE_REF^{commit}")
  if ! git merge-base --is-ancestor "$base" HEAD; then
    # The base moved since the run began: test the merged result before it lands.
    git merge --quiet --no-edit "$base" || {
      git merge --abort
      die "$BASE_REF conflicts with $branch; merge it by hand"
    }
    ci
  fi
  [ "$(git rev-parse "HEAD^{tree}")" != "$(git rev-parse "$base^{tree}")" ] ||
    die "nothing to land: $branch makes no change to $BASE_REF"
  title=$(task_title "$branch")
  landing=$(git commit-tree "HEAD^{tree}" -p "$base" -m "${title:-fastedit-task: $branch}" \
    -m "Landed by the fastedit-task Paddock workflow from $branch.")
  wt=$(checkout_of "$BASE_REF")
  if [ -z "$wt" ]; then
    git update-ref -m "fastedit-task: land $branch" "refs/heads/$BASE_REF" "$landing" "$base" ||
      die "$BASE_REF moved while landing; nothing changed, so retry"
  else
    [ -z "$(git -C "$wt" status --porcelain --untracked-files=no)" ] ||
      die "$wt has uncommitted changes, so $BASE_REF can't be fast-forwarded there; nothing changed"
    [ "$(git -C "$wt" rev-parse HEAD)" = "$base" ] || die "$BASE_REF moved while landing; nothing changed, so retry"
    git -C "$wt" merge --ff-only --quiet "$landing" || die "fast-forwarding $BASE_REF in $wt failed; see above"
  fi
  echo "LANDED: $(git rev-parse --short "$landing") on $BASE_REF: ${title:-$branch}"
  ;;
*)
  die "usage: $0 red|relock|green <crate>... | guard | ci | land"
  ;;
esac
