#!/usr/bin/env bash
# nextest setup script: give every test run its own Tenmoku state and config
# dirs, so no test writes the user's real $XDG_STATE_HOME/tenmoku (recent.toml,
# layouts, backups, history). Exports reach the tests through $NEXTEST_ENV.
# The dirs live under the target dir (target/tmp/test-state-<run id>), so
# `cargo clean` removes them. Runs under bash on every platform, Windows CI
# included (Git for Windows ships it): there the paths go out as mixed
# C:/... form, which the Windows test binaries read.
set -eu
target=${CARGO_TARGET_DIR:-target}
base="$target/tmp/test-state-${NEXTEST_RUN_ID:-$$}"
mkdir -p "$base/state" "$base/config"
base=$(cd "$base" && pwd -P)
if command -v cygpath >/dev/null 2>&1; then
	base=$(cygpath -m "$base")
fi
{
	echo "TENMOKU_STATE_DIR=$base/state"
	echo "TENMOKU_CONFIG_DIR=$base/config"
} >>"$NEXTEST_ENV"
