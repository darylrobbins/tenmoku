---
task_uid: "01m4vwqpxkk6kg96zgxjzhy4vh"
task: "T711"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "selection_kept passes 20 of 20 runs while the workspace's heavy tests run in parallel"
    required: true
    manual: false
  - id: "C2"
    text: "If the panel itself could drop the selection mid-update, a regression test that fails at base pins the fix"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench and clippy -D warnings pass"
    required: true
    manual: false
---

# Flake: fe-workbench problems_panel selection_kept reads None for the kept selection after another file's problems clear

## Problem

`crates/fe-workbench/src/problems_panel_tests.rs:482`, `selection_kept`, failed T652's gate (run
01m4vvr0r5bsn3ptvk5cmep3ys, gate on 887baa68, 2026-10-14 ~00:12Z, under load): `left: None, right: Some(2)`
after `publish(&s, "src/lib.rs", Vec::new())` and `s.until("lib.rs cleared", |s| rows(s).len() == 3)`.
Neither T652 nor T657 touches the problems panel; the test passed 20 of 20 with nextest `--stress-count 20`
in T652's worktree at load 22-38.

Likely cause (gate-triage, unverified): a race, not a timing budget. `s.until(rows == 3)` can observe a
list that `set_files` built partway, before the selected problem was found again
(`crates/fe-workbench/src/problems_panel.rs`, around line 199); once the selection reads None the test
fails. Find out first whether the panel itself can lose the selection (a production bug: it then never
restores it) or only the test reads it too early.

## Change

- If the panel can drop the selection mid-update, keep it across `set_files` atomically (rows and the
  re-found selection published together), with a regression test that drives the interleaving.
- If only the test reads too early, wait on the condition the assertion needs (rows == 3 and the
  selection settled), not on the row count alone.

## Tests

A production fix needs a regression test that fails at base. A test-only fix: prove it with 20 runs while
the workspace's heavy tests run in parallel.

## Implementation notes

Test-only. `set_files` is synchronous and builds rows and selection together, so the panel can't drop the selection mid-update. The test's `until("the twins", rows.len() == 5)` was already true from setup's five rows, so `select_next` could run before the twins arrived; the twins then replaced the selected row (`E`) and the selection correctly became None. It now waits on the first twin's row.

C1 evidence (2026-10-14 14:39:31-14:41:44, this candidate): 20 sequential runs of
`cargo test -p fe-workbench --lib dock::problems_panel::problems_panel_tests::selection_kept`, each
`1 passed; 0 failed` (0.35-0.94 s), while `cargo test -p fe-vim -p fe-editor` and
`cargo test -p fe-workbench --lib` ran in parallel in the background; load average 30 at the start, 64 at the end.
