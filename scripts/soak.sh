#!/usr/bin/env bash
# Long randomized runs of the core property tests, in release mode.
#
#   scripts/soak.sh            # 50k cases per property (a few minutes)
#   scripts/soak.sh 500000     # much longer
#
# Worth running after changing undo/redo, anchor or LSP sync code.
#
# Failures print a minimized reproduction; add it to the `regressions`
# module in crates/fe-buffer/src/tests/ideal.rs (or the matching crate's
# tests) so it's checked on every run.
set -euo pipefail

cd "$(dirname "$0")/.."
export PROPTEST_CASES="${1:-50000}"
echo "PROPTEST_CASES=$PROPTEST_CASES"
cargo test --release -p fe-text -p fe-buffer -p fe-display -p fe-editor -- --include-ignored
# fe-lsp's sync round trips, on their own line and by name: fe-lsp's
# `#[ignore]`d `real_server_*` tests need real language servers, so this
# runs no `--include-ignored`.
cargo test --release -p fe-lsp --lib -- tests::sync::round_trip_utf8 tests::sync::round_trip_utf16
