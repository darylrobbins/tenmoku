#!/usr/bin/env bash
# Records a grammar release's wasm files in extensions/grammars/pack.toml: each
# language's size in bytes and SHA-256, which the app checks a download against.
# Run it after a grammar release, then commit pack.toml.
#
#   scripts/record-grammar-hashes.sh            # the release base_url names
#   scripts/record-grammar-hashes.sh grammars-2 # that release; base_url moves to it
#
# Every language's file must be in the release, or pack.toml is left as it was.
# PACK_TOML names another manifest, for the tests.
#
# See docs/decisions/c1pt00h80c-grammar-fetch.md.
set -euo pipefail

die() {
  echo "record-grammar-hashes: $*" >&2
  exit 1
}

command -v gh >/dev/null 2>&1 || die "needs gh (https://cli.github.com)"
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1"; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1"; }
else
  die "needs sha256sum or shasum"
fi
for tool in awk mktemp wc; do
  command -v "$tool" >/dev/null 2>&1 || die "needs $tool"
done

pack=${PACK_TOML:-"$(dirname "$0")/../extensions/grammars/pack.toml"}
[ -f "$pack" ] || die "no manifest at $pack"

base_url=$(awk -F'"' '/^base_url = /{ print $2; exit }' "$pack")
[[ $base_url =~ ^https://github\.com/([^/]+/[^/]+)/releases/download/([^/]+)$ ]] ||
  die "base_url isn't a GitHub release URL: $base_url"
repo=${BASH_REMATCH[1]}
tag=${1:-${BASH_REMATCH[2]}}

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT

gh release download "$tag" --repo "$repo" --pattern '*.wasm' --dir "$dir" ||
  die "couldn't download the release $tag of $repo"

# "file size sha256" for each file the manifest lists.
: >"$dir/recorded"
for file in $(awk -F'"' '/^file = /{ print $2 }' "$pack"); do
  [ -f "$dir/$file" ] || die "the release $tag has no $file"
  size=$(wc -c <"$dir/$file")
  hash=$(sha256 "$dir/$file")
  echo "$file $((size)) ${hash%% *}" >>"$dir/recorded"
done

awk -v base="https://github.com/$repo/releases/download/$tag" '
  NR == FNR { sizes[$1] = $2; hashes[$1] = $3; next }
  /^base_url = /  { print "base_url = \"" base "\""; next }
  /^file = /      { split($0, part, "\""); file = part[2] }
  /^size = /      { print "size = " sizes[file]; next }
  /^sha256 = /    { print "sha256 = \"" hashes[file] "\""; next }
  { print }
' "$dir/recorded" "$pack" >"$dir/pack.toml"

# Through cat, so that a symlink or the file's mode survives.
cat "$dir/pack.toml" >"$pack"
echo "record-grammar-hashes: recorded $(wc -l <"$dir/recorded" | tr -d ' ') grammars from $tag in $pack"
