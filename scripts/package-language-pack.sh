#!/usr/bin/env bash
# Packs the first-party grammar extensions, one per directory under
# extensions/grammars/, into a tarball that unpacks into the user extensions
# directory, with its checksum: every language, or one pack's.
#
#   scripts/package-language-pack.sh               # every language
#   scripts/package-language-pack.sh --pack jvm    # the pack jvm's languages
#   GRAMMAR_WASM_DIR=dir DIST_DIR=out scripts/package-language-pack.sh
#
# - The wasm come from GRAMMAR_WASM_DIR (default target/grammar-wasm), where
#   scripts/build-grammar-wasm.sh writes them; the rest from GRAMMARS_DIR
#   (default extensions/grammars).
# - With no --pack, writes $DIST_DIR/Tenmoku-language-pack.tar.gz (default
#   dist/) and its .sha256, in the format `sha256sum -c` and `shasum -a 256 -c`
#   read.
# - With --pack <name>, packs only the languages the generated index lists in
#   that pack, into Tenmoku-language-pack-<name>.tar.gz and its .sha256. The
#   index is GRAMMAR_INDEX (default target/registry/index.json), which
#   scripts/build-grammar-registry.sh writes, read through `grammar-registry
#   packs` (GRAMMAR_REGISTRY, default `cargo run` of crates/fe-registry's
#   binary).
# - Each directory is tenmoku.grammar_<language>/ with that language's
#   registry artifact: the wasm, extension.toml, highlights.scm,
#   injections.scm, LICENSE, any LICENSE-* and any NOTICE, and nothing else
#   (not test/ or grammar-source.toml).
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

pack=
while [ $# -gt 0 ]; do
  case $1 in
  --pack)
    [ $# -ge 2 ] && [ -n "$2" ] || die "--pack needs a pack's name"
    pack=$2
    shift 2
    ;;
  *) die "unknown argument $1 (usage: $0 [--pack <name>])" ;;
  esac
done

grammars=${GRAMMARS_DIR:-extensions/grammars}
wasm_dir=${GRAMMAR_WASM_DIR:-target/grammar-wasm}
dist=${DIST_DIR:-dist}
name=Tenmoku-language-pack.tar.gz
if [ -n "$pack" ]; then
  [[ $pack =~ ^[a-z0-9][a-z0-9-]*$ ]] || die "the pack name $pack isn't lowercase letters, digits and -"
  name=Tenmoku-language-pack-$pack.tar.gz
fi

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
if [ -n "$pack" ]; then
  index=${GRAMMAR_INDEX:-target/registry/index.json}
  [ -f "$index" ] || die "no index at $index: write it with scripts/build-grammar-registry.sh"
  read -r -a registry <<<"${GRAMMAR_REGISTRY:-cargo run --quiet --locked -p fe-registry --bin grammar-registry --}"
  listed=$("${registry[@]}" packs --index "$index" "$pack") ||
    die "no pack $pack in $index"
  while IFS= read -r language; do
    [ -n "$language" ] || continue
    [ -f "$grammars/$language/extension.toml" ] ||
      die "$index's pack $pack lists $language, which has no $grammars/$language/extension.toml"
    languages+=("$language")
  done <<<"$listed"
  [ "${#languages[@]}" -gt 0 ] || die "$index's pack $pack has no languages"
else
  for source in "$grammars"/*/extension.toml; do
    [ -f "$source" ] && languages+=("$(basename "$(dirname "$source")")")
  done
  [ "${#languages[@]}" -gt 0 ] || die "no language under $grammars/"
fi

# Check every wasm before writing anything.
missing=()
for language in "${languages[@]}"; do
  wasm=$(field "$grammars/$language/extension.toml" wasm)
  [ -n "$wasm" ] || die "$grammars/$language/extension.toml names no wasm"
  [ -f "$grammars/$language/LICENSE" ] || die "$grammars/$language has no LICENSE"
  [ -f "$wasm_dir/$wasm" ] || missing+=("$wasm_dir/$wasm")
done
if [ "${#missing[@]}" -gt 0 ]; then
  echo "package-language-pack: missing: ${missing[*]}" >&2
  die "build them with scripts/build-grammar-wasm.sh"
fi

stage=$(mktemp -d)
list=$(mktemp)
trap 'rm -rf "$stage" "$list"' EXIT
for language in "${languages[@]}"; do
  src=$grammars/$language
  dir=$stage/tenmoku.grammar_$language
  mkdir "$dir"
  wasm=$(field "$src/extension.toml" wasm)
  cp "$src/extension.toml" "$src/highlights.scm" "$src/injections.scm" "$src/LICENSE" "$dir/"
  # fe_registry::grammars::generate's licence files too.
  for licence in "$src"/LICENSE-* "$src"/NOTICE; do
    if [ -f "$licence" ]; then
      cp "$licence" "$dir/"
    fi
  done
  cp "$wasm_dir/$wasm" "$dir/$wasm"
done

mkdir -p "$dist"
dist=$(cd "$dist" && pwd)
rm -f "$dist/$name" "$dist/$name.sha256"
# A fixed order, owner, modes and times, with GNU tar or BSD tar (macOS's):
# the same inputs make the same archive, whatever the clock, umask or time
# zone. The times and modes are set in the stage, since BSD tar has no
# --mtime; the names are listed sorted, since it has no --sort; and ustar has
# no access or change times, extended attributes or ACLs.
(
  cd "$stage"
  export LC_ALL=C TZ=UTC COPYFILE_DISABLE=1
  find tenmoku.grammar_* -exec touch -t 200001010000 {} +
  find tenmoku.grammar_* -type d -exec chmod 755 {} +
  find tenmoku.grammar_* -type f -exec chmod 644 {} +
  find tenmoku.grammar_* | sort >"$list"
  if tar --version 2>/dev/null | grep -q GNU; then
    tar --format=ustar --no-recursion --owner=0 --group=0 --numeric-owner \
      -cf - -T "$list" | gzip -n >"$dist/$name.tmp"
  else
    tar --format ustar --no-recursion --uid 0 --gid 0 --numeric-owner \
      --no-xattrs --no-acls --no-fflags -cf - -T "$list" | gzip -n >"$dist/$name.tmp"
  fi
)
mv "$dist/$name.tmp" "$dist/$name"
(cd "$dist" && sha "$name" >"$name.sha256")
echo "wrote $dist/$name and $dist/$name.sha256"
cd "$root"
