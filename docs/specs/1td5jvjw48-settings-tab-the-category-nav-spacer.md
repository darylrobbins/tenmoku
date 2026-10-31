---
task_uid: "01m66qbtedpxved41td5jvjw48"
task: "T1247"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "settings_editor::layout_tests regression (named at respec): after nav to a section, narrowing and shortening the window and scrolling to the end, setting rows are visible (no blank list); fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo test -p fe-workbench settings_editor passes; cargo clippy -p fe-workbench clean"
    required: true
    manual: false
---

# Settings tab: the category-nav spacer survives a narrow-and-short resize and can leave the list blank after scrolling to the end (T1237 F5)

Follow-up to T1237 review finding F5 (codex, medium).

The Settings tab's category-nav spacer (crates/fe-workbench/src/settings_editor_layout.rs ~line 442) is recalculated only when both endpoint rows have bounds. Narrowing and shortening the window invalidates GPUI's cached row heights; when the expanded section is taller than the viewport plus overdraw, the last row stays unmeasured and the old spacer survives. After scrolling past the anchor, bounds_for_item(anchor) returns None, so it never recovers.

Repro: click a nav entry (e.g. Workbench) in a large window so the spacer is added, resize to a narrow, short window, scroll to the end: the retained spacer can exceed the viewport and leave the list blank.

Fix: invalidate or conservatively clear stale spacer geometry when the viewport size or row measurements change, without requiring both endpoint bounds.

Regression test: in settings_editor_layout_tests.rs (declared at base once T1237 lands), navigate to a section, narrow and shorten the window, scroll to the end, and check setting rows are visible, driven by application-requested frames. It must fail at base.
