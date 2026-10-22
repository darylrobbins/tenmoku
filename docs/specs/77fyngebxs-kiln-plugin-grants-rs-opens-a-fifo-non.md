---
task_uid: "01m5gfb7yby9sbjj77fyngebxs"
task: "T274"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "python3 scripts/check_kiln_boundary.py exits 0 (prints 'ok: kiln/ stands alone') with T271's engine.rs fix in the base; the existing kiln-plugin grants tests pass unchanged and a FIFO standing where a program was is still not waited on; no Cargo change"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln-plugin -p kiln-core, cargo clippy -p kiln-plugin -p kiln-core --all-targets -- -D warnings and cargo fmt --all --check pass"
    required: true
    manual: false
---

# kiln-plugin: grants.rs opens a FIFO non-blocking without branching on the OS outside kiln-core/platform (kiln boundary check fails on main since T264)

## Problem

`python3 scripts/check_kiln_boundary.py` also fails on `kiln/crates/kiln-plugin/src/grants.rs:315` since T264 (plugins P6, Mac T1051) landed: "branches on the OS outside kiln/crates/kiln-core/src/platform, kiln/crates/kiln-platform/src". The file defines `O_NONBLOCK` per OS with `#[cfg(target_os = ...)]` (macOS/iOS/FreeBSD 0x0004; Linux/Android 0x0800, 0x0080 on mips, 0x0004 on sparc, ...), so opening a FIFO put where a program was doesn't wait for a writer. The gate's `no_gpui` stage runs the boundary check, so every integration after T264 fails it until this is fixed. (T271 fixes the separate engine.rs doc comment violation.)

## Change

Keep the behaviour (a FIFO standing where a program was never blocks the grant check; regular files are unaffected) and remove the OS branching from grants.rs:

- Move the per-OS constant and the non-blocking open into `kiln/crates/kiln-core/src/platform/fs.rs` (an allowed OS directory) behind a small function such as `open_nonblocking(path) -> io::Result<File>`, and call it from grants.rs. Keep the per-OS values exactly as they are today.
- No Cargo change (scope has none: T250 holds Cargo.lock).
- The existing grants tests (including the FIFO one, if any) must still pass unchanged.
- Run `python3 scripts/check_kiln_boundary.py` (must print `ok: kiln/ stands alone`, so T271's engine.rs fix must be in your base: `git rebase main` after T271 lands) and `cargo fmt --all --check`.
