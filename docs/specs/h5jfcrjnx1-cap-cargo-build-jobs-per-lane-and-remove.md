---
task_uid: "01m4gfdf8j495tpqh5jfcrjnx1"
task: "T282"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "scripts/ci-env.sh exports CARGO_BUILD_JOBS=6 for lanes and .verify snapshots when the caller hasn't set CARGO_BUILD_JOBS, and leaves a caller's value untouched"
    required: true
    manual: false
  - id: "C2"
    text: "No file in the repo describes lanes sharing ~/Library/Caches/fastedit-regatta/target or .worktrees/.cargo/config.toml as current behaviour (rg for fastedit-regatta and .worktrees/.cargo outside docs/specs/ finds only historical notes); regatta.toml's header and max_parallel comments describe per-lane targets"
    required: true
    manual: false
  - id: "C3"
    text: "ci-env.sh's low-disk message names a remedy that works without .worktrees/.cargo/config.toml: scripts/prune-build-cache.sh is either removed (and the message updated) or prunes the lanes' own target/debug/incremental, exiting 0 on --dry-run with no config file present"
    required: true
    manual: false
---

# Cap cargo build jobs per lane and remove the stale shared-target docs

## Why

Each cargo build defaults to one job per core (14). With up to 4 lanes plus a snapshot gate
building at once, the Mac ran at load average ~70 with 17 of 18 GB swap used (2026-10-09).
Oversubscription past the core count doesn't add throughput, and the extra rustc/linker memory
pushes the machine into swap. That slows every build and the gate's timing-sensitive tests
(see the T94 gate timeouts under load).

Separately, the shared build cache is being retired: `.worktrees/.cargo/config.toml`
(`target-dir = ~/Library/Caches/fastedit-regatta/target`) never applied to rgt stages, which
set `CARGO_TARGET_DIR` per lane, so it only made each lane build twice and caused the
cross-lane artifact races (T24, T8, T63). Once it's removed, lanes build in their own `target/`.
Several files still describe the shared target.

## What

1. `scripts/ci-env.sh` exports a default `CARGO_BUILD_JOBS` (6) for lanes and snapshots
   unless the caller already set it.
2. Remove stale descriptions of the shared target: the header comment and the `max_parallel`
   comment in `regatta.toml`, `TESTING.md` (~line 53), and `scripts/prune-build-cache.sh` plus
   the hint that points at it in `ci-env.sh`. Either retire the prune script or retarget it at
   the lanes' own `target/debug/incremental`; pick the smaller change that still gives the
   low-disk message a working remedy.
