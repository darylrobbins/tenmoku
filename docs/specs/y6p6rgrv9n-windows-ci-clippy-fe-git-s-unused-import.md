---
task_uid: "01m7qg77jkjmz4b5y6p6rgrv9n"
task: "T1951"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "On a non-unix target, remote.rs no longer imports run_raw_with unused and Askpass.answer is no longer a never-read field: both are behind #[cfg(unix)], or a cfg_attr(not(unix), allow(...)) with a comment giving the reason; unix behaviour and fe-git's public API are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "cargo clippy -p fe-git --all-targets -- -D warnings and cargo nextest run -p fe-git pass on the worker's host"
    required: true
    manual: false
---

# Windows CI Clippy: fe-git's unused import run_raw_with (remote.rs:30) and never-read Askpass.answer (askpass.rs:68) fail -D warnings off unix

GitHub CI's windows "Clippy" job (`cargo clippy --workspace --all-targets -- -D warnings`) fails on every main run (44252290199, 44280161517, 44312586576). It stops at fe-git with two errors that fire only off unix:

- crates/fe-git/src/remote.rs:30: unused import `run_raw_with`; its only use is inside the `#[cfg(unix)]` block near line 442.
- crates/fe-git/src/askpass.rs:68: field `Askpass.answer` is never read; its readers (line 363 on) are unix-only, since askpass is not used on Windows.

Fix: gate the import and the field (or the code using them) with `#[cfg(unix)]`, or a `cfg_attr(not(unix), allow(...))` with a comment saying why. Unix behaviour and the public API stay the same. No workflow edits.

The lint fires only on a Windows target, so the fix is proved by reading the cfgs, plus unix clippy and tests. After landing, the lead checks the next windows Clippy run; if it then stops at a later crate, those lints get new cards.
