---
task_uid: "01m7ppgerkktv5rygj756vt625"
task: "T422"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "dock_transfer::a_panel_moved_between_docks_keeps_its_entity_and_state passes: take_panel from one DockState and insert_panel into another keep the same entity, focus handle and save_state; the source no longer lists or draws it, the target does; remove_action takes a rail action off"
    required: true
    manual: false
  - id: "C2"
    text: "dock_transfer::drags_cross_only_between_docks_of_one_transfer_group passes: a tabbed dock's tab dragged over a side dock's column or rail top group of the same transfer group draws the drop indicator and its drop calls panel_dropped with the panel, source and target; a side rail button dragged onto a tabbed dock's group zones or a side rail's foot group is offered the same way; docks without a shared group ignore each other's drags"
    required: true
    manual: false
  - id: "C3"
    text: "tests::tool_window_placement::the_git_log_dragged_onto_the_sidebar_becomes_a_sidebar_panel passes: dragging the Git log tab onto the sidebar shows it there under its heading, focused, with a rail top-group button and no foot button; git.log then shows it in the sidebar; the bottom panel's other tools are unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "tests::tool_window_placement::the_move_commands_work_both_ways passes: dock.move_tool_window_to_sidebar with Problems focused moves it to the sidebar; dock.move_tool_window_to_bottom_panel with Search focused puts it in the bottom panel's focused group with a lit foot button, search.show then showing it there; the Explorer is not moved and each command does nothing where it can't apply"
    required: true
    manual: false
  - id: "C5"
    text: "tests::tool_window_placement::placements_come_back_with_the_window_layout passes: the saved entry has placements for moved panels only; a new workbench restores them before the docks' state (the moved Search active in its saved group, the Git log active in the sidebar); an entry without placements restores today's places"
    required: true
    manual: false
  - id: "C6"
    text: "tests::tool_window_placement::focus_and_window_keys_follow_a_moved_tool_window passes: with Problems in the sidebar and Search in the bottom panel, focus_bottom_panel, bottom_panel_focused, leave_hidden_panel, dock.grow and vim's \u003cC-w\u003e keys treat each by the dock now holding it"
    required: true
    manual: false
  - id: "C7"
    text: "tests::bottom_panel_b3, tests::bottom_panel_groups, tests::activity_rail, tests::rail_b3, tests::sidebar_b3, tests::runs_panel, tests::debug_panel, dock_leave_tests, dock_focus_tests, and kiln-shell's dock and dock_groups tests pass unmodified; typed_command_ids_tests and command_catalog_tests pass with the two new ids (cargo nextest run -p kiln-shell -p fe-workbench)"
    required: true
    manual: false
  - id: "C8"
    text: "fe-workbench::l10n.ui_text_is_in_the_catalogue, fe-workbench::l10n.keys_used_here_exist, kiln-shell::it.l10n::ui_text_is_in_the_catalogue and kiln-shell::it.l10n::keys_used_here_exist pass"
    required: true
    manual: false
---

# Move a tool window between the bottom panel and the sidebar, by dragging a tab onto the sidebar or rail and a rail button onto the bottom panel, or by Move Tool Window to Sidebar / Bottom Panel, the rail following placement, kept in the window layout (Mac T1935)

# Move a tool window between the bottom panel and the sidebar, by drag or command, kept in the layout

Follow-up to the user's 2026-11-17 request ("I think we need a bit more flexible
placement of those panes"), after the bottom panel groups (cards A and B) and
their vim keys (card D).

## Today

There are two docks, with different models:

- **The sidebar** is fe-workbench's `Dock` (`crates/fe-workbench/src/dock.rs`)
  around a side `DockState`. It is not tabbed and shows one panel at a time under a
  heading. Its rail's top group is the sidebar's own panels (Explorer, Search,
  Changes, Agents, Chat, then References, History, Call Hierarchy and Quickfix,
  which join the rail on first show).
- **The rail's foot** is a fixed list of `RailAction`s (`rail_actions()`: Problems,
  Runs, Git log, Terminal, plus Debug through `join_debug`). They run show commands
  for the tool windows of the bottom panel, which is `terminal_dock`, a tabbed
  `kiln_shell::Dock` built in `workbench.rs`.
- **Routing** is hard-wired:
  - `run_dock_op` sends Problems, Git log and Runs to `show_bottom_panel`, and every
    other op to the sidebar.
  - `follow_terminal` lights the fixed `BOTTOM_BUTTONS`.
  - `focus_bottom_panel`, `bottom_panel_focused`, `focused_dock` (in `dock_leave.rs`)
    and the debug and zen toggles name each tool's focus handle and dock.
- **kiln's `DockState`** holds panels as private `Box<dyn PanelHandle>`. It has no
  way to take a panel out, or to remove a rail action.

## Change

- **kiln-shell** (new `kiln/crates/kiln-shell/src/dock/transfer.rs`, declared from
  `dock.rs`):
  - `DockState::take_panel(id) -> Option<DockedPanel>`, where `DockedPanel` is an
    opaque, owned handle, and `insert_panel(DockedPanel, at)`. Moving a panel this
    way keeps its entity, its focus handle and its `save_state`.
  - `remove_action(id)`, the counterpart of `add_action` / `add_action_after`.
  - Docks built with the same `.transfer_group(name)` accept each other's drags.
    Without a shared group, drags from another dock are ignored, as in card A.
  - A side dock's rail buttons become draggable, with `DraggedDockTab` carrying the
    source dock. A tabbed dock's tabs already drag (card A).
  - Drop targets:
    - a side dock's column and its rail's top group: "into this dock";
    - a side rail's foot group: "into the bottom dock", for a host that maps it so;
    - a tabbed dock's groups: card A's split, join and index zones.
  - Each target draws the drop indicator while hovering.
  - A cross-dock drop calls a new `DockView::panel_dropped(panel, from, target)`
    hook. Its default ignores the drop, kiln's own `Dock` emits `DockEvent`, and the
    host performs the move, since two entities change. This is the pattern of
    `pane::TabDropped`.
