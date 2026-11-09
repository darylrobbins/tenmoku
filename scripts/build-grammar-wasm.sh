#!/usr/bin/env bash
# Builds the first-party WebAssembly grammars (extensions/grammars/<language>/)
# into target/grammar-wasm/, then checks each highlights its sample as its
# golden file says (crates/fe-syntax/tests/wasm_parity.rs,
# every_extension_highlights_its_sample).
#
#   scripts/build-grammar-wasm.sh                  # every language
#   scripts/build-grammar-wasm.sh kotlin scala     # just these
#   GRAMMAR_LANGUAGES=kotlin,scala scripts/build-grammar-wasm.sh   # the same
#   UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh dart  # writes test/*.golden
#   WASI_SDK=/path/to/wasi-sdk scripts/build-grammar-wasm.sh
#
# - Each language's grammar-source.toml says where its source is, in one of
#   three forms (extensions/grammars/README.md):
#   - crate + version: the crate source Cargo.lock pins, from the cargo
#     registry, so the wasm is the grammar fe-syntax links natively;
#   - git + rev: that commit of a GitHub repository, fetched whole into
#     target/grammar-src/<language>-<rev>/ and reused from there;
#   - registry = "crates.io" + crate + version + sha256: that .crate from
#     static.crates.io, checked against the hash before it's unpacked into
#     target/grammar-src/.
#   Its parity_needs, other first-party languages its sample injects, are
#   built too.
# - The flags are those of `tree-sitter build --wasm` (tree-sitter-loader 0.27),
#   with wasi-sdk's clang: WASI_SDK, else ~/.cache/tree-sitter/wasi-sdk, where
#   the tree-sitter CLI keeps it. With neither, this downloads the pinned
#   wasi-sdk for the host (macOS arm64 or x86_64, Linux x86_64 or arm64) there.
# - Directories whose grammar export is the same (ERB and EJS) share one module,
#   built once per run.
# - At most GRAMMAR_JOBS clang processes run at once (default half the cores).
# - The test is given GRAMMAR_LANGUAGES, the languages named, and checks only
#   those.
#
# .github/workflows/grammars.yml runs this and publishes the files.
set -euo pipefail

cd "$(dirname "$0")/.."

# .github/workflows/grammars.yml's WASI_SDK_VERSION; wasm_parity.rs checks
# they agree. The hashes are those GitHub lists for the release's archives.
wasi_sdk_version=34.0
wasi_sdk_sha256() {
  case $1 in
  arm64-macos) echo 9c59398106b417f8f14913380fdf0097a8cc0ff4af9eb3ce0065a859e88d49e9 ;;
  x86_64-macos) echo 87d27fa8adc68dee59bfbf2e22a6d34ef717c34d6bf1d8af2a56fc929d9ce0eb ;;
  x86_64-linux) echo b761e3a0721dbae9c09a0059e5fdb2bf917d1b4a8a7b430fb3b5aafb0984b2c4 ;;
  arm64-linux) echo f7e243dff54d60bcc576e94d6166b69f410f2500ae4a9ceef34315be10e77971 ;;
  *) return 1 ;;
  esac
}

grammars=extensions/grammars
out=target/grammar-wasm
fetched=target/grammar-src
cargo_home=${CARGO_HOME:-$HOME/.cargo}
PATH="$cargo_home/bin:$PATH"

die() {
  echo "build-grammar-wasm: $*" >&2
  exit 1
}

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d ' ' -f 1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d ' ' -f 1; }
else
  die "needs sha256sum or shasum"
fi

# Whether a TOML file of ours, one key per line, has the key.
has() {
  grep -q "^$2 *=" "$1"
}

# The string `key = "..."` of a TOML file, which must have it.
field() {
  local value
  value=$(sed -n "s/^$2 *= *\"\(.*\)\"\$/\1/p" "$1")
  [ -n "$value" ] || die "$1 has no $2"
  echo "$value"
}

