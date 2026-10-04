#!/bin/sh
# Wrapper for the regatta.toml commands: puts cargo (rustup, ~/.cargo/bin) on PATH and,
# inside a verification snapshot (.worktrees/.verify/<id>), builds in that snapshot's own
# target/ instead of the shared one, then execs its arguments.
PATH="$HOME/.cargo/bin:$PATH"
export PATH
case "$PWD" in
*/.worktrees/.verify/*) export CARGO_TARGET_DIR="$PWD/target" ;;
esac
exec "$@"
