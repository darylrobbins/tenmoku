#!/usr/bin/env bash
# Builds and packages a release, used as a golden highlighting sample.
set -euo pipefail

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-0.1.0}"
TARGETS=(aarch64-apple-darwin x86_64-unknown-linux-gnu)
declare -A SUFFIX=([darwin]=".tar.gz" [linux]=".tar.xz")

log() {
  printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*" >&2
}

die() {
  log "error: $1"
  exit "${2:-1}"
}

suffix_for() {
  local target=$1
  case "$target" in
    *-apple-darwin) echo "${SUFFIX[darwin]}" ;;
    *-linux-*) echo "${SUFFIX[linux]}" ;;
    *) die "unknown target $target" 2 ;;
  esac
}

build() {
  local target=$1 out
  out="$ROOT/dist/fastedit-$VERSION-$target$(suffix_for "$target")"
  if [[ -f $out && -z ${FORCE:-} ]]; then
    log "skipping $target, $out exists"
    return 0
  fi
  cargo build --release --target "$target" --locked
  tar -C "$ROOT/target/$target/release" -czf "$out" fastedit
  log "built ${out##*/} ($(du -h "$out" | cut -f1))"
}

command -v cargo >/dev/null 2>&1 || die "cargo not found"
mkdir -p "$ROOT/dist"

count=0
for target in "${TARGETS[@]}"; do
  build "$target" &
  count=$((count + 1))
done
wait

while read -r line; do
  [[ $line == *.tar.* ]] && echo "  $line"
done < <(ls "$ROOT/dist")

cat <<NOTES >"$ROOT/dist/NOTES.txt"
fastedit $VERSION
built $count targets on $(uname -s)
NOTES

if (( count < ${#TARGETS[@]} )); then
  die "only built $count targets"
elif [ "$count" -eq 0 ]; then
  exit 1
fi
trap 'log done' EXIT
