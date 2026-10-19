---
task_uid: "01m58zfwzprkrmccfxsqvbhyct"
task: "T214"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Cargo.toml's [profile.dev] has debug = \"line-tables-only\" and split-debuginfo = \"unpacked\", and scripts/ci-env.sh still exports CARGO_PROFILE_DEV_DEBUG=line-tables-only; cargo metadata --format-version 1 --no-deps succeeds"
    required: true
    manual: false
  - id: "C2"
    text: "cargo build --workspace passes and a panic in a dev-profile test still reports file:line (cargo nextest run -p fe-text passes); TESTING.md matches Cargo.toml and ci-env.sh"
    required: true
    manual: false
---

# Build dev profile with line-tables-only debuginfo in the workspace, matching the gate stacks, to shrink lane target dirs and links (Mac T999)

## Problem

The user approved lighter Mac and host builds (2026-10-18). A dev build's debuginfo is the largest part of a lane's
`target/` and of link time. The workspace sets `[profile.dev] debug = "limited"` (`Cargo.toml:192`), but
`scripts/ci-env.sh:32` sets `CARGO_PROFILE_DEV_DEBUG=line-tables-only` only for gate worktrees
(`.worktrees/.verify/*`), so a lane worker's own builds and a developer's builds carry full limited debuginfo while
the gate's don't. TESTING.md:192 documents the gate setting ("panics keep file:line").

## Change

- `Cargo.toml`: `[profile.dev] debug = "line-tables-only"` (keep `split-debuginfo = "unpacked"`). Panics keep
  file:line, and backtraces keep function names and lines; variable inspection in a debugger is the cost.
- `scripts/ci-env.sh`: keep the `CARGO_PROFILE_DEV_DEBUG` export (a gate must not depend on the file) and make its
  comment say it repeats the workspace setting. `TESTING.md`: say the workspace profile now has it too and how to
  get full debuginfo for a debugger session (`CARGO_PROFILE_DEV_DEBUG=limited cargo build`).

Not in this card: deleting a lane's `target/` at teardown (that is Regatta's `rgt lane down`, outside this repo).
