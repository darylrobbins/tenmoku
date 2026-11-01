#!/usr/bin/env bash
# Checks for the fastedit-task, fastedit-regatta and fastedit-stack Paddock workflows
# (.paddock/workflows/).
#   red <crate>...    the last commit is a `tdd-lock:` commit, the tests build, and some fail
#   relock <crate>... like red, but none need fail: tests rewritten after a person's decision
#   green <crate>...  locked tests unchanged, every test that failed at red now passes, all pass
#   green-if-locked <crate>...  green when red recorded a tdd-lock commit, else the crates' tests pass
#   guard             no test file changed after the `tdd-lock:` commit that last touched it
#   ci                a clean tree, guard, then the Regatta gate's fmt/vet/test/no_gpui/syntax steps;
#                     records the tree it passed
#   rerun-failed      each test the last ci run failed, rerun alone three times: passing means a
#                     load flake, logged to the repo's paddock-flakes.log
#   deps <task>...    every named Paddock task is done (none named passes)
#   land              bring in the base's new commits (rerunning ci if any), then squash the branch
#                     onto the local base: update-ref when nothing has it checked out, else a
#                     fast-forward in its clean checkout (as Regatta lands). Nothing is pushed.
#   land-ff           fast-forward the local base to HEAD, keeping its commits (a host stack)
#   post              build and test the landing, unless its tree is the tree ci passed
#   revert            revert the landing on the local base, check the revert, and advance the base
#   sync-hosts        push the base to each Linux host's refs/remotes/mac/main and prompt its lead
#                     (hosts from $FASTEDIT_HOSTS, "host:pane" words)
#   cleanup           cargo clean: delete this worktree's target/ and its cargo build-dir
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

TEST_RE='(^|/)(tests|benches)/|(^|/)tests?\.rs$|_tests?\.rs$'
JUNIT=target/nextest/ci/junit.xml
STATE="$(git rev-parse --git-dir)/paddock-tdd"

die() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

# A string field of the Paddock run that has branch $1 checked out ("taskId", "inputs.baseRef"),
# or nothing. Steps read run data here rather than from `run:` templates: Paddock pastes those
# into the shell unescaped, so a templated value could run as a command.
run_field() {
  paddock --json workflow runs --limit 100000 2>/dev/null | python3 -c '
import json, sys
value = next((r for r in json.load(sys.stdin) if r.get("branch") == sys.argv[1]), {})
for key in sys.argv[2].split("."):
    value = value.get(key) if isinstance(value, dict) else None
print(value if isinstance(value, str) else "")
' "$1" "$2" 2>/dev/null || true
}

# $BASE_REF overrides the run's baseRef input, for running a step by hand.
BASE_REF="${BASE_REF:-$(run_field "$(git symbolic-ref --short -q HEAD || true)" inputs.baseRef)}"
BASE_REF="${BASE_REF:-main}"
git check-ref-format --branch "$BASE_REF" >/dev/null 2>&1 || die "not a branch name: $BASE_REF"

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
  # A stale report would make rerun-failed blame the wrong tests when an earlier step fails.
  rm -f "$JUNIT"
  guard
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo nextest run --workspace --profile ci
  scripts/check-no-gpui.sh
  scripts/check-syntax-features.sh
  mkdir -p "$STATE"
  git rev-parse "HEAD^{tree}" >"$STATE/ci-tree"
  echo "CI OK"
}

# Moves the local base from $2 to $1: update-ref when nothing has it checked out, else a
# fast-forward in its clean checkout. Refuses when the base moved meanwhile.
advance_base() {
  local new=$1 old=$2 why=$3 wt
  wt=$(checkout_of "$BASE_REF")
  if [ -z "$wt" ]; then
    git update-ref -m "$why" "refs/heads/$BASE_REF" "$new" "$old" ||
      die "$BASE_REF moved meanwhile; nothing changed, so retry"
  else
    [ -z "$(git -C "$wt" status --porcelain --untracked-files=no)" ] ||
      die "$wt has uncommitted changes, so $BASE_REF can't be fast-forwarded there; nothing changed"
    [ "$(git -C "$wt" rev-parse HEAD)" = "$old" ] || die "$BASE_REF moved meanwhile; nothing changed, so retry"
    git -C "$wt" merge --ff-only --quiet "$new" || die "fast-forwarding $BASE_REF in $wt failed; see above"
  fi
}