# The strings of the one-line list `key = ["...", ...]`, one per line; none if
# the file hasn't the key.
strings() {
  sed -n "s/^$2 *= *\[\(.*\)\]\$/\1/p" "$1" | tr ',' '\n' | sed 's/^ *"//; s/" *$//' |
    grep -v '^ *$' || true
}

# The versions Cargo.lock has of the crate, one per line.
locked_versions() {
  awk -v name="name = \"$1\"" '
    $0 == name { getline; gsub(/version = |"/, ""); print }
  ' Cargo.lock
}

# Makes sure there's a wasi-sdk clang at $wasi_sdk, downloading the pinned one
# when it's the default directory and has none.
wasi_sdk_ready() {
  [ -x "$wasi_sdk/bin/clang" ] && return
  [ -z "${WASI_SDK:-}" ] ||
    die "no wasi-sdk clang at $wasi_sdk/bin/clang: set WASI_SDK to a wasi-sdk directory"
  [ ! -e "$wasi_sdk" ] ||
    die "$wasi_sdk has no bin/clang: remove it to download wasi-sdk $wasi_sdk_version, or set WASI_SDK"

  local os arch host expected name url temp
  case $(uname -s) in
  Darwin) os=macos ;;
  Linux) os=linux ;;
  *) os=$(uname -s) ;;
  esac
  case $(uname -m) in
  arm64 | aarch64) arch=arm64 ;;
  x86_64 | amd64) arch=x86_64 ;;
  *) arch=$(uname -m) ;;
  esac
  host=$arch-$os
  expected=$(wasi_sdk_sha256 "$host") ||
    die "no wasi-sdk download for $host: set WASI_SDK to a wasi-sdk $wasi_sdk_version directory"
  name=wasi-sdk-$wasi_sdk_version-$host
  url=https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${wasi_sdk_version%%.*}/$name.tar.gz

  echo "downloading $name into $wasi_sdk"
  mkdir -p "$(dirname "$wasi_sdk")"
  temp=$(mktemp -d "$(dirname "$wasi_sdk")/.wasi-sdk.XXXXXX")
  curl -fsSL -o "$temp/$name.tar.gz" "$url" || { rm -rf "$temp"; die "couldn't download $url"; }
  if [ "$(sha256 "$temp/$name.tar.gz")" != "$expected" ]; then
    rm -rf "$temp"
    die "$url doesn't have its SHA-256 $expected"
  fi
  tar -xzf "$temp/$name.tar.gz" -C "$temp" || { rm -rf "$temp"; die "couldn't unpack $name"; }
  [ -x "$temp/$name/bin/clang" ] || { rm -rf "$temp"; die "$name has no bin/clang"; }
  mv "$temp/$name" "$wasi_sdk"
  rm -rf "$temp"
}

# A path inside the source: relative, and never up out of it.
inside() {
  case /$1/ in
  //* | */../* | */./*) return 1 ;;
  esac
}

# Fetches commit `rev` of the GitHub repository `url` whole (a scanner may
# include a file outside src/), into a directory it prints. A complete fetch
# is reused.
fetch_git() {
  local language=$1 url=$2 rev=$3 dir temp
  dir=$fetched/$language-$rev
  if [ ! -e "$dir/.fetched" ]; then
    echo "fetching $url $rev" >&2
    temp=$dir.$$.tmp
    rm -rf "$temp" "$dir"
    mkdir -p "$temp"
    git -C "$temp" init -q
    git -C "$temp" fetch -q --depth 1 "$url" "$rev" ||
      { rm -rf "$temp"; die "couldn't fetch $rev of $url"; }
    git -C "$temp" -c advice.detachedHead=false checkout -q FETCH_HEAD
    [ "$(git -C "$temp" rev-parse HEAD)" = "$rev" ] ||
      { rm -rf "$temp"; die "$url's fetched commit isn't $rev"; }
    touch "$temp/.fetched"
    mv "$temp" "$dir"
  fi
  echo "$dir"
}

