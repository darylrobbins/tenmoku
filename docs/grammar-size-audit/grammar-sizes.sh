#!/usr/bin/env bash
# Per-grammar size of the tree-sitter grammars fe-syntax links in.
#
# Usage: grammar-sizes.sh [--partial] [debug|release] [BUILD_DIR]
#
# Reads an EXISTING build; never builds. For every tree-sitter-* crate it finds
# the static library its build script produced (<build-dir>/<profile>/build/
# tree-sitter-*/out/*.a), sums that library's __TEXT and __DATA* sections with
# `size -m` (debug info lives in __DWARF and is excluded; the parse tables are
# constant data), and adds the bytes of the .scm query files fe-syntax embeds
# with include_str! for the language. Prints a table sorted by size, then a total.
#
# BUILD_DIR defaults to cargo's build_directory (cargo metadata; this machine
# sets build.build-dir in ~/.cargo/config.toml), else target_directory.
# Build first with e.g. `cargo build -p fe-syntax` (debug) if there is none.
# The expected crates are fe-syntax'"'"'s optional tree-sitter-* dependencies (cargo
# metadata). A crate with no static library, or an empty build-dir, is reported
# on stderr and the script exits 1, unless --partial is given (then the table
# is headed "PARTIAL" and missing grammars are still listed).
# macOS only (Mach-O `size -m`).
set -euo pipefail

partial=0
if [ "${1:-}" = --partial ]; then partial=1; shift; fi
profile=${1:-debug}
here=$(cd "$(dirname "$0")" && pwd)
queries=$here/../../crates/fe-syntax/queries

meta=$(cargo metadata --format-version 1 --no-deps --manifest-path "$here/../../Cargo.toml")
expected=$(jq -r '.packages[] | select(.name == "fe-syntax") | .dependencies[]
  | select(.optional and (.name | startswith("tree-sitter-"))) | .name | sub("^tree-sitter-"; "")' <<<"$meta" | sort)
build_dir=${2:-}
if [ -z "$build_dir" ]; then
  build_dir=$(jq -r '.build_directory // .target_directory' <<<"$meta")
fi
root=$build_dir/$profile/build
[ -d "$root" ] || { echo "no $root: build first (cargo build -p fe-syntax)" >&2; exit 1; }

# Languages whose queries embed under a crate; default is the crate name minus
# "tree-sitter-". TypeScript's crate also holds TSX (indents/tsx, jsx).
langs_for() {
  case $1 in
    c-sharp) echo csharp ;;
    containerfile) echo dockerfile ;;
    kotlin-sg) echo kotlin ;;
    md) echo markdown ;;
    sequel) echo sql ;;
    toml-ng) echo toml ;;
    typescript) echo "typescript tsx" ;;
    javascript) echo "javascript jsx" ;;
    rust) echo "rust rustdoc-inline-highlights" ;;
    *) echo "$1" ;;
  esac
}

query_bytes() { # <lang...>
  local total=0 f n
  for l in "$@"; do
    for f in "$queries/$l.scm" "$queries/$l-injections.scm" "$queries"/{outline,definitions,indents,imports}/"$l.scm"; do
      [ -f "$f" ] || continue
      n=$(wc -c <"$f"); total=$((total + n))
    done
  done
  echo "$total"
}

found=$(mktemp); rows=$(mktemp); trap 'rm -f "$rows" "$found"' EXIT
for dir in "$root"/tree-sitter-*; do
  name=${dir##*/}; name=${name#tree-sitter-}
  case $name in *-*) name=${name%-*} ;; *) continue ;; esac # drop the hash; skip the core runtime
  # A stale or unrelated grammar left in a reused build-dir is not measured.
  grep -qxF "$name" <<<"$expected" || { echo "ignoring $name: not an fe-syntax dependency" >&2; continue; }
  lib=$(ls -t "$dir"/out/*.a 2>/dev/null | head -1 || true)
  [ -n "$lib" ] || continue
  code=$(size -m "$lib" 2>/dev/null |
    awk '/^[[:space:]]*Section \((__TEXT|__DATA[A-Za-z_]*),/ { s += $NF } END { print s + 0 }')
  q=$(query_bytes $(langs_for "$name"))
  echo "$name" >>"$found"
  echo "$code $q $name $(stat -f %m "$lib")" >>"$rows"
done

missing=$(comm -23 <(echo "$expected") <(sort -u "$found"))
if [ -n "$missing" ]; then
  echo "missing static libraries under $root for: $(echo $missing)" >&2
  [ "$partial" = 1 ] || { echo "incomplete build: build fe-syntax with default features, or pass --partial" >&2; exit 1; }
  echo "PARTIAL audit: the total below leaves out the grammars listed above"
fi

# Several hashed dirs for one crate (stale builds): keep the newest only.
printf '%-16s %12s %12s %12s\n' grammar code+data queries total
sort -k3,3 -k4,4r "$rows" | awk '!seen[$3]++ { print }' | sort -rn |
  awk '{ printf "%-16s %12d %12d %12d\n", $3, $1, $2, $1 + $2; c += $1; q += $2 }
       END { printf "%-16s %12d %12d %12d\n", "TOTAL", c, q, c + q
             printf "\nTOTAL %.1f MB code+data, %.2f MB queries (1 MB = 1048576 bytes)\n", c / 1048576, q / 1048576 }'
