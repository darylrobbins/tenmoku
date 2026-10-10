#!/usr/bin/env bash
# fe-lsp's real-server tests (docs/decisions/lsp-client.md §6.2, §6.3):
# installs the language servers, then runs the `#[ignore]`d `real_server_*`
# tests in crates/fe-lsp/tests/real_server.rs.
#
#   scripts/lsp-real.sh
#   FE_LSP_REQUIRE_SERVERS=1 scripts/lsp-real.sh   # a missing server fails
#
# - Node (tools/lsp/mise.toml) and the npm servers (tools/lsp/package-lock.json)
#   go under tools/lsp, for these tests only: fastedit never installs servers.
# - rust-analyzer is the pinned toolchain's rustup component. It isn't in
#   rust-toolchain.toml, so everyday builds don't download it.
# - FE_LSP_RUST_ANALYZER, FE_LSP_PYRIGHT, FE_LSP_TYPESCRIPT_LANGUAGE_SERVER
#   and FE_LSP_TYPESCRIPT_GO point a test at another binary.
#
# .github/workflows/lsp.yml runs this on demand.
set -euo pipefail

cd "$(dirname "$0")/.."

(cd tools/lsp && mise install)
# The servers run on tools/lsp's Node, and so do the tests' `node` lookups.
# Assigned first, so a failing `mise env` stops the script.
mise_env=$(mise env -C tools/lsp -s bash)
eval "$mise_env"
npm ci --prefix tools/lsp
rustup component add rust-analyzer
cargo nextest run -p fe-lsp --run-ignored only -E 'test(/real_server_/)' \
  --success-output final
