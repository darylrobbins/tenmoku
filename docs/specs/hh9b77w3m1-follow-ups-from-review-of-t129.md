---
task_uid: "01m4ebvs7708ryh5hh9b77w3m1"
task: "T200"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Picking a symbol from the breadcrumb symbol picker while a go-to-definition answer is held (nav_state.hold_answer), then releasing the answer, leaves the caret on the picked symbol; back returns to the caret before the pick and forward to the picked symbol; the held answer records nothing. Tested in crates/fe-workbench/src/tests/toolbar_history/goto.rs."
    required: true
    manual: false
  - id: "C2"
    text: "The same holds for a breadcrumb entry pick that reopens the file already shown in the pane (same editor kept): the held go-to-definition answer, released after the pick, neither moves the caret nor pushes history. Tested in the same new file."
    required: true
    manual: false
  - id: "C3"
    text: "The fix is in crates/fe-workbench/src/toolbar_picker.rs only; tests/toolbar_history.rs gains only the mod line; existing toolbar_history and nav tests pass unchanged."
    required: true
    manual: false
---

# Follow-ups from review of T129

Findings of the review of T129 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/toolbar_picker.rs:348: Breadcrumb jumps record history without cancelling an outstanding go-to-definition request. If F12 is pending when the user picks a sibling symbol, the editor identity remains unchanged, so the old answer still lands, overrides the selected symbol, and records the original F12 position again instead of the breadcrumb destination. The existing nav_jump helper explicitly cancels this request before recording history; the new picker helper omits that step. Entry picks retaining the same editor have the same problem.
  fix: Cancel the pending goto request in the shared breadcrumb-jump helper before pushing history. Add a regression test using hold_answer: start F12, choose a sibling symbol, release the answer, and verify that the selected symbol and its back/forward history remain intact.
  instances: crates/fe-workbench/src/toolbar_picker.rs:348, crates/fe-workbench/src/toolbar_picker.rs:383, crates/fe-workbench/src/toolbar_picker.rs:428

Still open on main (86d226d): `ToolbarPicker`'s `push_history` (toolbar_picker.rs:346) calls `Workbench::push_history` only; `Workbench::nav_jump` (nav.rs:566) calls `self.nav_state.goto.next()` first, which cancels a running go-to-definition.

## Fix

In crates/fe-workbench/src/toolbar_picker.rs only: the picker's shared jump helper cancels a running go-to-definition (`workbench.nav_state.goto.next()`, as `nav_jump` does) before it records history, so both `pick_symbol` and `pick_entry` get it. Don't change nav.rs (T144 holds it) or tests/mod.rs (T143 holds it).

## Tests

New file crates/fe-workbench/src/tests/toolbar_history/goto.rs, declared by one `mod goto;` line added to crates/fe-workbench/src/tests/toolbar_history.rs (no other change there), so it can use toolbar_history's fixture (`project`, `MAIN`, crumb clicks) via `super::`. Pattern: tests/nav.rs `a_picker_jump_drops_a_running_goto_definition` (hold `nav_state.hold_answer`, F12, pick, release, settle).
