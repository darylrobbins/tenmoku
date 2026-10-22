#!/usr/bin/env zsh
# Rebuilds fs.wasm, the component tests/fs.rs runs, from this crate. Needs the wasm32-wasip2
# target: `rustup target add wasm32-wasip2`. Arguments go to cargo, as `--offline`.
set -euo pipefail

here=${0:A:h}
target=${CARGO_TARGET_DIR:-$here/target}

cargo build --manifest-path "$here/Cargo.toml" --target wasm32-wasip2 --release --target-dir "$target" "$@"
cp "$target/wasm32-wasip2/release/kiln_plugin_fs_guest.wasm" "$here/fs.wasm"
