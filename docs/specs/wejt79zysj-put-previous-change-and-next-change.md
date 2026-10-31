---
task_uid: "01m67g1h0brkwrfywejt79zysj"
task: "T379"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test diff_view_change_buttons_tests::buttons_move: clicking next from the top lands the caret on the first hunk, again on the second; previous goes back; the result equals pressing f7 / shift-f7"
    required: true
    manual: false
  - id: "C2"
    text: "diff_view_change_buttons_tests::count: with 3 hunks the label reads 'Change 1 of 3' on the first and updates after a move, after an edit that merges two hunks, and in inline mode"
    required: true
    manual: false
  - id: "C3"
    text: "diff_view_change_buttons_tests::edges: no hunks disables both buttons and shows no count; at the last hunk next does what f7 does today (wrap or stay)"
    required: true
    manual: false
  - id: "C4"
    text: "buttons use the existing icons and tooltips that name their keys; cargo test -p fe-gpui passes; clippy clean"
    required: true
    manual: false
---

# Put previous-change and next-change buttons with a 'Change 2 of 5' count in a DiffView's tab toolbar, wired to diff.previous_change and diff.next_change (Mac T1219)

## Problem
The user: "Maybe a button to jump between changes?" The commands exist (diff_view.rs NEXT_CHANGE, f7/shift-f7, T660) but nothing on screen invokes them. VS Code's editor-title bar has up/down arrows.
## Change
Add two icon buttons and a count in the DiffView's toolbar/tab chrome (look at how pane_toolbar items attach). Same file as T1218, so this is chained after it.
