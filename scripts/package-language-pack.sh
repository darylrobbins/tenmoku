#!/usr/bin/env bash
# Packs the eight first-party grammar extensions into one tarball that unpacks
# into the user extensions directory, with its checksum.
#
#   scripts/package-language-pack.sh
#   GRAMMAR_WASM_DIR=dir DIST_DIR=out scripts/package-language-pack.sh
#
# - The wasm come from GRAMMAR_WASM_DIR (default target/grammar-wasm), where
#   scripts/build-grammar-wasm.sh writes them.
# - Writes $DIST_DIR/Tenmoku-language-pack.tar.gz (default dist/) and its
#   .sha256, in the format `sha256sum -c` and `shasum -a 256 -c` read.
# - Each directory is what crates/fastedit/src/grammar_install.rs writes:
#   tenmoku.grammar_<language>/ with the wasm, highlights.scm, injections.scm
#   and extension.toml.
#
# .github/workflows/grammars.yml runs this and publishes the files.
# docs/release/language-pack.md is the install guide.
set -euo pipefail

cd "$(dirname "$0")/.."
root=$PWD

die() {
  echo "package-language-pack: $*" >&2
  exit 1
}

wasm_dir=${GRAMMAR_WASM_DIR:-target/grammar-wasm}
dist=${DIST_DIR:-dist}
name=Tenmoku-language-pack.tar.gz

command -v tar >/dev/null || die "tar isn't installed"
if command -v sha256sum >/dev/null; then
  sha() { sha256sum "$@"; }
elif command -v shasum >/dev/null; then
  sha() { shasum -a 256 "$@"; }
else
  die "neither sha256sum nor shasum is installed"
fi

# The string `key = "..."` of a TOML file's first such line.
field() {
  sed -n "s/^$2 *= *\"\(.*\)\"\$/\1/p" "$1" | head -n 1
}

languages=()
for source in extensions/grammars/*/extension.toml; do
  languages+=("$(basename "$(dirname "$source")")")
done
[ "${#languages[@]}" -eq 8 ] ||
  die "expected 8 languages under extensions/grammars/, found ${#languages[@]}"

# Check every wasm before writing anything.
missing=()
for language in "${languages[@]}"; do
  wasm=$(field "extensions/grammars/$language/extension.toml" wasm)
  [ -n "$wasm" ] || die "extensions/grammars/$language/extension.toml names no wasm"
  [ -f "$wasm_dir/$wasm" ] || missing+=("$wasm_dir/$wasm")
done
if [ "${#missing[@]}" -gt 0 ]; then
  echo "package-language-pack: missing: ${missing[*]}" >&2
  die "build them with scripts/build-grammar-wasm.sh"
fi

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
for language in "${languages[@]}"; do
  src=extensions/grammars/$language
  dir=$stage/tenmoku.grammar_$language
  mkdir "$dir"
  wasm=$(field "$src/extension.toml" wasm)
  cp "$src/extension.toml" "$src/highlights.scm" "$src/injections.scm" "$dir/"
  cp "$wasm_dir/$wasm" "$dir/$wasm"
done

mkdir -p "$dist"
dist=$(cd "$dist" && pwd)
rm -f "$dist/$name" "$dist/$name.sha256"
# A fixed order, owner and times: the same inputs make the same archive.
(
  cd "$stage"
  export LC_ALL=C
  if tar --version 2>/dev/null | grep -q GNU; then
    tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='2000-01-01 00:00:00' \
      -cf - tenmoku.grammar_* | gzip -n >"$dist/$name.tmp"
  else
    tar --uid 0 --gid 0 -cf - tenmoku.grammar_* | gzip -n >"$dist/$name.tmp"
  fi
)
mv "$dist/$name.tmp" "$dist/$name"
(cd "$dist" && sha "$name" >"$name.sha256")
echo "wrote $dist/$name and $dist/$name.sha256"
cd "$root"
