#!/usr/bin/env bash
# Builds the grammar registry into target/registry/ from extensions/grammars/
# and the modules scripts/build-grammar-wasm.sh built: each language's
# artifact, the index, and the offline tarballs. Nothing is signed or
# published (docs/decisions/xb65tqark9-grammar-registry-index.md).
#
#   scripts/build-grammar-wasm.sh && scripts/build-grammar-registry.sh
#   REGISTRY_SERIAL=1762430400 scripts/build-grammar-registry.sh
#
# Writes, replacing what was there:
#
# - target/registry/languages/<language>/: the language's artifact, exactly
#   the files its index entry lists;
# - target/registry/index.json: the index (crates/fe-registry), its serial
#   REGISTRY_SERIAL, else HEAD's committer time;
# - target/registry/packs/: Tenmoku-language-pack-<pack>.tar.gz for each
#   pack in the index and Tenmoku-language-pack.tar.gz with every language,
#   each with its .sha256 (scripts/package-language-pack.sh). The index lists
#   each pack's tarball.
#
# The same inputs give byte-identical files. GRAMMAR_WASM_DIR is the modules'
# directory, as for scripts/package-language-pack.sh.
set -euo pipefail

cd "$(dirname "$0")/.."

die() {
  echo "build-grammar-registry: $*" >&2
  exit 1
}

out=target/registry
wasm_dir=${GRAMMAR_WASM_DIR:-target/grammar-wasm}
serial=${REGISTRY_SERIAL:-$(git log -1 --format=%ct)}
[[ $serial =~ ^[0-9]+$ ]] || die "the serial $serial isn't a whole number"
[ -d "$wasm_dir" ] || die "no $wasm_dir: build the modules with scripts/build-grammar-wasm.sh"

registry=(cargo run --quiet --locked -p fe-registry --bin grammar-registry --)

rm -rf "$out"
"${registry[@]}" generate --grammars extensions/grammars --wasm "$wasm_dir" --out "$out" \
  --serial "$serial"

export GRAMMAR_WASM_DIR=$wasm_dir GRAMMAR_INDEX=$out/index.json DIST_DIR=$out/packs
export GRAMMAR_REGISTRY="${registry[*]}"
packs=$("${registry[@]}" packs --index "$out/index.json")
while IFS= read -r pack; do
  [ -n "$pack" ] || continue
  scripts/package-language-pack.sh --pack "$pack"
done <<<"$packs"
scripts/package-language-pack.sh

"${registry[@]}" finalize --index "$out/index.json" --tarballs "$out/packs"
