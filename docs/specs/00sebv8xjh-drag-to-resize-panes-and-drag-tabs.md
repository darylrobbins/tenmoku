---
task_uid: "01m490c8yt61k2te00sebv8xjh"
task: "T119"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::pane_drag::dragging_a_divider_resizes: with two panes side by side, dragging the divider 100 px right (simulate_mouse_down, _move, _up) widens the left pane by 100 px and narrows the right one by as much (each within 1 px); the axis's ratios sum to 1; simulate_resize to a wider window keeps the ratios (widths within 1 px)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::pane_drag::minimums_and_double_click: dragging the divider far left stops the left pane at MIN_PANE_WIDTH (120 px); splitting the right pane down and dragging that divider stops at MIN_PANE_HEIGHT (80 px); a member that is a vertical axis holding a horizontal split stops at 240 px plus the border; double-clicking a divider makes that axis's widths equal within 1 px"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::pane_drag::splits_and_closes_share_space: after dragging to 70/30, splitting the left pane right gives 35/35/30; closing the middle pane gives its share back to the left one, which is 70/30 again"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::pane_drag::dropping_on_a_tab_bar: dragging b.rs's tab from the left pane over the left half of the right pane's second tab shows the insertion marker there, and dropping inserts it at index 1, active and focused, with the same EditorView entity; dragging the left pane's last tab out closes that pane; dropping a path the target pane already shows activates that tab and closes the dragged one; dropping a tab after the last tab of its own bar moves it to the end"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::pane_drag::dropping_on_pane_edges: hovering a dragged tab over a pane's right quarter reports drop zone Right with the overlay covering that pane's right half, and dropping splits that pane right with the tab in the new pane; the top quarter gives Up and a new pane above; the centre moves the tab into the pane; dropping a pane's only tab on its own centre, an edge or its tab bar leaves panes and tabs unchanged"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::pane_drag::drags_and_keys_share_move_tab: PaneGroup::move_tab's test counter rises by one for ctrl-cmd-right and by one for a tab-bar drop; a dirty tab dragged to another pane keeps its dirty dot and cmd-z there undoes its last edit; every tests::splits test passes"
    required: true
    manual: false
  - id: "C7"
    text: "No new commands or bindings; rg 'KeyBinding::new|actions!' finds no match in the files this card adds; crates/fe-ui changes only Tab and TabBar drag hooks and one UiTheme field, drop_target"
    required: true
    manual: false
---

# Drag to resize panes, and drag tabs between panes or onto a pane's edge to split it

# Drag to resize panes, and drag tabs between panes or onto a pane's edge to split it

Milestone M3 "Browse", a follow-up to the splits card (T79). It builds on:

- T79's `PaneGroup` (`Pane` or `Axis { axis, members }`, with 1 px borders), its split, focus,
  move-tab and close rules, and its `pane.*` commands
- T63's `Pane` and fe-ui's `TabBar` and `Tab`
- GPUI's drag and drop (`on_drag`, `drag_over`, `on_drop`, `on_drag_move`). A drag starts once the
  mouse moves more than 2 px with the button down, so `simulate_mouse_down`, then `_move`, then `_up`
  drives it in tests.

T79's "Out of scope" lists drag-resizing panes and dragging tabs between panes. Its panes are
equal-sized.

## Problem

Split panes can't be resized, and a tab can move only by key (`ctrl-cmd-arrow`). Users expect to drag a
divider, drag a tab onto another pane's tab bar, or drop it on a pane's edge to split there.

## Change

### Sizes live in the pane tree

`crates/fe-workbench/src/pane_group.rs`:

- **Ratios.** Each axis keeps one ratio per member, summing to 1. A new axis starts equal, so T79's
  equal sizes hold until something is dragged. Members lay out by ratio along the axis, less the 1 px
  borders.
- **Splitting** a member gives the new pane half of that member's share. A new nested axis splits 50/50.
- **Removing** a member gives its share to its preceding sibling, or to the following one when it was
  first.
- **Window resizes** keep the ratios.
- The ratios are kept in the tree only. Saving the layout across launches stays out of scope.

### Resize handles

- Between each pair of adjacent members, a handle 5 px across, centred on the 1 px border, shows
  `ResizeLeftRight` or `ResizeUpDown`.
