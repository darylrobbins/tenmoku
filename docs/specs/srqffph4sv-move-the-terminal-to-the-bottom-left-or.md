---
task_uid: "01m4sj260yx4p3m4srqffph4sv"
task: "T603"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-shell tests/dock_position.rs: set_position(Left) on a bottom dock draws a vertical rail on the left edge with its column to the rail's right, keeping its panels, active panel, visibility and rail actions; set_position(Bottom) puts the buttons back in the title row"
    required: true
    manual: false
  - id: "C2"
    text: "dock_position.rs: a dock's side width and bottom height are separate (resizing at one position leaves the other), and save/restore keeps both"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench dock_position_tests.rs: terminal.move_to_right with a session running puts the terminal dock on the right with the same session (no new start) and focus kept, and the sidebar moves left; terminal.move_to_bottom then leaves the sidebar on the left"
    required: true
    manual: false
  - id: "C4"
    text: "dock_position_tests.rs: with the sidebar left and the terminal right, dock.move_sidebar_right sends the terminal left; the positions are saved in layout_entry's docks object and a new workbench restoring it places both docks there"
    required: true
    manual: false
  - id: "C5"
    text: "The five commands are in the palette with no default key; cargo test and clippy -D warnings pass for kiln-shell and fe-workbench"
    required: true
    manual: false
---

# Move the terminal to the bottom, left or right and the sidebar to the left or right, one dock per edge, kept in the window layout

## Problem

The user (2026-10-13): "We should also allow moving and resizing panes."

Tabs already move between editor panes and onto pane edges (T119), but the docks are fixed where the
workbench puts them (`crates/fe-workbench/src/workbench.rs`): the sidebar (project, search, outline, changes,
references, problems) on the right, the terminal at the bottom. A user who wants the terminal beside the code
on a wide screen, or the sidebar on the left as in VS Code, can't. kiln-shell's `Shell::set_dock` already
places a dock view at any `DockPosition` (one per position), but a `DockState`'s position is fixed at `new`,
and its size is one number used as width at a side and height at the bottom.

## Change

### kiln-shell

`kiln/crates/kiln-shell/src/dock.rs`:

- `DockState::set_position(DockPosition)`: the dock draws as a dock at that edge from the next frame (a
  vertical `ActivityRail` at a side, its buttons in the title row at the bottom), keeping its panels, active
  panel, visibility and rail actions.
- A dock keeps two sizes: its width at a side and its height at the bottom, each starting at the size it was
  built with, each changed only by resizing it there (dragging, `set_size`). `DockState::save` and `restore`
  keep both (`"size"` stays the current position's, `"side_size"` / `"bottom_size"` added).

### fastedit

In `crates/fe-workbench/src/dock_layout.rs`'s registry (the dock-resize card's file, already initialised and
listed in the palette), run from the workbench:

- `terminal.move_to_bottom`, `terminal.move_to_left`, `terminal.move_to_right` move the terminal dock
  (`Shell::set_dock(old, None)`, `set_dock(new, Some(..))`, `DockState::set_position`). Its visibility, its
  sessions and focus are kept.
- `dock.move_sidebar_left`, `dock.move_sidebar_right` move the sidebar dock the same way.
- **One dock per edge.** Moving a dock onto the edge the other one holds sends that one to the edge it came
  from when that was a side, else to the opposite side: the terminal moved right with the sidebar on the
  right sends the sidebar left; the sidebar moved right with the terminal on the right sends the terminal
  left.
- The positions are kept in the window layout: `layout_entry` adds `"docks": {"sidebar": "right",
  "terminal": "bottom"}` and `restore_layout` applies it before the dock states.
- All five commands are in the palette; no default keys.

New tests: `kiln/crates/kiln-shell/tests/dock_position.rs`, and `crates/fe-workbench/src/dock_position_tests.rs`
declared from `workbench.rs` with `#[path]`.

Fits Mac main (no stack 6 needed); after the dock-resize card (same files) and the terminal rail button card
(`dock.rs`, `workbench.rs`).

## Implementation notes

- The dock-resize card's `DockSizeOp` became `DockLayoutOp` (grow, shrink, and the two moves), and
  `dock_layout::displaced` is the one-dock-per-edge rule as a pure function.
- A restored `"docks"` object is applied only when it names two edges with the sidebar at a side;
  anything else leaves the docks as built.
- The terminal's dock saves `"size"`, `"side_size"` and `"bottom_size"` and nothing else, so it still
  comes back hidden with no session (the dock-resize card's one-key assertion now expects the three).
  The sidebar saves its full state, both sizes included.
- A shown bottom dock draws its rail's buttons at the start of its title row (36 px tall then, the
  buttons' height); hidden, the buttons alone form a row on the window's bottom edge, as before.
  `DrawnDock::title_bounds` is new, for the tests.
- Carried T602 F8: the six sidebar panels now fill the dock's column (`flex_1().min_w_0()`) rather
  than a fixed `PANEL_WIDTH`; `dock_position_tests.rs` measures each at 440 px, 160 px, 300 px, moved
  left and restored.
