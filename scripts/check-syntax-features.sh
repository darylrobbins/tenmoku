#!/usr/bin/env bash
# Build fe-syntax with no language features and with each one alone. CI's
# other steps only build the default set, where a wrong `cfg` goes unnoticed.
set -euo pipefail

cd "$(dirname "$0")/.."

features=$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys
for p in json.load(sys.stdin)["packages"]:
    if p["name"] == "fe-syntax":
        print("\n".join(f for f in p["features"] if f != "default"))')

check() {
  echo "fe-syntax: ${1:-no features}"
  cargo clippy -q -p fe-syntax --all-targets --no-default-features --features "$1" -- -D warnings
}

check ""
for feature in $features; do
  check "$feature"
done