# Builds and tests commit $1 in a temporary worktree that shares this one's target/.
check_commit() {
  local tmp rc=0
  tmp="$(mktemp -d)/check"
  git worktree add --quiet --detach "$tmp" "$1"
  (cd "$tmp" && cargo build --workspace --all-targets && cargo nextest run --workspace --profile ci) || rc=$?
  git worktree remove --force "$tmp"
  return "$rc"
}

# The title of the board task whose run has this branch checked out, or nothing.
task_title() {
  local task
  task=$(run_field "$1" taskId)
  [ -n "$task" ] || return 0
  paddock --json task show "$task" 2>/dev/null |
    python3 -c 'import json, sys; print(json.load(sys.stdin).get("title", ""))' 2>/dev/null || true
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
red | relock | green | green-if-locked)
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
green-if-locked)
  if [ -f "$STATE/red-sha" ]; then
    exec bash "$0" green "$@"
  fi
  require_clean "commit your changes; uncommitted work is not merged"
  cargo nextest run --profile ci --status-level fail "${pkgs[@]}" || die "tests fail; see the output above"
  echo "GREEN OK: no tdd-lock record, so the crates' tests pass"
  ;;
guard)
  guard
  ;;
ci)
  ci
  ;;
rerun-failed)
  [ -f "$JUNIT" ] || die "no test report at $JUNIT: ci failed before its tests ran (fmt, clippy, guard or a build error), which is not a flake"
  failed=$(junit_ids failed)
  [ -n "$failed" ] || die "ci's tests all passed, so it failed in a later step; that is not a flake"
  while read -r bin name; do
    for i in 1 2 3; do
      cargo nextest run --workspace --profile ci --status-level fail -E "binary_id($bin) & test(=$name)" ||
        die "$bin $name fails alone too (run $i of 3), so it isn't a load flake"
    done
  done <<<"$failed"
  log="$(git rev-parse --path-format=absolute --git-common-dir)/paddock-flakes.log"
  while read -r bin name; do
    printf '%s\t%s\t%s %s\n' "$(date -u +%FT%TZ)" "$(git symbolic-ref --short HEAD 2>/dev/null || git rev-parse --short HEAD)" "$bin" "$name" >>"$log"
  done <<<"$failed"
  printf 'FLAKY: these failed under load and pass alone 3 of 3 (logged to %s):\n%s\n' "$log" "$failed"
  ;;
deps)
  for t in "$@"; do
    [[ $t =~ ^[A-Za-z0-9_-]+$ ]] || die "not a Paddock task id: $t"
    state=$(paddock --json task show "$t" 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin).get("state",""))' || true)
    [ "$state" = "done" ] || die "$t is not done (state: ${state:-unknown}); this lane shares files with it"
  done
  echo "DEPS OK: $# tasks done"
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
  advance_base "$landing" "$base" "fastedit-task: land $branch"
  mkdir -p "$STATE"
  echo "$landing" >"$STATE/landing"
  echo "$base" >"$STATE/landing-base"
  echo "LANDED: $(git rev-parse --short "$landing") on $BASE_REF: ${title:-$branch}"
  ;;
land-ff)
  require_clean "commit your changes"
  base=$(git rev-parse --verify "refs/heads/$BASE_REF^{commit}")
  git merge-base --is-ancestor "$base" HEAD || die "$BASE_REF moved past this stack; rebase it onto $BASE_REF and gate it again"
  [ -f "$STATE/ci-tree" ] && [ "$(cat "$STATE/ci-tree")" = "$(git rev-parse "HEAD^{tree}")" ] ||
    die "ci hasn't passed on this tree; run ci first"
  advance_base "$(git rev-parse HEAD)" "$base" "fastedit-stack: land $(git symbolic-ref --short HEAD)"
  mkdir -p "$STATE"
  git rev-parse HEAD >"$STATE/landing"
  echo "$base" >"$STATE/landing-base"
  echo "LANDED: $BASE_REF fast-forwarded to $(git rev-parse --short HEAD), $(git rev-list --count "$base..HEAD") commits"
  ;;
