---
task_uid: "01m4gfd6pd5yb25mcmxtwb52yd"
task: "T281"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "scripts/ci-env.sh sets RUSTC_WRAPPER to sccache, through scripts/sccache-wrap.sh (which only drops CARGO_TARGET_DIR, since sccache folds CARGO_* into its key, and execs sccache), for both lanes and .verify snapshots when sccache is found and RUSTC_WRAPPER is unset; with sccache absent from PATH or RUSTC_WRAPPER already set, the exported environment is unchanged from base"
    required: true
    manual: false
  - id: "C2"
    text: "sccache is actually used under rgt: the stage environment rgt gives ci-env.sh finds sccache (check its PATH, which may lack /opt/homebrew/bin), shown by sccache --show-stats compile requests rising across a rgt ci run --fast in the lane"
    required: true
    manual: false
  - id: "C3"
    text: "SCCACHE_DIR defaults to a directory under ~/Library/Caches/ and SCCACHE_CACHE_SIZE to a bounded size (at most 10G), each overridable by the caller's environment"
    required: true
    manual: false
  - id: "C4"
    text: "Evidence in the task notes: two cold workspace builds (cargo build --workspace --all-targets) into two empty target dirs through ci-env.sh, with wall times and the second build's sccache hit count for non-workspace crates"
    required: true
    manual: false
  - id: "C5"
    text: "TESTING.md describes the cache, its location and size bound, and how to turn it off (RUSTC_WRAPPER= )"
    required: true
    manual: false
---

# Share compiled dependencies across lanes and snapshot gates with sccache

## Why

Every lane and every snapshot gate (`.worktrees/.verify/<id>`) compiles GPUI and its whole
dependency tree from scratch. rgt deliberately gives each lane and snapshot its own
`CARGO_TARGET_DIR` (regatta `internal/stacks/descriptors/rust-cargo.toml`), so nothing is reused
between them. With 4 lanes plus gates, the 14-core Mac sat at load average ~70 with 17/18 GB
swap in use (2026-10-09). Registry and git dependencies are identical in every lane, so a
compiler cache can serve them.

## What

In `scripts/ci-env.sh`, use sccache as `RUSTC_WRAPPER` for lanes and snapshots when it is
installed, so each dependency is compiled once and reused by every lane and gate.

- Only when `sccache` is on PATH and `RUSTC_WRAPPER` isn't already set. Without sccache,
  behaviour is unchanged. sccache 0.18.0 is installed on this Mac (`/opt/homebrew/bin/sccache`,
  in `~/.config/homebrew/Brewfile`).
- Default `SCCACHE_DIR` under `~/Library/Caches/` (Time Machine excludes it) and a bounded
  `SCCACHE_CACHE_SIZE` (disk is tight: ~21 GiB free on 2026-10-09). Both overridable.
- Lanes build incrementally, so sccache skips workspace crates there; that's expected. Snapshots
  build with `CARGO_INCREMENTAL=0` and `line-tables-only`, so they get their own cache entries,
  still shared across all gates. Check whether sccache's base-dir support lets snapshot
  workspace crates hit too (their absolute path changes per snapshot); note the finding either way.

## Implementation notes

- **`RUSTC_WRAPPER` is `scripts/sccache-wrap.sh`, not bare `sccache`.** sccache folds every
  `CARGO_*` variable in rustc's environment into its key, and rgt exports a per-lane /
  per-snapshot `CARGO_TARGET_DIR`. With plain `RUSTC_WRAPPER=sccache` a second build into a
  different target dir got **0** Rust hits (454 misses, measured; reproduced on `-p memchr`
  with the env var vs `--target-dir`). The wrapper unsets `CARGO_TARGET_DIR` and execs sccache.
- **PATH under rgt** includes `/opt/homebrew/bin` (logged from ci-env.sh during a `rgt ci run
  --fast`), so a plain `command -v sccache` finds it; no fallback path needed.
- **Cold builds** (`cargo build --workspace --all-targets`, `CARGO_INCREMENTAL=0`, two empty
  target dirs, each through `scripts/ci-env.sh`, server stopped first so it started with
  ci-env's `SCCACHE_DIR`; `sccache --show-stats` reported `~/Library/Caches/fastedit-sccache`,
  8 GiB): build 1 263 s, 454 Rust misses, 0 hits; build 2 206 s, **343 Rust hits**, 111 misses
  (workspace crates and build scripts, which are path-dependent), so the non-workspace crates
  are served from the cache. An earlier attempt without the wrapper got 0 Rust hits on build 2.
  (An earlier run of these builds landed in the default `Mozilla.sccache` because my harness
  had started a server with the default dir; a running server ignores a new `SCCACHE_DIR`.)
- **Base dirs**: sccache 0.18.0 reports `Base directories (none)`; its `SCCACHE_BASEDIRS`
  support is newer, so workspace crates in snapshots do not hit. Not pursued.
- The sccache server fixes `SCCACHE_DIR` at start; a server started elsewhere with another
  dir keeps it until `sccache --stop-server`. Documented in TESTING.md.
