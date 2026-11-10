# Tests no longer write the user's Tenmoku state dir

Card T1664 (lane `vt3chx462h`), 2026-11-09. Status: **implemented**.

Every nextest run now gets its own `TENMOKU_STATE_DIR` and `TENMOKU_CONFIG_DIR` from the
setup script `scripts/nextest-isolate-state.sh` (wired into the `default` and `ci` profiles
of `.config/nextest.toml`). The dirs are `<target dir>/tmp/test-state-<run id>`, so
`cargo clean` removes them. The script runs as `["bash", ...]`, so Windows CI (Git for
Windows ships bash) runs it too, with `cygpath -m` paths. `scripts/e2e-macos.sh` exports
temp dirs of its own before its first build. `crates/fe-config/tests/test_state_isolation.rs`
fails when a test process's state dir resolves under the user's real state dir; the
workspace is nextest-only, so plain `cargo test` failing it is intended.

## C1 evidence

Read-only on `~/.local/state/tenmoku`.

**Run 1: `scripts/ci-env.sh cargo nextest run --workspace --profile ci`** (8664 tests, 8663
passed, 1 failed). `shasum -a 256` of all 183 files before and after: 182 identical,
`recent.toml` changed (`2e178e32...601b2` -> `64de8ae2...c4b8c`, mtime 23:02:50, before the
run ended at 23:04:56). Attribution: another lane's *unfixed* snapshot ran nextest at the
same time (pid 12693, started 22:49 in `.worktrees/.verify/01m71rpjcxz91tp8p5e31rnv7m`,
which has no `scripts/nextest-isolate-state.sh`). The live app (pid 76630) is not ruled out.
This run's own tests wrote to the isolated dir: its `state/recent.toml` holds 70
temp-path entries.
The one failure, `fe-vim::search_preview search_preview_finds_the_landing_match`, is a
load flake (machine load ~40), unrelated: it passes alone (3/3).

**Run 2: `cargo nextest run --profile ci -p fastedit -p fe-workbench`**, started with no
other nextest running (2982 passed, 7 skipped, exit 0):

```
before
5e56ff979b0638a7d6c19d12a51a4a15832c35fe40d36ffed546f6a86539ab88  /Users/daryl/.local/state/tenmoku/recent.toml
Nov  9 23:07:14 2026
after
5e56ff979b0638a7d6c19d12a51a4a15832c35fe40d36ffed546f6a86539ab88  /Users/daryl/.local/state/tenmoku/recent.toml
Nov  9 23:07:14 2026
```

`recent.toml` is byte-identical (same sha256) and its mtime did not move.

## C2 evidence

`scripts/e2e-macos.sh` exports both dirs before its first build or launch. `sh -n` passes,
and the line numbers show the order:

```
90:export TENMOKU_STATE_DIR TENMOKU_CONFIG_DIR
345:	run env CARGO_TARGET_DIR="$target" "$cargo" build --release -p fastedit
373:run xcodegen generate --spec "$spec" --project "$e2e"
```
