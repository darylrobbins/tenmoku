---
task_uid: "01m7nk7gbypmc4182754azdkqg"
task: "T1933"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::bottom_panel_groups::the_git_log_dragged_beside_the_terminal_shows_both passes: in a workbench with the terminal (fake sessions) and the Git log open in the bottom panel, dragging the Git log tab onto the right quarter of the terminal's content leaves both tool windows drawn side by side, the Terminal and Git log foot buttons both lit, and focus where the drop left it"
    required: true
    manual: false
  - id: "C2"
    text: "tests::bottom_panel_groups::a_lit_foot_button_hides_only_its_tool_and_brings_it_back_in_place passes: with the terminal left and the Git log right, clicking the lit Git log foot button collapses only its group (the terminal fills the panel, the panel stays shown, the Git log button unlit, focus that was in the Git log on the editor), and clicking it again shows the Git log back in the right group at its old width; with only one tool shown, its lit button hides the whole bottom panel as today; debug.show_panels with Debug shown beside another tool, and terminal.toggle with the terminal focused beside another tool, hide just that tool the same way"
    required: true
    manual: false
  - id: "C3"
    text: "tests::bottom_panel_groups::the_commands_split_move_join_and_focus_groups passes: from the palette's typed commands with focus in a bottom tool window, dock.split_panel_right makes a group after its own, dock.move_panel_to_previous_group and dock.move_panel_to_next_group move it between groups (an emptied group going away), dock.focus_next_panel_group focuses the next group's tool window and wraps, dock.join_panel_groups leaves one group; each does nothing where it can't apply"
    required: true
    manual: false
  - id: "C4"
    text: "tests::bottom_panel_groups::groups_come_back_with_the_window_layout passes: with the Git log right of the terminal, the saved window entry's terminal_dock has the groups; restored into a new workbench the Git log shows in the right group and the terminal is closed with no session but placed left, terminal.toggle showing it there; an entry saved before groups restores one group as now"
    required: true
    manual: false
  - id: "C5"
    text: "tests::bottom_panel_b3, tests::runs_panel, tests::debug_panel, dock_focus_tests, dock_leave_tests, dock_position_tests, zen_mode_tests and zen_mode_escape_tests pass unmodified, and typed_command_ids_tests and command_catalog_tests pass with the five new command ids added (cargo nextest run -p fe-workbench)"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench::l10n.ui_text_is_in_the_catalogue and fe-workbench::l10n.keys_used_here_exist pass with the new command titles"
    required: true
    manual: false
---

# Bottom panel groups in the workbench: drag the Git log beside the terminal, every shown tool's foot button lit, a lit button hiding just that tool, groups kept in the window layout, and Split Panel Right / Move Tool Window to Next or Previous Group / Join Panel Groups / Focus Next Panel Group

# Bottom panel groups in the workbench: Git log beside the terminal, kept in the layout, with commands

User request (2026-11-17): "Can we add support for dragging the panes to be side
by side so I can have like the Git graph beside a terminal, for example? I think we
need a bit more flexible placement of those panes."

kiln-shell's tabbed dock has groups (the card this one depends on: model, drag and
drop, divider, `DockState` save/restore). This card makes the workbench's bottom
panel (`terminal_dock`, `crates/fe-workbench/src/workbench.rs`) use them end to end.

## Change

- **Foot buttons**: today, in `workbench_chrome.rs` (`follow_terminal`), each rail
  foot button (Problems, Runs, Debug, Git log, Terminal) is lit while
  `rail_highlighted()` is its tool window. Light every tool window in
  `shown_panels()` instead, so with the Git log beside the terminal both buttons are
  lit.
- **Hiding one tool window (user decision, 2026-11-17)**: a click on a lit foot
  button (`bottom_button_clicked`) hides *just that tool window*, through kiln's
  `DockState::hide_panel` (card A). Its group collapses but keeps its place, and the
  other groups fill the panel. When it is the only tool window shown, the whole
  bottom panel hides, as today. A second click (its command) shows it back in its
  old group and place. Focus that was in it goes to the editor, as today. The same
  goes for the other toggles that hide a shown tool window:
  - `debug.show_panels` and `debug.show_console` (`debug_panel.rs`,
    `debug_panel/console.rs`), which ask `is_shown`, not `rail_highlighted`;
  - `terminal.toggle` while the terminal has focus (`toggle_terminal`).
- **Showing a tool window** (`show_bottom_panel`, `terminal.toggle`, `git.log`,
  `problems.show`, `runs.show`, Debug) shows it in its own group. The others stay
  shown, and focus goes where it goes today.
- **Window layout**: `terminal_dock_sizes` / `restore_terminal_dock_sizes`
  (`workbench_chrome.rs`) keep and restore the dock's `"groups"`, beside `"shown"`,
  per window entry as today. On restore:
  - Each group's saved active tool window shows again if it is restorable (Problems,
    Runs, the Git log).
  - The terminal keeps its group but comes back closed, with no session, as today.
    `terminal.toggle` later shows it in that group.
  - A layout saved before groups loads as one group, exactly as now.