- **fe-workbench placement**:
  - A placement map, `PanelId -> Sidebar | Bottom`, defaults to today's places. It
    owns the routing:
    - show ops and `show_bottom_panel` show a tool window in whichever dock holds
      it;
    - `focus_bottom_panel`, `bottom_panel_focused`, `focused_dock`,
      `leave_hidden_panel`, `dock.grow` / `dock.shrink`, the debug toggles and Zen
      Mode read the map, not fixed lists.
  - Moving a tool window (drag, or command) takes it from one dock, inserts it in
    the other, updates the map and shows it there, focused.
  - Movable: every bottom tool window (Problems, Runs, Debug, Git log, Terminal) and
    every sidebar panel except the Explorer, whose reveal and focus code (the tree
    and Structure) assumes the sidebar. Whether the Explorer should move too is the
    user's call; the default is no.
- **What the rail model must change** (`crates/fe-workbench/src/dock.rs`,
  `workbench_chrome.rs`):
  - The foot stops being a fixed array. It becomes the bottom-placed tool windows in
    design order, then the moved-in sidebar panels, each a `RailAction` with its
    panel's icon, title and show command.
  - The top group gains the moved-in bottom tools as always-on-rail panels. A
    bottom tool in the sidebar shows under its heading, one at a time, like any
    sidebar panel.
  - The lit state of foot buttons follows the map. That replaces `BOTTOM_BUTTONS`.
  - `join_debug` adds Debug where it is placed.
  - A sidebar panel that joins the rail on first show keeps that rule in the
    sidebar, and is always on the foot once it is moved to the bottom.
- **Commands** (typed, no default keys, titles in `commands.ftl`). Both act on the
  tool window holding focus, else on the sidebar's active panel or the bottom
  panel's focused group. Both do nothing where they can't apply.
  - `dock.move_tool_window_to_sidebar` "Move Tool Window to Sidebar".
  - `dock.move_tool_window_to_bottom_panel` "Move Tool Window to Bottom Panel".
- **Window layout**: `LayoutKeys` saves `"placements"`, `{id: "sidebar"|"bottom"}`,
  for moved panels only. It is restored before the docks' own state, so the groups
  and the sidebar's active panel find their panels. A layout without it restores
  today's places. The terminal moved to the sidebar still comes back closed, with
  no session.
- **Accessibility**: a moved rail button keeps its name and tooltip. The commands
  have catalogue titles.

## Not in this card

- The Explorer moving.
- Groups or tabs in the sidebar.
- Plugin panels (T1708) being placeable.
- Floating or undocked windows.

## Pins

With no panel moved, nothing changes. `tests::bottom_panel_b3`,
`tests::bottom_panel_groups`, `tests::activity_rail`, `tests::rail_b3`,
`tests::sidebar_b3`, `tests::runs_panel`, `tests::debug_panel`, `dock_leave_tests`,
`dock_focus_tests` and kiln-shell's dock tests pass unmodified (the rail-foot pins
among them). No golden should move. New tests go in the new
`crates/fe-workbench/src/tests/tool_window_placement.rs`, declared from
`tests/mod.rs`, and the new `kiln/crates/kiln-shell/tests/dock_transfer.rs`.

Risk: T1708 (ready, behind T1705) names `crates/fe-workbench/src/dock/mod.rs`. If it
turns `dock.rs` into `dock/mod.rs`, it collides with this card's `dock.rs` edits,
although rgt's prefix check won't flag it. Land one first and rebase the other.

## Implementation notes

- kiln-shell: `DockState::insert_panel(panel, at)` takes `at` as an index in the
  rail's order; the panel goes in the first group, before the next panel there,
  so a dock of one group keeps its default layout. A tabbed host then calls
  `move_panel` (`DockDrop::move_to` converts a drop's zone). `DockDrop` gains
  `Into` (a side dock's column or rail top group) and `Foot` (its own rail
  button on its rail's foot); `DraggedDockTab` carries the dock's transfer
  group. A side dock's rail is wrapped in a layout-neutral div that holds its
  drop layer and its indicator (drawn `deferred`/`anchored` in window
  coordinates); docks without a transfer group build exactly as before.
- `take_panel` in a tabbed dock left with no tab acts as `close_tab` does: the
  nearest drawn group is focused, or the dock hides.
- fe-workbench: the placement map lives on the sidebar `Dock`
  (`Dock::place_of` / `set_place`), which rebuilds the rail foot from it
  (`sync_foot`) and lights it from the bottom panel's shown tools
  (`light_foot`). The workbench moves panels (`move_tool_window`) and
  settles lazily made panels (History, Call Hierarchy, Quickfix, AI Changes)
  into their saved dock as they're registered (`settle_placements`, run from
  the sidebar's observer and before each show).
- A sidebar panel moved to the bottom panel restores active in its saved
  group (`restorable` now admits it); the terminal moved to the sidebar
  comes back with the sidebar hidden and no session.
- Carried findings: T357 F1 (`Dock::title` now `DockState::title`) and T417
  F1 (group navigation notifies the bottom dock;
  `window_keys_across_groups_notify_the_bottom_panel`).
