---
task_uid: "01m48tkzgb26h8brd202edq401"
task: "T101"
rev: 2
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "F1: after the window deactivates and reactivates with the palette open, typing and cmd-p work without a click (test in tests::quick_open_races)"
    required: true
    manual: false
  - id: "C2"
    text: "F2: typing a different query and pressing enter before its background query finishes never opens the previous query's file; a rejected confirmation keeps the picker open; reopening shows no stale rows (tests without draining the executor between type and enter)"
    required: true
    manual: false
  - id: "C3"
    text: "F3: a mouse-down, result reorder, mouse-up sequence opens the pressed file or nothing, never a different file (test in tests::quick_open_races)"
    required: true
    manual: false
  - id: "C4"
    text: "New tests live in crates/fe-workbench/src/tests/quick_open_races.rs and fail at base"
    required: true
    manual: false
---

# Follow-ups from review of T76

Findings of the review of T76 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/workbench.rs:131: Window deactivation triggers GPUI's on_blur even when the input retains its focus handle. This handler removes the palette without restoring focus, leaving focus on an unrendered input when the user returns. Typing and Cmd-P then fail until the user clicks another element.
  fix: When hiding the palette, restore the previous focus or a workbench fallback if focus still belongs to the picker; preserve focus when it has moved elsewhere. Add a deactivate/reactivate test that verifies typing and Cmd-P still work.
  instances: crates/fe-workbench/src/workbench.rs:131

- F2 medium crates/fe-workbench/src/quick_open.rs:280: Changing the query retains the previous matches, and confirmation never checks whether shown matches belong to the current input. Typing a different filename and pressing Enter before its background query finishes opens the previous query's selected file. Reopening also retains old rows while the empty query runs. Existing tests drain the executor before confirming, masking this transition.
  fix: Invalidate confirmable matches synchronously when the query changes, including reopening, or defer confirmation until current-query results arrive. Ensure a rejected confirmation does not dismiss the picker, and test typing followed immediately by Enter without draining the executor.
  instances: crates/fe-workbench/src/quick_open.rs:280, crates/fe-workbench/src/quick_open.rs:300, crates/fe-workbench/src/picker.rs:91, crates/fe-workbench/src/picker.rs:115

- F3 medium crates/fe-workbench/src/picker.rs:157: Rows use their position as their element identity, and click handlers confirm that position in the current matches. If indexing or a watcher refresh reorders results between mouse-down and mouse-up, GPUI preserves the pending click at that position and opens a different file from the one pressed. This also occurs with an unchanged query.
  fix: Bind click activation to a stable match identity or result generation, cancelling activation when its rendered snapshot is obsolete. Add a mouse-down, result-reorder, mouse-up regression test.
  instances: crates/fe-workbench/src/picker.rs:148, crates/fe-workbench/src/picker.rs:157
