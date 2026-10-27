---
task_uid: "01m5wbfg38c0jtdnx1dk88fhmr"
task: "T362"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "focus_group_tests::focuses_the_nth_pane_in_layout_order: with panes split right then the second split down (three panes), pane.focus_group1, 2, 3 activate the panes in PaneGroup::panes() order and focus its active tab; pane.focus_group4 with only three panes does nothing (active pane and focus unchanged, no panic)"
    required: true
    manual: false
  - id: "C2"
    text: "focus_group_tests::single_pane_and_repeat: with one pane group1 keeps the active pane and moves keyboard focus back to its editor from a dock panel; running the same command twice is idempotent"
    required: true
    manual: false
  - id: "C3"
    text: "focus_group_tests::vim_and_find: focusing a group from an editor with vim on leaves vim mode and registers untouched; with the find bar open in another pane the bar stays open there"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap focus_group_keys: on macOS cmd-1 to cmd-4 and on Linux ctrl-1 to ctrl-4 resolve to pane.focus_group1 to 4 in the workbench and editor contexts; in the image viewer context cmd-1 still resolves to image.zoom_actual (the existing image_viewer_keys test passes unchanged) and cmd-1 in a plain editor is not an image command; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-gpui keymap_tests, fe-workbench default_keymaps); cargo nextest run -p fe-workbench -p fe-keymap -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Focus Editor Group 1-4 (cmd-1 to 4), as VS Code's workbench.action.focusFirstEditorGroup to focusFourthEditorGroup (Mac T1191)

fastedit has pane.focus_left/right/up/down and focus_next/previous but no way to jump to the nth editor group. VS Code binds cmd-1 .. cmd-8 on macOS (ctrl-1 .. ctrl-8 elsewhere) to workbench.action.focusFirstEditorGroup .. focusEighthEditorGroup; do the first four (a split rarely goes past four; the card names the pattern so a follow-up can extend it).

Add four typed commands next to pane.focus_next in workbench_commands.rs (the #[kiln::command(context = "workbench")] impl): pane.focus_group1 .. pane.focus_group4, titled "Focus First Editor Group" .. "Focus Fourth Editor Group". Each is a one-liner over a new Workbench::focus_group(n, window, cx) in the new module focus_group.rs: take self.pane_group(cx).panes().get(n - 1), and self.activate(pane, window, cx) exactly as focus_next_pane does (workbench_chrome.rs ~536); out of range does nothing. The order is the tree's order, left-to-right then top-to-bottom, as VS Code numbers groups by grid order. Do NOT edit workbench_chrome.rs or lib.rs: declare the new modules from workbench_commands.rs with #[path = "focus_group.rs"] mod focus_group; and #[cfg(test)] #[path = "focus_group_tests.rs"] mod focus_group_tests; like markdown_preview_buttons_tests there.

Keys: "secondary-1" .. "secondary-4", context "workbench", beside the pane.focus_* chords (~line 843). "secondary-1" is already image.zoom_actual in context "image..." (~line 2070): that binding is more specific and keeps winning in the image viewer; test it. In the editor with vim on, ctrl-1 on Linux is not a vim key, so no extra guard is needed; check that cmd-1 does not collide with the Open Editor at Index card's keys (ctrl-1 on macOS, alt-1 elsewhere): different modifier, but add an assertion if that card has merged.

Tests: headless Workbench via the closed_editors_tests.rs / close_tests.rs shape (Launch with a temp folder, run(id) through RunCommand); build the three-pane layout with pane.split_right and pane.split_down.

Lent from Mac T1191 (y75). Linux-OK, headless tests. Serialized behind T360 on shared keymap/catalog lists.
