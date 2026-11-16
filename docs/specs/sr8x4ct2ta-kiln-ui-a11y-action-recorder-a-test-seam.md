---
task_uid: "01m7hpdtdjbsv8xasr8x4ct2ta"
task: "T666"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Under test-support, kiln-ui records each a11y action registered through its helper per rendered node id, reports them, and performs one by running its listener (test in kiln/crates/kiln-ui/src/tests/a11y_actions.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "a_screen_readers_click_on_a_row_toggles_it asserts the drawn row's node has Click registered and toggles through the recorded listener, failing if the registration in view/breakpoints.rs is removed"
    required: true
    manual: false
---

# kiln-ui a11y action recorder: a test seam that reports a rendered node's registered a11y actions and performs them, and the breakpoint row test asserts its Click through it (T1881 F2) (Mac T1885)

Review follow-up of Mac T1881 (helium T665), from T1849 F15 (T1881 C3).

**F2 (medium, test infra)** GPUI registers an element's `on_a11y_action` listeners with the window only while a screen reader is active (gpui-pre 0.3.9 `elements/div.rs`, the `if window.a11y.is_active()` block in paint that calls `Window::on_a11y_action`). The test platform never activates accessibility (see crates/fe-workbench/tests/a11y_tree_dump.rs), so a headless test can't check that the breakpoint row registered its Click: crates/fe-gpui/tests/breakpoint_gutter.rs calls `activate_breakpoint_row` directly, and stays green if the `.on_a11y_action(AccessibleAction::Click, ...)` in crates/fe-gpui/src/view/breakpoints.rs is removed.

GPUI comes from crates.io (`gpui-pre =0.3.9`); it isn't vendored, and [patch.crates-io] covers only tree-sitter. So the seam goes in kiln-ui, beside the existing node recorder:
- kiln/crates/kiln-ui/src/a11y.rs: an `on_action` helper (re-exported as `fe_ui::on_action`) that calls the element's `on_a11y_action` and, under `cfg(any(test, feature = "test-support"))`, records the action against the element id in the window's recorder, keeping the listener. `actions(window, cx)` reports each rendered node's registered actions, and `perform(window, id, action, cx)` runs that recorded listener, as a screen reader's request would. Records follow the recorder's existing per-draw lifecycle. The release build only calls `on_a11y_action`.
- kiln/crates/kiln-ui/src/lib.rs re-exports them. A kiln-ui test in a new kiln/crates/kiln-ui/src/tests/a11y_actions.rs, declared from src/tests/mod.rs, covers recording, replacement on redraw and `perform`.
- crates/fe-gpui/src/view/breakpoints.rs registers the row's Click through `fe_ui::on_action`. In crates/fe-gpui/tests/breakpoint_gutter.rs the screen-reader test finds the drawn row's node, asserts that Click is among its actions, and toggles through `perform`, not `activate_breakpoint_row`.

The other two `on_a11y_action` users (crates/fe-gpui/src/element.rs and crates/fe-gpui/src/file_viewer/element.rs) stay as they are.

## Implementation notes

- The screen-reader test lives in `crates/fe-gpui/src/view/breakpoints_tests.rs` (in-crate), not `tests/breakpoint_gutter.rs`; scope was respecced to include it.
- T665 F7 (menu focus target / IME composition) is not addressed: it needs a separate focus design and is unrelated to the recorder seam; left for a follow-up card.
