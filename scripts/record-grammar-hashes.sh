#!/usr/bin/env bash
# Records a grammar release's wasm files in extensions/grammars/pack.toml: an
# entry for each language, each directory under extensions/grammars/, with its
# wasm's file (its extension.toml's), size in bytes and SHA-256, which the app
# checks a download against. Run it after a grammar release, then commit
# pack.toml.
#
#   scripts/record-grammar-hashes.sh            # the release base_url names
#   scripts/record-grammar-hashes.sh grammars-2 # that release; base_url moves to it
#
# Every language's file must be in the release, or pack.toml is left as it was.
# What's before the entries stays, but base_url; the entries are written anew,
# by language. PACK_TOML names another manifest, for the tests.
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

grammars=$(dirname "$0")/../extensions/grammars
pack=${PACK_TOML:-"$grammars/pack.toml"}
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

# "language file size sha256" for each language.
: >"$dir/recorded"
for manifest in "$grammars"/*/extension.toml; do
  [ -f "$manifest" ] || continue
  language=$(basename "$(dirname "$manifest")")
  file=$(sed -n 's/^wasm *= *"\(.*\)"$/\1/p' "$manifest" | head -n 1)
  [ -n "$file" ] || die "$manifest names no wasm"
  [ -f "$dir/$file" ] || die "the release $tag has no $file"
  size=$(wc -c <"$dir/$file")
  hash=$(sha256 "$dir/$file")
  echo "$language $file $((size)) ${hash%% *}" >>"$dir/recorded"
done
[ -s "$dir/recorded" ] || die "no language under $grammars"

# What's before the first entry, base_url moved, without its trailing blank
# lines; then the entries.
awk -v base="https://github.com/$repo/releases/download/$tag" '
  /^\[languages\./ { exit }
  /^base_url = / { $0 = "base_url = \"" base "\"" }
  /^$/ { blank++; next }
  { while (blank > 0) { print ""; blank-- } print }
' "$pack" >"$dir/pack.toml"
while read -r language file size hash; do
  printf '\n[languages.%s]\nfile = "%s"\nsize = %s\nsha256 = "%s"\n' \
    "$language" "$file" "$size" "$hash" >>"$dir/pack.toml"
done <"$dir/recorded"

# Through cat, so that a symlink or the file's mode survives.
cat "$dir/pack.toml" >"$pack"
echo "record-grammar-hashes: recorded $(wc -l <"$dir/recorded" | tr -d ' ') grammars from $tag in $pack"