- Dragging it moves that border and changes only those two members' ratios.
- **Minimum sizes:**
  - A pane is at least `MIN_PANE_WIDTH` (120 px) wide and `MIN_PANE_HEIGHT` (80 px) tall.
  - An axis's minimum along its own direction is the sum of its members' minimums plus the borders.
  - An axis's minimum across its direction is the largest of its members' minimums.
  - A drag clamps at the minimum.
  - When the window is too small for every minimum, members shrink in proportion. Minimums apply only
    while dragging.
- Double-clicking a handle makes that axis's ratios equal again.

### Dragging a tab

- **Starting.** In `crates/fe-ui/src/tab_bar.rs`, `Tab` gets an `on_drag` hook (a payload value and a
  ghost view), plus `on_drop` and `drag_over` hooks. `TabBar` gets the same hooks for the space after
  the last tab.
  - In `crates/fe-workbench/src/pane.rs`, each tab is draggable as a `DraggedTab`: its pane, and a
    stable tab ID (add one in `pane.rs` if tabs have none, because indices can shift while a file
    loads).
  - The ghost is the file name, in tab style, following the pointer.
  - New file `crates/fe-workbench/src/pane_drag.rs` holds `DraggedTab`, the ghost, the drop-zone maths
    and the overlay.
- **Onto a tab bar.**
  - The insertion point is before the tab under the pointer if the pointer is in that tab's left half,
    and after it otherwise. Past the last tab, it's the end.
  - While hovering, a 2 px vertical marker in a new `UiTheme::drop_target` colour shows the point. The
    colour goes in `crates/fe-ui/src/theme.rs`.
  - **In its own pane,** a drop reorders the tab.
  - **In another pane,** a drop moves the tab there, at that index, active and focused, under T79's
    rules:
    - If the target already shows the path, that tab activates and the moved one closes.
    - A source pane emptied by the move closes.
- **Onto a pane's content area** (the editor or the empty-pane placeholder):
  - The drop zone is worked out from the pointer within the content bounds:
    - the outer quarter of the width nearest the left or right edge is Left or Right
    - the outer quarter of the height nearest the top or bottom is Up or Down
    - in a corner, the nearer edge wins, by distance relative to the size
    - anywhere else is Centre
  - While hovering, an overlay filled with `drop_target` at low alpha covers the half of the pane on
    that side, or the whole content area for Centre.
  - Dropping on **Centre** moves the tab into that pane, appended.
  - Dropping on **an edge** splits that pane in that direction and moves the tab into the new pane.
  - Dropping a pane's only tab on that same pane, whether its centre, an edge or its own tab bar,
    changes nothing.
- **The moved tab keeps its `EditorView` entity,** so its dirty state, undo history, selections and
  scroll go with it.

### One move path for drags and keys

`PaneGroup::move_tab(source, tab, destination, window, cx)` is the only way a tab moves between panes,
where `destination` is one of:

- `Pane { pane, index: Option<usize> }`
- `Split { pane, direction }`

T79's `pane.move_tab_*` commands call it:

- with a neighbour in that direction, `Pane { neighbour, index: None }`
- without one, `Split { source, direction }`

Drops call it too. If T79 already has one function for its moves, give it these destinations rather than
adding a second function. A test-only counter records calls.

`crates/fe-workbench/src/workbench.rs` and `crates/fe-workbench/src/lib.rs` change only as wiring needs:
a `mod` line, and passing drops to the `PaneGroup`.

### Commands

There are no new commands or bindings. Moves by key keep T79's commands and bindings.

### Tests

New file `crates/fe-workbench/src/tests/pane_drag.rs`, declared in `tests/mod.rs`. It uses headless GPUI
and drives drags with `simulate_mouse_down`, `simulate_mouse_move` and `simulate_mouse_up` at laid-out
bounds. Test-only accessors expose an axis's ratios, the hovered drop zone and the overlay's bounds.

The merge cap is 1,000 changed lines, not counting tests. If the card runs over, split the edge-drop
overlay into a follow-up.

## Out of scope

- Saving the layout across launches.
- Dragging files from the tree, or from Finder, into panes.
- Dragging tabs between windows, or out into a new window.
- Cancelling a drag with Escape.
- Animating the drop.
- Resizing the dock.
- Keyboard resizing of panes.

## Decided (lead defaults; the user may revisit)

- Ratios live in the pane tree.
- Splitting halves the split member's share, as VS Code does. A removed pane's share goes to its
  neighbour.
- The minimum pane size is 120 × 80 px. Double-clicking a divider evens out that axis.
- The drop zones are the outer quarter on each side, with Centre in the middle, as VS Code's editor
  groups have them.
- Drags and keyboard moves share `PaneGroup::move_tab`.