- **Commands** (typed commands in `workbench_commands.rs`, titles in
  `l10n/en-US/commands.ftl`, no default keys, like the `dock_layout` commands; the
  palette lists them). Each acts on the bottom tool window that holds focus, else
  the bottom dock's focused group's active one. Each does nothing when it can't
  apply.
  - `dock.split_panel_right` "Split Panel Right": moves the tool window into a new
    group after its own. Its group must keep another open tab.
  - `dock.move_panel_to_next_group` "Move Tool Window to Next Group", and
    `dock.move_panel_to_previous_group` "Move Tool Window to Previous Group". The
    tool window becomes active there, and an emptied group goes away.
  - `dock.join_panel_groups` "Join Panel Groups": one group again.
  - `dock.focus_next_panel_group` "Focus Next Panel Group": focuses the next drawn
    group's active tool window, wrapping.

  They keep the dock visible and keep focus on the tool window they moved, so the
  whole feature works without a mouse.
- **Focus**: `focus_bottom_panel` and vim's `<C-w>j` into the bottom panel focus the
  focused group's tool window (no change needed if `rail_highlighted` keeps its
  meaning). `leave_hidden_panel` relies on kiln's `hidden_panel_focused`, which now
  counts every drawn group.
- **Accessibility**: the groups and the divider are named by kiln. The commands have
  catalogue titles, so the palette and VoiceOver read them.

## Not in this card

- Moving tool windows into the sidebar, or sidebar panels into the bottom panel.
- Vim's `<C-w>h/l` between groups: its own card.
- A tab context menu.
- An audit scene for a split panel.

## Pins

`tests::bottom_panel_b3`, `tests::runs_panel`, `tests::debug_panel`, `dock_focus_tests`,
`dock_leave_tests`, `dock_position_tests`, `zen_mode_tests` and `zen_mode_escape_tests` must pass unmodified:
with one group nothing changes. `typed_command_ids_tests.rs` and
`command_catalog_tests.rs` get the five new ids. No screenshot golden should move;
if one does, block for a Mac regeneration rather than editing goldens. New tests go
in the new `crates/fe-workbench/src/tests/bottom_panel_groups.rs`, declared from
`crates/fe-workbench/src/tests/mod.rs`.

Depends on T1898, which holds `tests/mod.rs`, `commands.ftl`,
`typed_command_ids_tests.rs`, `command_catalog_tests.rs` and `l10n-allow.toml`.

## Implementation notes

- The workbench uses kiln's `Dock` unchanged: dragging a tab, its drop
  indicator and the `HidFocus` after a drop all come from kiln. C1 needed
  only the foot buttons lit from `shown_panels()`.
- `Workbench::hide_bottom_tool` is the shared hide. The foot buttons,
  `debug.show_panels` and `debug.show_console` all call it, through
  `DockState::hide_panel`. Focus goes to the editor only when it was in
  the bottom panel and `hidden_panel_focused` now holds. Focus in a group
  that stays shown is left where it is. `terminal.toggle` calls
  `hide_panel(TERMINAL)` too, and falls back to hiding the dock.
- The commands act on the "tool window in play": the shown bottom tool
  window that holds focus, else `rail_highlighted()`. Nothing happens
  while the dock is hidden. "Next" and "previous" group count drawn
  groups only, so a collapsed group, or one whose tabs are all closed, is
  skipped. Split needs a second open tab in the group, Join needs more
  than one group, and Focus Next needs two drawn groups. Each command
  focuses the tool window it moved or reached.
- Window layout: `terminal_dock` keeps kiln's `"groups"` as saved. On
  restore, `restored_groups` turns each group's `"active"` into null
  unless it is Problems, Runs or the Git log. With a null active, the
  terminal's group has no tab, so it isn't drawn but keeps its place. The
  dock is restored shown, with the saved `"shown"` (else the first
  restorable active of a group that is not collapsed) as the top-level
  `"active"`, so that group is the focused one. Without `"groups"`, the
  old path runs unchanged.
- The command palette sorts matches by title, with no scoring. A
  "split right" query therefore lists "Split Panel Right" before "Split
  Right". `palette_tests::the_palette_runs_a_command_in_the_editor` now
  presses down once. `palette_tests.rs` was added to the scope (respec).
- Carried findings in `debug_panel/console.rs`, fixed:
  - T1853 F1: output that evicts the oldest entries while the reader is
    scrolled up moves the list's offset up by the rows that went
    (`CODE_SMALL.line_height` each; `last_item_size.item` is the
    viewport, not a row). The regression test is
    `debug_panel::console::retention_tests`.
  - T1853 F2: an evaluation answered in the stop shown calls the new
    `DebugPanel::refresh_stop_data` (`debug_panel/stop.rs`, added to the
    scope). It bumps `Stop::generation`, clears the cached children,
    asks for every watch again (each keeps its value until the new one
    comes) and fetches what is shown expanded. Replies to `variables` and
    watch requests from an older generation land nothing. The regression
    is `tests::debug_console_refresh` (new file, added to the scope). A
    `setVariable` sent straight to the fake stands in for an
    evaluation's side effect, because the fake's `evaluate` changes
    nothing.
