#!/bin/sh
# RUSTC_WRAPPER set by ci-env.sh. sccache folds every CARGO_* variable in rustc's environment
# into its cache key, and rgt gives each lane and snapshot its own CARGO_TARGET_DIR, so with
# plain sccache nothing ever hits across them (measured: 0 Rust hits on a second build into a
# different target dir). Drop it for the compile; cargo has already resolved it by then.
unset CARGO_TARGET_DIR
exec sccache "$@"
