---
task_uid: "01m4sv2qf0dntr0v67yxbxqyes"
task: "T623"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The cause of the empty highlights is named in the decision file, and the test no longer reads results before the work they depend on is drawn: whatever it checks, it checks after a draw that follows the text it needs. Polling for a real pty's output with a bounded timeout, as the helper did before, is allowed (lead decision 2026-10-13); no new sleeps are added"
    required: true
    manual: false
  - id: "C2"
    text: "toggles_and_errors passes 30 runs in a row while every core is loaded, with the command and result in the decision file"
    required: true
    manual: false
  - id: "C3"
    text: "Only crates/fe-workbench/src/terminal_find_tests.rs changes, except this lane's own spec notes"
    required: true
    manual: false
---

# Flake: fe-workbench terminal_find_tests::toggles_and_errors sees no highlights in a new terminal that took the query

fe-workbench terminal_panel::terminal_find_tests::toggles_and_errors failed once on Mac main 62470f56 in the post-merge check (run 01m4stq19f7d4ns5z64gqmwe26, 0.27 s): at crates/fe-workbench/src/terminal_find_tests.rs:275, `assert_eq!(highlighted(&second, cx), [1, 2])` saw `[]`. The same commit passed it in the previous post-merge run (01m4st46bnhqfyc838z45jpvys) and in T601's gate, so the outcome depends on timing. That failure froze the Mac repo and reverted T601, which was innocent.

The step creates a new terminal (`terminal.new`), which takes the find query, and expects its matches highlighted at once. The likely race: the second terminal's printed text, or its highlight pass, isn't ready by the time the test reads it (an off-thread search, a PTY read, or an executor tick not run). Find the cause:
- If the test reads before work it should wait for (it doesn't run the executor until idle, or doesn't wait for the printed text), fix the test so it waits deterministically. That makes this a test-only chore.
- If production code can leave a new terminal with a query but no highlights (a real race), stop and ask the lead to respec this as a bugfix with a regression test, rather than widening the scope yourself.

Prove it: run the test 30 times in a row while every core is loaded (for example several `yes > /dev/null`), and record the command and the result in the decision file.

## Implementation notes

**Cause (test-only).** `TerminalView::find_matches` is recomputed only in the element's prepaint, from the snapshot at that draw. The test helper `printed` drew, *then* checked the snapshot for `end`; when the shell's output landed between that draw and the check, it returned with matches computed from a partial screen (here none), so `highlighted(&second)` saw `[]`. Production is fine: the next draw highlights the text. Fix: `printed` draws once more after seeing the text. That is a draw, not a sleep; the existing poll that waits for the pty's output is unchanged.

**Proof (C2).** 14 cores loaded with `yes > /dev/null` (8 + 14 processes):
`for i in $(seq 1 30); do cargo test -q -p fe-workbench --lib toggles_and_errors; done` → passed 30/30.
