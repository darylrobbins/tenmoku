---
task_uid: "01m43sq30f52xmj8xr8dm8gzpn"
task: "T53"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "scripts/ci-env.sh run from a directory under .worktrees/.verify/\u003cid\u003e exports CARGO_TARGET_DIR=$PWD/target; run from a lane or the primary checkout it leaves CARGO_TARGET_DIR unset; both shown with the command and output"
    required: true
    manual: false
  - id: "C2"
    text: "Every [components.commands] entry in regatta.toml runs through scripts/ci-env.sh and none keeps the inline PATH prefix; rgt config validate passes"
    required: true
    manual: false
  - id: "C3"
    text: "rgt ci run --fast passes"
    required: true
    manual: false
---

# Build verification snapshots in their own cargo target

## Why

The untracked `.worktrees/.cargo/config.toml` points every lane and every verification
snapshot (`.worktrees/.verify/<id>`, `-rb`, `-rc`) at one shared cargo target,
`~/Library/Caches/fastedit-regatta/target`. A workspace crate gets the same artifact name
in every worktree, and cargo trusts mtimes, so an artifact another worktree built after
this checkout counts as fresh. Concurrent runs then use each other's builds:

- Post-merge run `01m43rw4j5bcakasb83m9scfdb` (T30, 17db8a2) compiled nothing and ran an
  `fe_syntax` test binary built in a since-deleted snapshot; `golden.rs` failed with
  `tests/golden` NotFound (it bakes in `CARGO_MANIFEST_DIR`). A rerun passed.
- T47's regression check (`runs/regression-01m43rng77twnzamjwptpy448y`) failed at base with
  E0061 in `fe-gpui` `view.rs` (`this.model.fold()`): it linked T30's `fe-editor` rlib from the
  concurrent post-merge snapshot.
- T26's gate ran none of its own `language_tests` (suspected, same cause).

User decision (2026-10-04): lanes keep the shared cache; gate, post-merge and regression
snapshots each build in their own `target/`, accepting a cold build per check.

## What

Add `scripts/ci-env.sh`, a wrapper the `regatta.toml` commands run through: it puts
`$HOME/.cargo/bin` on PATH and, only when the working directory is inside a
`.worktrees/.verify/` snapshot, exports `CARGO_TARGET_DIR="$PWD/target"`; then it execs
its arguments. Route every `[components.commands]` entry through it (cargo commands and
the two check scripts), replacing the inline `PATH=...` prefix. Update the
`regatta.toml` header comment to say snapshots build in their own target and why.
Leave `.config/nextest.toml` alone (its store is already per workspace), and leave
`.worktrees/.cargo/config.toml` alone (lanes keep using it).
