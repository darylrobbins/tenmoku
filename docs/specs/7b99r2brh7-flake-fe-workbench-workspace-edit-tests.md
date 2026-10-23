---
task_uid: "01m5jzztjgejbqfy7b99r2brh7"
task: "T230"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "an_edit_from_an_ended_session passes 500 times in a row under load 30+, still asserting the failed answer 'the server's session ended' and an empty queue; the root cause (production ordering or test race) is named in the commit message"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench -p fe-lsp passes and cargo clippy -p fe-workbench -p fe-lsp --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Flake: fe-workbench workspace_edit::tests::an_edit_from_an_ended_session gets no answer to workspace/applyEdit in 120 s under load (found by the mac/main 610008ff sync gate)

Flake found by argon's sync gate of mac/main 610008ff (merge 53116d3a); filed by the argon lead.

## Failure
```
FAIL [120.351s] fe-workbench workspace_edit::tests::an_edit_from_an_ended_session
thread '<unnamed>' panicked at crates/fe-workbench/src/workspace_edit_tests.rs:85:18:
called `Result::unwrap()` on an `Err` value: FakeError { code: None, message: "no answer to workspace/applyEdit in 120s" }
```
Run log: ~/.local/state/regatta/runs/01m5jykds2gh1e7vm0m7519dft/test.fastedit.log (full workspace test under load; three other tests ran SLOW at the same time). Alone, on the same commit, it passes in 0.135 s.

## What the test does
A server (`answer_shutdown: false`, on a clock that doesn't move) pushes `workspace/applyEdit` from a thread (`push_edit`, 120 s request timeout). The test waits until `lsp_queued() != 0` (the edit reached the workbench), then `lsp_stop(server)`, and expects the thread to be answered `{applied: false, failureReason: "the server's session ended"}`. Under load the answer never came in 120 s, so either the queued edit is dropped/answered on a path that can be skipped (a REAL ordering bug in the stop / queued-edit handling, then fix production, and respec this card to `bugfix` with a regression test), or the test races (e.g. `lsp_queued` seen before the edit is tied to that session/incarnation, so `lsp_stop` doesn't end it).

## Work
Reproduce under load (stress-ng, or `cargo nextest run -p fe-workbench -E 'test(an_edit_from_an_ended_session)'` in a parallel loop with the machine loaded to 30+; use `--stress-count` or a loop of at least 500). Name the cause from the order of events (the edit queued, `lsp_stop`, session end, fe-lsp's failed answer). Fix the cause; don't loosen the assertion and don't just raise a timeout. Files: `crates/fe-workbench/src/workspace_edit_tests.rs`, `crates/fe-workbench/src/workspace_edit.rs`, `crates/fe-workbench/src/lsp.rs`, `crates/fe-lsp/src/manager.rs` only if production needs the fix.

## Implementation notes

Cause: `serve` ranks `Stop` above `ApplyEdits`, so an edit queued in `Edits` but not yet taken when the stop arrived was never answered, and the router (still reading during shutdown) queued later edits for a gone actor; only the 5 s timeout on a clock that never moves would answer them. Fix: `end_session` calls `Edits::fail_all`, which answers the queue failed and closes it; `ask` on a closed queue answers at once.

Validation: before the second half of the fix a 200-iteration stress failed at iteration 4 (120 s). After it, `cargo nextest run -p fe-workbench -E 'test(an_edit_from_an_ended_session)' --stress-count 500 --no-fail-fast` with 40 busy-loop processes on 12 cores (load average about 40): 500/500 passed (817 s). Unit test `apply_edits_queued_or_asked_after_the_session_ended_are_failed` covers both new paths.
