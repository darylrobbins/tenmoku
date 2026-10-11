---
task_uid: "01m4nrmksnyby84v7xw9867ya7"
task: "T36"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "assert_one_document waits (polls up to 10 s, or uses the fake's wait_for) before asserting the last launch has the document open once; its assertions are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "The lsp_aliases_reroute tests pass 3 consecutive runs, with the 1-minute load average logged before each run"
    required: true
    manual: false
  - id: "C3"
    text: "Of the worker's own changes, only crates/fe-workbench/src/tests/lsp_aliases_reroute.rs changes; Regatta's spec file in docs/specs/ is not the worker's change"
    required: true
    manual: false
---

# Flaky under load: fe-workbench tests::lsp_aliases_reroute::a_settings_change_that_merges_routes_leaves_one_document: wait for the didOpen before counting, like T23's pyright_open

## Problem

`fe-workbench tests::lsp_aliases_reroute::a_settings_change_that_merges_routes_leaves_one_document` failed in the daemon's check of main 085ec4e (run 01m4nr6ww4ps5j7gevrx2381q8, 1-min load ~30), and passed in a clean gate of the same commit:

    panicked at crates/fe-workbench/src/tests/lsp_aliases_reroute.rs:75:5:
    assertion `left == right` failed: the last launch has file:///tmp/.../src/main.rs open once
      left: 0
     right: 1

`assert_one_document` (line ~67) counts the fake server's didOpen records for its last launch right after `settle()`. The fake records on its own thread, so under load the didOpen may not be recorded yet. This is the same race T9, T28 and T23 fixed (T23 made `pyright_open` in this file poll for up to 10 s).

## Change

In `crates/fe-workbench/src/tests/lsp_aliases_reroute.rs` only: make `assert_one_document` wait, like `pyright_open` does (poll up to 10 s until the last launch has the document open once, then assert as now), or use the fake's `wait_for`. The checks stay the same: no launch ever had the document open twice at once, and the last launch has it open exactly once. All four callers keep their assertions.