post)
  [ -f "$STATE/landing" ] || die "no landing recorded in this worktree"
  landing=$(cat "$STATE/landing")
  if [ -f "$STATE/ci-tree" ] && [ "$(git rev-parse "$landing^{tree}")" = "$(cat "$STATE/ci-tree")" ]; then
    echo "POST OK: the landing's tree is the tree ci passed; nothing to rerun"
    exit 0
  fi
  check_commit "$landing" || die "the landing $(git rev-parse --short "$landing") fails its build or tests on $BASE_REF"
  echo "POST OK: $(git rev-parse --short "$landing") builds and passes its tests"
  ;;
revert)
  [ -f "$STATE/landing" ] && [ -f "$STATE/landing-base" ] || die "no landing recorded in this worktree"
  landing=$(cat "$STATE/landing")
  from=$(cat "$STATE/landing-base")
  base=$(git rev-parse --verify "refs/heads/$BASE_REF^{commit}")
  git merge-base --is-ancestor "$landing" "$base" || die "$(git rev-parse --short "$landing") is not on $BASE_REF; nothing to revert"
  tmp="$(mktemp -d)/revert"
  git worktree add --quiet --detach "$tmp" "$base"
  # One revert commit per landed commit, newest first: a squash landing is one, a stack is several.
  git -C "$tmp" revert --no-edit "$from..$landing" >/dev/null ||
    { git -C "$tmp" revert --abort 2>/dev/null; git worktree remove --force "$tmp"; die "reverting $(git rev-parse --short "$landing") conflicts with later commits on $BASE_REF; revert it by hand"; }
  reverted=$(git -C "$tmp" rev-parse HEAD)
  git worktree remove --force "$tmp"
  check_commit "$reverted" || die "the revert $(git rev-parse --short "$reverted") fails its own build or tests; $BASE_REF is unchanged"
  advance_base "$reverted" "$base" "fastedit-regatta: revert $(git rev-parse --short "$landing")"
  echo "REVERTED: $(git rev-parse --short "$landing") on $BASE_REF by $(git rev-parse --short "$reverted"); $BASE_REF is frozen until a person re-lands"
  ;;
sync-hosts)
  sha=$(git rev-parse --verify "refs/heads/$BASE_REF^{commit}")
  msg="From the Mac lead: refs/remotes/mac/main is now $(git rev-parse --short "$sha"), and its post-merge check is green. Sync your trunk at your next quiet point. No reply needed."
  for hp in ${FASTEDIT_HOSTS:-neon:w8:p1 argon:w2:p1 helium:w96:p1}; do
    h=${hp%%:*}
    pane=${hp#*:}
    git push -q "ssh://$h/home/daryl/code/fastedit-lanes" "$sha:refs/remotes/mac/main" || die "pushing to $h failed"
    # The host's login shell may be zsh; run the prompt under bash with the message quoted for it.
    printf 'herdr agent prompt %q %q >/dev/null\n' "$pane" "$msg" | ssh "$h" bash -s || die "prompting $h's lead in pane $pane failed"
    echo "synced $h"
  done
  echo "SYNC OK: $(git rev-parse --short "$sha") on every host"
  ;;
cleanup)
  # cargo clean removes target/ and the external build-dir (deps, incremental, ...). If it fails,
  # remove both by path; the build-dir is found first, as it is keyed by this worktree's path.
  build_dir=$(cargo metadata --format-version 1 --no-deps 2>/dev/null | jq -r '.build_directory // empty' 2>/dev/null || true)
  if cargo clean 2>/dev/null; then
    echo "CLEANUP OK: cargo clean removed $CARGO_TARGET_DIR and ${build_dir:-its build-dir}"
  else
    [ -z "$build_dir" ] || rm -rf "$build_dir"
    rm -rf "$CARGO_TARGET_DIR"
    echo "CLEANUP OK: removed $CARGO_TARGET_DIR${build_dir:+ and $build_dir}"
  fi
  ;;
*)
  die "usage: $0 red|relock|green|green-if-locked <crate>... | guard | ci | rerun-failed | deps <task>... | land | land-ff | post | revert | sync-hosts | cleanup"
  ;;
esac
