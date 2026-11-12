---
task_uid: "01m78dywjm0zryx23jwedkda86"
task: "T333"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::git_commit_box_size (cargo nextest run -p fe-workbench) pass: dragging_the_handle_grows_the_box_and_shrinks_the_list, a_long_message_scrolls_inside_the_box_past_the_cap, the_height_is_remembered_per_window, a_short_panel_keeps_the_box_and_a_usable_list, the_default_is_unchanged_at_three_rows"
    required: true
    manual: false
  - id: "C2"
    text: "git_commit_box_tests, git_panel_tests and git_panel_groups_tests still pass; the audit changes scene probes (180,125 and 174,500) still hit surface.background and surface.panel, else move them"
    required: true
    manual: false
---

# Changes panel: the commit message box can be made taller, and the panel scrolls when the box and list don't fit (Mac T1749)

# Commit box height

User report (2026-11-12): "In the changes view, the message, I can't scroll down and make more room for it."

`render_commit_box` (git_panel.rs ~:1715) sizes the box 3..BOX_ROWS (10) rows from the text (`box_rows` ~:1926) and soft-wraps; the list below takes the rest. Do both readings of the report:
1. A drag handle on the box's bottom edge (kiln-ui resize affordance, keyboard-adjustable, accessible name) sets its height in rows, up to the panel height less a minimum list height, remembered per window (state, not settings). Past the height, the text scrolls inside the box; check that wheel/trackpad scroll reaches the editor and is not eaten by the panel.
2. If the panel is shorter than box + list minimum, the whole panel scrolls instead of clipping the box.

Leave the box as the plain EditorView with a trailing slot in its footer row (beside the counter), so T1592's later "Draft from diff" button (AI commit message, later) can sit there without a layout change. T1591 (ready) adds a checks block above the box in this same file.

Platform: Linux-OK.
