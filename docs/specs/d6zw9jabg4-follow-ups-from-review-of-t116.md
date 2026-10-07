---
task_uid: "01m49q0xvb33kkpfd6zw9jabg4"
task: "T149"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "F1: moving the pointer from one unselected project-tree row to another, and then out of the list, updates each row's hover highlight without any unrelated redraw. The hover-styled element has a stable ID scoped under the row, or its background is driven by a hover group on the identified row. Outline rows share fe-ui's TreeRow and get the same fix"
    required: true
    manual: false
  - id: "C2"
    text: "New tests live in crates/fe-workbench/src/project_panel_hover_tests.rs, declared from project_panel.rs by a #[cfg(test)] #[path] mod line, and fail at base. If the stale highlight can't be observed headlessly, a manual note says why"
    required: true
    manual: false
---

# Follow-ups from review of T116

Findings of the review of T116 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-ui/src/tree_row.rs:159: The hover background is attached to a selection div without an element ID. GPUI 0.3.7 retains hover state and requests redraws on entry/exit only for identified elements. Moving between unselected rows therefore fails to update their highlights; unrelated redraws can display a highlight that subsequently remains stale. This affects both project and outline rows, and the new tests do not exercise hover transitions.
  fix: Give the selection div a stable ID scoped beneath the row, or drive its background through a hover group on the identified outer row. Add a regression test that moves between rows and outside the list, verifying highlights update without forcing a redraw.
  instances: crates/fe-ui/src/tree_row.rs:159

## Lead notes

All findings checked on main 4037dd1: F1 still holds (the selection div at tree_row.rs:149-162 has no element ID and carries the hover style).

Scope is narrowed to the fix and one new test file. The test file is declared from project_panel.rs by `#[cfg(test)] #[path = "project_panel_hover_tests.rs"] mod hover_tests;` (project_panel.rs changes only by that line). It is not declared from crates/fe-workbench/src/tests/mod.rs, which the active T123 and most ready workbench cards hold. Tests there can't use tests/mod.rs's private helpers, so build the small headless-workbench harness locally. fe-ui has no gpui test-support dev-dependency, so the hover test goes through the project panel's rows (`ProjectPanel::row_bounds`). theme.rs, lib.rs, picker.rs and search_panel.rs were dropped from scope because the fix doesn't need them.