# Downloads crates.io's .crate of `crate` `version`, checks its SHA-256, and
# unpacks it into a directory it prints. A complete unpack is reused.
fetch_crate() {
  local crate=$1 version=$2 expected=$3 dir temp url
  dir=$fetched/crates.io/$crate-$version
  if [ "$(cat "$dir/.fetched" 2>/dev/null)" != "$expected" ]; then
    url=https://static.crates.io/crates/$crate/$crate-$version.crate
    echo "downloading $url" >&2
    temp=$fetched/crates.io/.$crate-$version.$$.tmp
    rm -rf "$temp" "$dir"
    mkdir -p "$temp"
    curl -fsSL -o "$temp/crate.tar.gz" "$url" || { rm -rf "$temp"; die "couldn't download $url"; }
    if [ "$(sha256 "$temp/crate.tar.gz")" != "$expected" ]; then
      rm -rf "$temp"
      die "$url doesn't have its SHA-256 $expected"
    fi
    tar -xzf "$temp/crate.tar.gz" -C "$temp" || { rm -rf "$temp"; die "couldn't unpack $url"; }
    [ -d "$temp/$crate-$version" ] || { rm -rf "$temp"; die "$url has no $crate-$version/"; }
    echo "$expected" >"$temp/$crate-$version/.fetched"
    mv "$temp/$crate-$version" "$dir"
    rm -rf "$temp"
  fi
  echo "$dir"
}

