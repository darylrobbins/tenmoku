---
task_uid: "01m7gvvepydwgqf39eqfynaywh"
task: "T665"
rev: 9
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "F11: a stopped frame's and a caller frame's band stay on their line through insertion, deletion, save and later unrelated refreshes (tests in src/tests/debug_gutter/frames_after_save.rs, a submodule of src/tests/debug_gutter.rs). Known limit: a caller re-selected after switching away (or clearing the selection) across an edit and save may land on the wrong row until the follow-up keeps caller anchors per stop epoch (review F6)"
    required: true
    manual: false
  - id: "C2"
    text: "F12: pending verification clears only for the awaited replacement request; delayed and failed replacements with intervening events for old and unrelated breakpoint ids keep it pending (tests in src/tests/debug_gutter/pending_verification.rs). Known limit: adapters that keep breakpoint ids across a successful replacement stay pending until the follow-up adds a request generation (review F1); the same limit covers a pending line whose earlier verification carried no adapter id (review F5)"
    required: true
    manual: false
  - id: "C3"
    text: "F15: the screen-reader regression test (in-crate, src/view/breakpoints_tests.rs) activates the breakpoint row through the accessibility Click handler that on_a11y_click registers on the rendered row, and fails if that registration helper or its use on the row is removed. Known limit: gpui registers on_a11y_action only with an active screen reader, so removing just that inner call isn't detectable headlessly (review F2)"
    required: true
    manual: false
---

# Debug gutter follow-ups (T1849 F11 F12 F14 F15 F16 F17): frames keep their place after a save and a split line, pending verification survives unrelated events, contrast on the hover tile, a keyboard-dismissable breakpoint menu, a11y click through the node (Mac T1881)

Review follow-ups of Mac T1849 (breakpoints over the line-number column), carried as known limits in its C3, C4 and C6 after review round 3.

- **F11 (medium)** crates/fe-workbench/src/debug_gutter.rs:413: Frame positions still snap back after saving edits. Insert a line above a stopped frame: its band follows correctly until save updates the store's disk baseline. The next refresh maps the original stop.row against that new baseline and returns the old numeric row. Caller frames have the same defect; the new test covers only unsaved edits.
  Fix: Retain frame anchors for the stop epoch, or map against an immutable source snapshot for that stop. Test both frames through insertion, deletion, save, and subsequent unrelated refreshes.

- **F12 (medium)** crates/fe-workbench/src/debug_gutter.rs:249: Any DebuggerEvent::Breakpoints clears all pending verification for the file, but D7 emits this event for individual adapter breakpoint changes too, not just replacement replies. After disabling and re-enabling a verified breakpoint with its replacement delayed or failed, a changed event for another breakpoint in that file restores the obsolete verified dot and relocated position.
  Fix: Clear invalidation only when verification belongs to the replacement request or breakpoint generation being awaited; distinguish replacement completion from individual adapter events. Add delayed and failed replacement tests with intervening events for both old and unrelated breakpoint IDs.

- **F15 (low)** crates/fe-gpui/tests/breakpoint_gutter.rs:362: The screen-reader regression test calls activate_breakpoint_row directly, bypassing the accessible node and its registered Click listener. It would still pass if the production on_a11y_action registration were removed, recreating F13.
  Fix: Exercise the rendered node's registered accessibility Click and assert the resulting breakpoint; ensure removing the action registration makes the test fail.

Regression tests in new files: crates/fe-workbench/src/tests/debug_gutter/frames_after_save.rs and crates/fe-workbench/src/tests/debug_gutter/pending_verification.rs, submodules of the existing test module crates/fe-workbench/src/tests/debug_gutter.rs (test ids tests::debug_gutter::<module>::<test>), and the F15 test in-crate in crates/fe-gpui/src/view/breakpoints_tests.rs (declared from breakpoints.rs) driven through the row node's accessibility Click handler.
- **F14 (medium)** crates/fe-config/tests/debug_tints.rs:285: The contrast test composites hover over the plain background, but rendering composites it over the active-line fill when that row is hovered. That combination still fails 3:1: enabled dots measure approximately 2.999 in Tenmoku Dark, 2.90 in Ember, 2.88 in Tenmoku Light, and 2.40 in Monokai; Tenmoku Light's frame arrow measures 2.96.
  Fix: Include fold_hover over active_line over background in the contrast assertions, then adjust the affected glyph colours or background treatment until every combination clears 3:1.

- **F16 (medium)** crates/fe-workbench/src/debug_gutter.rs:434: Frame bands anchor the entire line's text range. Inserting a newline inside a stopped or caller line expands its band across two buffer rows while its arrow remains on one. If no breakpoint row moves, the store emits no Moved event, and the editor observer ignores the text-version change, leaving the incorrect band until another refresh.
  Fix: Represent each frame band by its anchored row rather than an expanding text range, or rebuild it on text changes. Add tests splitting stopped and caller lines without moving any breakpoint rows.

- **F17 (medium)** crates/fe-gpui/src/view/breakpoints.rs:382: Opening the context menu explicitly focuses the editor, while the menu has no keyboard navigation or dismissal handling. Escape leaves it open, and arrows or Enter operate on the underlying editor; in insert mode, attempting to choose an item with Enter edits the file.
  Fix: Give the menu keyboard focus and handle navigation, activation and Escape, restoring editor focus on dismissal. Test these through dispatched keyboard events.

## Placement (helium respec, review round 2, F4)

The bugfix regression check runs criteria tests against base with only test files overlaid. A module declared by `#[path]` from src/debug_gutter.rs has 0 tests at base, and a changed crates/fe-gpui/tests/*.rs that calls new APIs fails to compile at base. So the two workbench tests are submodules of src/tests/debug_gutter.rs, and the fe-gpui tests that use new helpers are in-crate in src/view/breakpoints_tests.rs. This placement replaces the earlier `src/debug_gutter_*_tests.rs` beside debug_gutter.rs.

## Known limits (helium respec, review round 2)

- **F1 (C2):** pending verification clears on the awaited replacement for adapters that answer a replacement with new breakpoint ids. An adapter that keeps the same ids across a successful replacement leaves the re-enabled breakpoint pending, because identifying the awaited request needs a request generation from crates/fe-workbench/src/debug_session.rs, outside this card. Follow-up card: expose the setBreakpoints request generation from debug_session.rs and clear pending on it.
- **F2 (C3):** gpui registers `on_a11y_action` only while a screen reader is active, so a headless test can't observe the inner registration. The test drives the row through the click handler that `on_a11y_click` builds and registers, and fails if that helper or its use on the row is removed. Removing only the inner `on_a11y_action` call isn't detectable headlessly. Follow-up card: a gpui test seam that reports the node's registered a11y actions.

## Known limits (helium respec, review round 3)

- **F5 (C2):** a pending line whose earlier verification carried no adapter breakpoint id can't be told apart from the awaited replacement's answer, so an unrelated event can clear it and restore the obsolete verification. Telling them apart needs the same setBreakpoints request generation as F1, from crates/fe-workbench/src/debug_session.rs, outside this card. Follow-up card: Mac T1884 (request generation) covers ID-less prior verification too.
- **F6 (C1):** the anchor survives edits and saves for the caller frame that stays selected through them. Switching to another caller, or clearing the selection, and coming back after an edit and save maps that caller's disk row against the saved baseline, so its band can land on the wrong row within the same stop epoch. Follow-up card: keep caller anchors by frame location for the stop epoch (or map against an immutable stop snapshot), with a test that switches away and back.
