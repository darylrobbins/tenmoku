---
task_uid: "01m4sj2517n8176cvxgt9j7xk2"
task: "T602"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-shell tests/dock_resize.rs: dragging a right dock's inner-edge handle 100 px left widens it by 100 px, a left dock's 100 px right likewise, and a bottom dock's 50 px up heightens it by 50 px; the handle shows the resize cursor"
    required: true
    manual: false
  - id: "C2"
    text: "dock_resize.rs: a drag past the limits stops at MIN_DOCK_WIDTH (160 px) or MIN_DOCK_HEIGHT (80 px) and at the size that leaves the centre MIN_PANE_WIDTH or MIN_PANE_HEIGHT; double-clicking the handle restores the size the dock was built with"
    required: true
    manual: false
  - id: "C3"
    text: "dock_resize.rs: DockState::save includes size; restore sets a saved size, raises one below the minimum to it, and leaves the built size when none was saved"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench dock_resize_tests.rs: after dragging the sidebar to 320 px and the terminal dock to 400 px, layout_entry holds both; a new workbench restoring that entry draws both at those sizes, the terminal dock hidden until terminal.toggle"
    required: true
    manual: false
  - id: "C5"
    text: "dock_resize_tests.rs: dock.grow with the terminal focused grows the terminal dock by 40 px and with an editor focused the sidebar; dock.shrink stops at the minimum; both are in the palette with no default key; cargo test and clippy -D warnings pass for kiln-shell and fe-workbench"
    required: true
    manual: false
---

# Resize the sidebar and terminal docks by dragging their inner edge or with dock.grow and dock.shrink, with the sizes kept in the window layout

## Problem

The user (2026-10-13): "We should also allow moving and resizing panes."

Editor panes already resize and move: T119 gave kiln-shell's `PaneGroup` drag handles on split borders
(double-click evens them), tab drags between panes and onto pane edges, and the ratios are saved in the
window layout (`kiln/crates/kiln-shell/src/layout.rs`, `save_panes`). The docks don't resize at all:
`DockState` (`kiln/crates/kiln-shell/src/dock.rs`) has a fixed `size` set at construction (the sidebar's
`PANEL_WIDTH` 240 px, the terminal dock's default 240 px tall), no handle, no key, and `DockState::save`
keeps only `visible`, `active` and `joined`. The workbench saves the sidebar dock's state in the window
layout (`Workbench::layout_entry`, `"dock"`) and nothing of the terminal dock. A terminal 240 px tall is a
dozen rows.

## Change

### kiln-shell: a dock's edge drags

`kiln/crates/kiln-shell/src/dock.rs`:

- A shown dock's column has a handle on its inner edge (the edge facing the centre: a right dock's left, a
  left dock's right, a bottom dock's top), 5 px across, centred on the 1 px border, with
  `ResizeLeftRight` / `ResizeUpDown`, as `PaneGroup`'s handles are. Dragging it sets the dock's size to
  follow the pointer.
- **Limits.** At least `MIN_DOCK_WIDTH` (160 px) for a side dock and `MIN_DOCK_HEIGHT` (80 px) for a bottom
  one; at most what leaves the centre `MIN_PANE_WIDTH` / `MIN_PANE_HEIGHT` (from `pane_group.rs`), measured
  from the window's content size in the frame the drag moves in. A window shrinking below that leaves the
  stored size alone and draws the dock clamped.
- **Double-click** on the handle puts back the size the dock was built with.
- `DockState::size()` and `DockState::set_size(Pixels)` (clamped to the minimum). The host reads the size
  when it saves its layout; no event is needed.
- `DockState::save` adds `"size"` (pixels, a number); `restore` reads it back, clamped to the minimum.

### fastedit: kept in the layout, and keys

- `crates/fe-workbench/src/workbench.rs`: `layout_entry` adds `"terminal_dock": {"size": …}`, and
  `restore_layout` applies only its size (the terminal dock still starts hidden, with no session). The
  sidebar's size comes with its existing `"dock"` entry.
- New `crates/fe-workbench/src/dock_layout.rs` (declared and initialised from `lib.rs`, listed in
  `command_list`, run from `workbench.rs`'s command dispatch as `pane_group::op` is): `dock.grow` and
  `dock.shrink` change the size of the dock holding focus (the terminal's while the terminal has focus),
  else the sidebar's, by 40 px within the limits, showing it if hidden. No default keys: they are for the
  palette and the user's keymap (vim users can bind them beside `<C-w>`).

New tests: `kiln/crates/kiln-shell/tests/dock_resize.rs`, and `crates/fe-workbench/src/dock_resize_tests.rs`
declared from `workbench.rs` with `#[path]`.

Fits Mac main (no stack 6 needed).

## Implementation notes

- **The getter is `DockState::current_size()`, not the spec's `DockState::size()`: a lead decision
  (2026-10-13).** `DockState` already has the builder `size(Pixels) -> Self`, and Rust can't have both
  under one name. Renaming the builder would change `crates/fe-workbench/src/dock.rs`, which T601 holds,
  so the builder keeps its name and callers as at base, and the getter takes another. The builder also sets
  the size a double-click puts back. `MIN_DOCK_WIDTH` / `MIN_DOCK_HEIGHT` are reached as
  `kiln_shell::dock::…`; `kiln-shell/src/lib.rs`, which T601 also holds, is unchanged.
- **Double-click then drag (review round 3, F7).** The second press puts back the built size and starts
  its drag from that size, not from the size the last frame drew.
- **Drawn size: by layout (review rounds 1 and 2).** A shown dock's column has its size as its width (or
  height), shrinks (`flex_shrink`) down to `MIN_DOCK_WIDTH` / `MIN_DOCK_HEIGHT`, and its outermost element
  shrinks down to that plus the rail. `Shell` gives the centre its least: the centre's wrapper has
  `min_w(MIN_PANE_WIDTH)` and the main row `min_h(MIN_PANE_HEIGHT)` (it was `min_h_0`); the title bar, the
  top row and the status bar don't shrink. So in a window too small for the docks' sizes, flex layout
  shrinks the docks within their minimums, together, in the same frame, the stored sizes untouched: from
  the first frame, with both side docks oversized, and as rows come and go. Nothing is shared between
  docks. A host other than `Shell` gives its centre the same minimums to get this.
- **Stored size: measured.** `Shell` draws `dock::track_centre()` in its centre (a `relative` wrapper round
  the centre view), which records the centre's laid-out bounds per window. `DockState::max_size` (drags and
  `dock.grow`) is the centre's extent less its least plus the dock's column as last laid out, counting the
  window's change of size since; before the centre is measured, the window's content size less the rail
  and the centre's least.
- `DrawnDock::handle_bounds` and `handle_cursor` report the handle for tests.
- A drag notifies only the dock, so the workbench observes both docks and saves its layout from there too.