# The directory with `language`'s parser.c, fetching its source if need be;
# the second line says where it came from.
source_dir() {
  local language=$1 source=$grammars/$1/grammar-source.toml
  local root src crate version url rev
  src=$(field "$source" src)
  inside "$src" || die "$source's src $src isn't a path inside the source"
  if has "$source" git; then
    url=$(field "$source" git)
    rev=$(field "$source" rev)
    [[ $url =~ ^https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] ||
      die "$source's git $url isn't https://github.com/<owner>/<repository>"
    [[ $rev =~ ^[0-9a-f]{40}$ ]] || die "$source's rev $rev isn't a 40-hex commit"
    root=$(fetch_git "$language" "$url" "$rev")
    echo "$root/$src"
    echo "$url $rev"
  elif has "$source" registry; then
    [ "$(field "$source" registry)" = crates.io ] ||
      die "$source's registry isn't \"crates.io\""
    crate=$(field "$source" crate)
    version=$(field "$source" version)
    local expected
    expected=$(field "$source" sha256)
    [[ $expected =~ ^[0-9a-f]{64}$ ]] || die "$source's sha256 isn't 64 hex digits"
    root=$(fetch_crate "$crate" "$version" "$expected")
    echo "$root/$src"
    echo "crates.io $crate $version"
  else
    crate=$(field "$source" crate)
    version=$(field "$source" version)
    locked_versions "$crate" | grep -qxF "$version" ||
      die "$source pins $crate $version; Cargo.lock has: $(locked_versions "$crate" | xargs)"
    # The registry's directory has a hash in its name; any copy is the same.
    for root in "$cargo_home"/registry/src/*/"$crate-$version"; do
      break
    done
    echo "$root/$src"
    echo "$crate $version"
  fi
}

# Compiles one language from `dir`, writing the module only once it's whole.
build() {
  local language=$1 dir=$2 from=$3 grammar wasm temp
  grammar=$(field "$grammars/$language/grammar-source.toml" grammar)
  [ -f "$dir/parser.c" ] || die "$language: no parser.c in $dir"
  [ ! -f "$dir/scanner.cc" ] || die "$language: $dir has a C++ scanner.cc, which isn't supported"

  local sources=(parser.c)
  [ -f "$dir/scanner.c" ] && sources+=(scanner.c)

  wasm=$PWD/$out/tree-sitter-${grammar//_/-}.wasm
  temp=$(mktemp "$wasm.XXXXXX")
  (cd "$dir" && "$wasi_sdk/bin/clang" --target=wasm32-wasip1 \
    --sysroot="$wasi_sdk/share/wasi-sysroot" -o "$temp" -fPIC -shared \
    --no-wasm-opt -Os "-Wl,--export=tree_sitter_$grammar" \
    -Wl,--allow-undefined -Wl,--no-entry -nostdlib -fno-exceptions \
    -fvisibility=hidden -I . "${sources[@]}") || { rm -f "$temp"; return 1; }
  mv "$temp" "$wasm"
  echo "built $out/$(basename "$wasm") ($from)"
}

max_jobs=${GRAMMAR_JOBS:-$(($(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2) / 2))}
[[ $max_jobs =~ ^[0-9]+$ ]] || die "GRAMMAR_JOBS isn't a number: $max_jobs"
[ "$max_jobs" -ge 1 ] || max_jobs=1

languages=("$@")
if [ $# -eq 0 ] && [ -n "${GRAMMAR_LANGUAGES:-}" ]; then
  IFS=, read -r -a languages <<<"$GRAMMAR_LANGUAGES"
fi
whole=0
if [ "${#languages[@]}" -eq 0 ]; then
  whole=1
  for source in "$grammars"/*/grammar-source.toml; do
    languages+=("$(basename "$(dirname "$source")")")
  done
fi
for language in "${languages[@]}"; do
  [ -f "$grammars/$language/grammar-source.toml" ] ||
    die "no language $language: expected $grammars/$language/grammar-source.toml"
done

# What's built: the languages, and what their samples inject, transitively.
built=()
pending=("${languages[@]}")
while [ "${#pending[@]}" -gt 0 ]; do
  language=${pending[0]}
  pending=("${pending[@]:1}")
  [[ " ${built[*]-} " == *" $language "* ]] && continue
  [ -f "$grammars/$language/grammar-source.toml" ] ||
    die "a parity_needs names $language, which has no $grammars/$language/grammar-source.toml"
  built+=("$language")
  while IFS= read -r need; do
    pending+=("$need")
  done < <(strings "$grammars/$language/grammar-source.toml" parity_needs)
done

wasi_sdk=${WASI_SDK:-$HOME/.cache/tree-sitter/wasi-sdk}
wasi_sdk_ready

# A whole build starts clean, so a cached target/ can't publish a stale module.
[ "$whole" -eq 0 ] || rm -rf "$out"
mkdir -p "$out" "$fetched"
cargo fetch --locked

# The sources first, one at a time: each may be a download.
dirs=()
froms=()
for language in "${built[@]}"; do
  found=$(source_dir "$language") || exit 1
  dirs+=("$(sed -n 1p <<<"$found")")
  froms+=("$(sed -n 2p <<<"$found")")
done

# Then at most $max_jobs at once: each is one slow clang process. Languages
# sharing a grammar export (ERB and EJS) share a module and a pinned source
# (wasm_parity.rs checks), so the first of them builds it.
pids=()
modules=" "
for i in "${!built[@]}"; do
  grammar=$(field "$grammars/${built[$i]}/grammar-source.toml" grammar)
  [[ $modules == *" $grammar "* ]] && continue
  modules+="$grammar "
  while [ "$(jobs -pr | wc -l)" -ge "$max_jobs" ]; do
    sleep 0.2
  done
  build "${built[$i]}" "${dirs[$i]}" "${froms[$i]}" &
  pids+=($!)
done
failed=0
for pid in "${pids[@]}"; do
  wait "$pid" || failed=1
done
[ "$failed" -eq 0 ] || die "a grammar failed to build"

# The test reads which languages to check; every one when it's empty.
if [ "$whole" -eq 1 ]; then
  GRAMMAR_LANGUAGES=
else
  GRAMMAR_LANGUAGES=$(IFS=,; echo "${languages[*]}")
fi
export GRAMMAR_LANGUAGES
scripts/ci-env.sh cargo nextest run --locked -p fe-syntax --test wasm_parity \
  --run-ignored only --no-fail-fast -E 'test(=every_extension_highlights_its_sample)'
