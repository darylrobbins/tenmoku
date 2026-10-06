---
task_uid: "01m49a5d8zzgv57wfgyb8ttccc"
task: "T133"
rev: 3
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "F1: after an edit in one pane of a split highlighted (JSON) file, the other pane's view shows highlighting on the first redraw without forcing another frame; proven by tests::splits_followups::shared_highlighted_view_is_highlighted_on_first_redraw."
    required: true
    manual: false
  - id: "C2"
    text: "F2: the launch read opens in its originating pane even after that pane opened another file or activated a tab, and is discarded if that pane has closed; proven by tests::splits_followups::launch_read_goes_to_its_pane_after_its_tab_changes and tests::splits_followups::launch_read_is_dropped_when_its_pane_closed."
    required: true
    manual: false
---

# Follow-ups from review of T79

Follow-up from the review of T79 (editor pane splits).

Dropped: review F3 (T79's non-test diff over the 1,000-line cap) is moot now T79 has merged.

## F1 (medium): shared-view notifications don't catch the receiving model up

Still true on main: `Workbench::editor_changed` (workbench.rs ~l.429) only `cx.notify()`s the other editors sharing the buffer. `EditorView::render` then syncs syntax against the stale model snapshot, `EditorElement::prepaint` catches the model up and rejects the now-stale syntax, so a split highlighted (e.g. JSON) file's other pane is unhighlighted until another frame.

Fix: in `editor_changed`, catch each receiving model up (`model_mut().sync_display()`) before notifying it.

## F2 (medium): launch-read ownership is inferred from the loading indicator

Still true on main: `Workbench::file_read` (workbench.rs ~l.496) picks the pane by `is_opening(&path)`, else the active pane. If the original pane opens another file or activates a tab before the launch read finishes, the result goes to whichever pane is active; if the original pane closed, the late result reopens the file elsewhere.

Fix: `Workbench::new` records the launch read's originating pane (weakly), independently of its loading state; `file_read` delivers to it, and discards the result if that pane has closed (no longer in the pane group).

Tests go in a NEW file, crates/fe-workbench/src/tests/splits_followups.rs, registered in tests/mod.rs (don't add to splits.rs).
