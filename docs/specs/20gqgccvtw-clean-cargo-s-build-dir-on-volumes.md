---
task_uid: "01m699ktznvepbm320gqgccvtw"
task: "T1255"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "scripts/paddock-check.sh cleanup runs cargo clean, falling back to removing cargo metadata's .build_directory and CARGO_TARGET_DIR by path"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/prune-build-cache.sh prunes debug/incremental under cargo metadata's .build_directory (FASTEDIT_PRUNE_TARGET still overrides), and scripts/test-prune-build-cache.sh passes"
    required: true
    manual: false
  - id: "C3"
    text: "scripts/prune-build-dirs.sh removes only build-dir folders not belonging to any existing worktree or .worktrees/.verify dir and untouched for 2 hours, supports --dry-run, does nothing when no external build-dir is configured, and refuses an unsafe root; scripts/test-prune-build-dirs.sh covers live, orphaned, recently-touched and no-build-dir cases with a fake root"
    required: true
    manual: false
  - id: "C4"
    text: "scripts/ci-env.sh runs the sweeper non-fatally before its free-space check, and its low-space message names the build volume"
    required: true
    manual: false
  - id: "C5"
    text: "No script assumes test binaries live in target/debug/deps. The hand-in need not report sccache --show-stats (documented limit: the stats call timed out after the gate build; review round 2 F4 dropped by the lead)"
    required: true
    manual: false
---

# Clean cargo's build-dir on /Volumes/Builds: cargo clean in paddock cleanup, prune the build-dir's incremental, and sweep build-dirs whose worktree is gone

## Problem

This Mac's `~/.cargo/config.toml` now sets `[build] build-dir = "/Volumes/Builds/cargo/{workspace-path-hash}"`
(an APFS volume sharing the main disk's free space, with Spotlight, FSEvents logging and Time Machine off).
`CARGO_TARGET_DIR` stays `$PWD/target`, but that folder now holds only final artifacts (binaries,
examples, nextest output); `deps/`, `build/`, `.fingerprint/` and `incremental/` live in the build-dir,
one folder per workspace path.

Verified on cargo 1.98.1 (2026-10-31, a scratch crate): `cargo metadata --format-version 1 --no-deps`
reports the folder as `.build_directory`, and `cargo clean` removes both the build-dir and `target/`.
The build-dir is keyed by the workspace's path, so it can only be found (or cleaned by cargo) while
the worktree still exists.

What breaks:

1. `scripts/paddock-check.sh` cleanup does `rm -rf "$CARGO_TARGET_DIR"`, which leaves the build-dir behind.
2. `scripts/prune-build-cache.sh` prunes `$target/debug/incremental`, which is now empty; the
   incremental folders are in the build-dir.
3. `scripts/ci-env.sh`'s free-space check (df on `$PWD`) is still right, since both volumes share one
   pool, but its message says nothing about the build volume.
4. **The big one:** Regatta tears down lanes (`rgt lane down`), finish-gate dirs (`.worktrees/.verify/<id>`),
   post-merge check dirs and gate stacks by deleting the worktree. Each leaves its build-dir (~38 GB for a
   full workspace build) on `/Volumes/Builds` with nothing pointing at it. Regatta is language-agnostic and
   has no teardown hook, so fastedit has to sweep them.

## Change

- `paddock-check.sh` cleanup: run `cargo clean` (removes target and build-dir), falling back to
  removing `.build_directory` and `$CARGO_TARGET_DIR` by path if cargo fails.
- `prune-build-cache.sh`: find the build-dir with `cargo metadata --format-version 1 --no-deps | jq -r
  .build_directory` (keep `FASTEDIT_PRUNE_TARGET` as an override for tests) and prune its
  `debug/incremental`; update `scripts/test-prune-build-cache.sh`.
- `ci-env.sh`: the low-space message names the build volume (both volumes share free space; build
  intermediates live under the configured cargo build-dir) and points at the new sweeper.
- New `scripts/prune-build-dirs.sh`: removes orphaned build-dirs. Root = the parent of the parent of
  this worktree's `.build_directory` (so `/Volumes/Builds/cargo` here); do nothing when no build-dir is
  configured (`.build_directory` inside the workspace's own target, as on Linux hosts and CI). The live
  set is the `.build_directory` of every existing `git worktree list` entry of this repo plus every
  directory under `.worktrees/.verify/` that has a Cargo.toml. Delete a `<root>/<xx>/<hash>` folder only
  when it is not in the live set AND nothing under it was modified in the last 2 hours (a worktree being
  created right now). `--dry-run` prints what it would remove and the GB freed. Never follow symlinks; refuse
  a root that is `/`, `$HOME` or not an absolute path.
- `ci-env.sh` runs the sweeper (non-fatal, quiet unless it frees something) before its free-space check.
- sccache: `scripts/sccache-wrap.sh` unsets `CARGO_TARGET_DIR` for the cache key. The build-dir comes
  from config, not an env var, and per-workspace deps paths already differed per lane before, so sharing
  should be unchanged; note in the hand-in what `sccache --show-stats` reports after the gate's build.
- Nothing may assume test binaries are in `target/debug/deps` (nextest finds them through cargo's
  messages): `rg 'debug/deps' scripts .config` and fix any hit.

## Out of scope

Pruning old `.worktrees/` stacks and logs (the lead does that by hand). Regatta-side teardown hooks.
