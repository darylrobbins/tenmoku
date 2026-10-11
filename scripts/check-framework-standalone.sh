#!/usr/bin/env bash
# Prove kiln/, the app framework, builds and passes its tests outside this
# repository (docs/decisions/app-framework.md, card K6.2). check-no-gpui.sh's
# boundary check shows kiln names nothing of fastedit's; only a real build
# shows it doesn't lean on our root manifest, lock, patches or vendor/.
#
#   scripts/check-framework-standalone.sh [--dir DIR] [--target-dir DIR] [--keep] [--online] [--no-test]
#
# --dir         the scratch workspace (default: a new mktemp dir under
#               ${TMPDIR:-/tmp}); it must not exist yet.
# --target-dir  its cargo target dir (default: ${TMPDIR:-/tmp}/kiln-standalone-target,
#               kept between runs so only kiln itself rebuilds). Our own
#               CARGO_TARGET_DIR is never used: a separate workspace shares
#               nothing with it but the downloads.
# --keep        keep the scratch workspace after a pass (a failure always keeps it).
# --online      let cargo reach the network; by default it works offline from
#               the local registry cache, so a pass proves nothing new was needed.
# --no-test     build only.
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
scratch=""
target="${KILN_STANDALONE_TARGET_DIR:-${TMPDIR:-/tmp}/kiln-standalone-target}"
keep=0
offline=(--offline)
test=1
while (($#)); do
  case "$1" in
    --dir) scratch="$2"; shift 2 ;;
    --target-dir) target="$2"; shift 2 ;;
    --keep) keep=1; shift ;;
    --online) offline=(); shift ;;
    --no-test) test=0; shift ;;
    -h | --help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "error: unknown argument '$1' (see --help)" >&2; exit 2 ;;
  esac
done

if [[ -z "$scratch" ]]; then
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/kiln-standalone.XXXXXX")
elif [[ -e "$scratch" ]]; then
  echo "error: $scratch already exists; the scratch workspace must start empty" >&2
  exit 2
else
  mkdir -p "$scratch"
fi
scratch=$(cd "$scratch" && pwd)

started=$SECONDS
step=""
finish() {
  local status=$?
  if ((status != 0)); then
    echo >&2
    echo "error: kiln/ doesn't stand alone: '$step' failed (exit $status)." >&2
    echo "  workspace: $scratch (kept; its Cargo.toml is the generated manifest)" >&2
    echo "  reproduce: cd $scratch && CARGO_TARGET_DIR=$target $step" >&2
  elif ((keep)); then
    echo "kept:        $scratch"
  else
    rm -rf "$scratch"
  fi
}
trap finish EXIT

# kiln/ as it would be split out (`git filter-repo --subdirectory-filter kiln`),
# without editor droppings or anything git ignores, plus the toolchain pin:
# the scratch dir is outside the repo, so rustup wouldn't find ours.
step="python3 $repo/scripts/kiln_standalone.py prepare $repo $scratch"
python3 "$repo/scripts/kiln_standalone.py" prepare "$repo" "$scratch"
cp "$repo/rust-toolchain.toml" "$scratch/"
echo "copied:      rust-toolchain.toml"
# No .cargo/config.toml: the repo has none, and kiln's build needs no
# settings of ours (scripts/ci-env.sh only tunes caching through env vars).

# Our lock pins every version; cargo drops the packages kiln doesn't use
# (which --locked alone would refuse), then the helper checks the rest kept
# the versions we test with. From then on the lock must not change.
step="cargo metadata ${offline[*]}"
cp "$repo/Cargo.lock" "$scratch/"
(cd "$scratch" && cargo metadata "${offline[@]}" --format-version 1 >/dev/null)
step="python3 $repo/scripts/kiln_standalone.py lock $repo $scratch"
python3 "$repo/scripts/kiln_standalone.py" lock "$repo" "$scratch"

export CARGO_TARGET_DIR="$target"
echo "target:      $target"
echo

step="cargo build --locked ${offline[*]} --workspace --all-targets"
(cd "$scratch" && cargo build --locked "${offline[@]}" --workspace --all-targets)
if ((test)); then
  step="cargo test --locked --no-fail-fast ${offline[*]} --workspace"
  (cd "$scratch" && cargo test --locked --no-fail-fast "${offline[@]}" --workspace)
fi
step="done"

echo
python3 "$repo/scripts/kiln_standalone.py" report "$repo"
echo
what="builds"
((test)) && what="builds and passes its tests"
echo "ok: kiln/ $what on its own ($((SECONDS - started)) s)"
