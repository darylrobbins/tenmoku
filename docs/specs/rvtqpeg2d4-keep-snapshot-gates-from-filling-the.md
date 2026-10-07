---
task_uid: "01m4a0rwwbx80q7vrvtqpeg2d4"
task: "T158"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Inside .worktrees/.verify/, ci-env.sh exports CARGO_INCREMENTAL=0 and CARGO_PROFILE_DEV_DEBUG=line-tables-only; outside it, the environment is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "Inside .verify with less than FASTEDIT_MIN_FREE_GIB (default 10) free, ci-env.sh prints the one-line message and exits 75 without running its command; lanes are never checked"
    required: true
    manual: false
  - id: "C3"
    text: "scripts/prune-build-cache.sh supports --dry-run, --older-than and --if-below, deletes only stale session dirs under the shared cache's debug/incremental, keeps each crate's newest, and prints what it freed"
    required: true
    manual: false
  - id: "C4"
    text: "The snapshot build size and time with the new settings are measured (or the finish says why not) and recorded in TESTING.md and the commit message"
    required: true
    manual: false
---

# Keep snapshot gates from filling the disk: lighter snapshot builds, a low-disk check, a build-cache prune script

Gates have failed twice with ENOSPC (T133, 2026-10-06), and free space on the data volume is now about 6 GiB. Everything outside fastedit already fills most of the disk, so fastedit's own build output has to stay small.

Today:
- Lanes share one build cache, `~/Library/Caches/fastedit-regatta/target` (about 22 GiB, 4.5 GiB of it `debug/incremental`), via the untracked `.worktrees/.cargo/config.toml`.
- A verification snapshot (`.worktrees/.verify/<id>`) builds from cold in its own `target/` (`scripts/ci-env.sh` sets `CARGO_TARGET_DIR`), about 6 GiB each, and `pipeline.max_parallel = 2` lets two run at once.
- The dev profile is `debug = "limited"`, `split-debuginfo = "unpacked"` (Cargo.toml).

Do three things.

1. **Lighter snapshot builds.** In `scripts/ci-env.sh`, inside the `*/.worktrees/.verify/*` branch only, also export `CARGO_INCREMENTAL=0` (a snapshot is built once and thrown away, so incremental data is pure waste) and `CARGO_PROFILE_DEV_DEBUG=line-tables-only` (test panics still get file and line backtraces). Every gate stage runs through ci-env.sh, so all stages of one snapshot see the same settings and share one build. Don't touch Cargo.toml's profiles: lanes keep full incremental builds. Measure only the new settings, and only with at least 15 GiB free (the disk is tight; don't build the old settings again): a cold `scripts/ci-env.sh cargo build --workspace --all-targets` plus `scripts/ci-env.sh cargo nextest run --workspace --profile ci --no-run` in a throwaway copy under `.worktrees/.verify/`, then `du -sh target` and wall time, then delete the copy. Compare with the ~6 GiB a snapshot took on 2026-10-06, and record the numbers in the commit message and in a short "Disk" note in TESTING.md. If there's too little space to measure, say so in finish instead. Don't go further (no changes to `debug` for lanes, no `strip`).

2. **Fail fast when the disk is low.** Also in that branch, before `exec`, check free space on the volume holding `$PWD` (`df -Pk "$PWD"`, POSIX sh, no bashisms: the script is `#!/bin/sh`). Below `FASTEDIT_MIN_FREE_GIB` (default 10) print one line to stderr, `ci-env: only N GiB free on <mount>, need FASTEDIT_MIN_FREE_GIB=10; prune the build cache (scripts/prune-build-cache.sh)`, and exit 75 without running the command. Lanes (outside `.verify`) are never checked.

3. **A prune script.** New `scripts/prune-build-cache.sh` (POSIX sh): deletes `incremental/*/*` session directories older than `--older-than` minutes (default 180) under the shared cache's `debug/incremental`, finding the cache from `.worktrees/.cargo/config.toml`'s `target-dir` (fail with a clear message if it can't). `--dry-run` lists what it would delete and the total size; without it, it prints the GiB freed. `--if-below <GiB>` makes it a no-op while free space is at least that. It never touches anything outside that `incremental` directory, never deletes a crate's newest session directory, and never runs `cargo clean`. Use `find` (not `fd`; it may not be installed in CI). Document it in TESTING.md next to the other scripts.

Keep it small: no Rust changes, no launchd plist, no regatta.toml edits.
