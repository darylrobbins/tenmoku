#!/usr/bin/env zsh
# Rebuilds word_count.wasm, the component extension.toml names, from this crate. Needs the
# wasm32-wasip2 target: `rustup target add wasm32-wasip2`. Arguments go to cargo, as `--offline`.
set -euo pipefail

here=${0:A:h}
target=${CARGO_TARGET_DIR:-$here/target}

cargo build --manifest-path "$here/Cargo.toml" --target wasm32-wasip2 --release --target-dir "$target" "$@"
cp "$target/wasm32-wasip2/release/word_count.wasm" "$here/word_count.wasm"
