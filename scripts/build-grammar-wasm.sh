#!/usr/bin/env bash
# Builds the first-party WebAssembly grammars (extensions/grammars/<language>/)
# into target/grammar-wasm/, then checks each highlights its golden sample as
# the built-in language does (crates/fe-syntax/tests/wasm_parity.rs).
#
#   scripts/build-grammar-wasm.sh                  # every language
#   scripts/build-grammar-wasm.sh kotlin scala     # just these
#   WASI_SDK=/path/to/wasi-sdk scripts/build-grammar-wasm.sh
#
# - Each language's grammar-source.toml names the crate and version Cargo.lock
#   pins; its source comes from the cargo registry, so the wasm is the grammar
#   fe-syntax links natively.
# - The flags are those of `tree-sitter build --wasm` (tree-sitter-loader 0.27),
#   with wasi-sdk's clang (default ~/.cache/tree-sitter/wasi-sdk, where the
#   tree-sitter CLI keeps it).
#
# .github/workflows/grammars.yml runs this and publishes the files.
set -euo pipefail

cd "$(dirname "$0")/.."

wasi_sdk=${WASI_SDK:-$HOME/.cache/tree-sitter/wasi-sdk}
out=target/grammar-wasm
PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

die() {
  echo "build-grammar-wasm: $*" >&2
  exit 1
}

# The string `key = "..."` of a TOML file, which must have it.
field() {
  local value
  value=$(sed -n "s/^$2 *= *\"\(.*\)\"\$/\1/p" "$1")
  [ -n "$value" ] || die "$1 has no $2"
  echo "$value"
}

# The versions Cargo.lock has of the crate, one per line.
locked_versions() {
  awk -v name="name = \"$1\"" '
    $0 == name { getline; gsub(/version = |"/, ""); print }
  ' Cargo.lock
}

# Compiles one language, writing the module only once it's whole.
build() {
  local language=$1 source=extensions/grammars/$1/grammar-source.toml
  local crate version grammar src dir wasm temp
  crate=$(field "$source" crate)
  version=$(field "$source" version)
  grammar=$(field "$source" grammar)
  src=$(field "$source" src)

  locked_versions "$crate" | grep -qxF "$version" ||
    die "$source pins $crate $version; Cargo.lock has: $(locked_versions "$crate" | xargs)"
  # The registry's directory has a hash in its name; any copy is the same.
  for dir in "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/"$crate-$version/$src"; do
    break
  done
  [ -f "$dir/parser.c" ] || die "no parser.c in $dir"

  local sources=(parser.c)
  [ -f "$dir/scanner.c" ] && sources+=(scanner.c)

  wasm=$PWD/$out/tree-sitter-${grammar//_/-}.wasm
  temp=$wasm.$$.tmp
  (cd "$dir" && "$wasi_sdk/bin/clang" --target=wasm32-wasip1 \
    --sysroot="$wasi_sdk/share/wasi-sysroot" -o "$temp" -fPIC -shared \
    --no-wasm-opt -Os "-Wl,--export=tree_sitter_$grammar" \
    -Wl,--allow-undefined -Wl,--no-entry -nostdlib -fno-exceptions \
    -fvisibility=hidden -I . "${sources[@]}")
  mv "$temp" "$wasm"
  echo "built $out/$(basename "$wasm") ($crate $version)"
}

[ -x "$wasi_sdk/bin/clang" ] ||
  die "no wasi-sdk clang at $wasi_sdk/bin/clang: set WASI_SDK to a wasi-sdk directory"

languages=("$@")
if [ $# -eq 0 ]; then
  languages=()
  for source in extensions/grammars/*/grammar-source.toml; do
    languages+=("$(basename "$(dirname "$source")")")
  done
fi
for language in "${languages[@]}"; do
  [ -f "extensions/grammars/$language/grammar-source.toml" ] ||
    die "no language $language: expected extensions/grammars/$language/grammar-source.toml"
done

# A whole build starts clean, so a cached target/ can't publish a stale module.
[ $# -gt 0 ] || rm -rf "$out"
mkdir -p "$out"
cargo fetch --locked

# In parallel: each is one slow clang process.
pids=()
for language in "${languages[@]}"; do
  build "$language" &
  pids+=($!)
done
failed=0
for pid in "${pids[@]}"; do
  wait "$pid" || failed=1
done
[ "$failed" -eq 0 ] || die "a grammar failed to build"

# Each language's test is named after it, so naming languages runs only theirs.
filter=()
if [ $# -gt 0 ]; then
  expression=
  for language in "${languages[@]}"; do
    expression="$expression${expression:+ | }test(=$language)"
  done
  filter=(-E "$expression")
fi
scripts/ci-env.sh cargo nextest run --locked -p fe-syntax --test wasm_parity \
  --run-ignored only ${filter[@]+"${filter[@]}"}
