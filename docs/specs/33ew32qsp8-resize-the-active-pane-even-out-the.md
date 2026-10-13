---
task_uid: "01m4sj26nk5p5spf33ew32qsp8"
task: "T604"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-shell tests/pane_group_resize.rs: with two panes side by side in 801 px, resize(left, Horizontal, 32 px) makes the left 32 px wider and the right 32 px narrower (within 1 px); resize on the last pane takes from its previous sibling; resize along an axis the pane isn't in, or before any frame, returns false and changes nothing"
    required: true
    manual: false
  - id: "C2"
    text: "pane_group_resize.rs: resize stops at MIN_PANE_WIDTH and MIN_PANE_HEIGHT; in left | (top / bottom), resize(top, Horizontal) changes the outer axis's ratios; equalize evens every axis's ratios"
    required: true
    manual: false
  - id: "C3"
    text: "pane_group_resize.rs: next(from, true) and next(from, false) step through panes() order and wrap; with one pane they give None"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench pane_resize_tests.rs: pane.grow_width, shrink_width, grow_height, shrink_height (32 px), equalize, focus_next and focus_previous run through the workbench's command dispatch resize, even out or focus as the kiln-shell calls do, and all seven are in the palette with no default key"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test and cargo clippy --all-targets -- -D warnings pass for kiln-shell and fe-workbench"
    required: true
    manual: false
---

# Resize the active pane, even out the panes and focus the next or previous pane by command: pane.grow_width, shrink_width, grow_height, shrink_height, equalize, focus_next and focus_previous

## Problem

The user (2026-10-13): "We should also allow moving and resizing panes." They work in vim mode, so the keyboard
matters as much as the mouse.

Split panes resize only by dragging a border (T119, `kiln/crates/kiln-shell/src/pane_group.rs`
`drag_divider`; a double-click evens an axis), and focus moves only by direction. The pane commands
(`crates/fe-workbench/src/pane_group.rs` `registry`: `pane.split_right`, `pane.split_down`,
`pane.focus_left|right|up|down`, `pane.move_tab_left|right|up|down`, `pane.close`) have no resize, no
even-out and no next/previous pane, which vim's `<C-w>> < + - = w W` need (bound in a follow-up card).

## Change

### kiln-shell

`kiln/crates/kiln-shell/src/pane_group.rs`:

- `PaneGroup::resize(&mut self, pane: &Entity<V>, axis: Axis, delta: Pixels) -> bool`: grows (`delta > 0`)
  or shrinks the member holding `pane` along the nearest enclosing axis of that kind, taking the space from,
  or giving it to, its next sibling (its previous one when it's last). It changes only those two members'
  ratios and respects `MIN_PANE_WIDTH` / `MIN_PANE_HEIGHT` as dragging does; false, changing nothing, when
  there's no such axis or no room. Pixels convert to ratios with the axis's size from the last frame the
  group drew (record it in `render`); before any frame it returns false.
- `PaneGroup::equalize(&mut self)`: every axis's members get equal ratios, as a double-click does to one
  axis.
- `PaneGroup::next(&self, from, forward: bool) -> Option<Entity<V>>`: the pane after (or before) `from` in
  `panes()` order, wrapping; `None` with one pane.

### fastedit

`crates/fe-workbench/src/pane_group.rs` adds `PaneOp` variants and commands, run by `workbench.rs` beside the
existing pane ops:

| command | does |
|---|---|
| `pane.grow_width` / `pane.shrink_width` | `resize(active, Horizontal, ±32 px)` |
| `pane.grow_height` / `pane.shrink_height` | `resize(active, Vertical, ±32 px)` |
| `pane.equalize` | `equalize()` |
| `pane.focus_next` / `pane.focus_previous` | activates and focuses `next(active, ±)` |

All in the palette (the registry is already listed); no default keys here.

New tests: `kiln/crates/kiln-shell/tests/pane_group_resize.rs`, and
`crates/fe-workbench/src/pane_resize_tests.rs` declared from `pane_group.rs` with `#[path]`.

Fits Mac main (no stack 6 needed).
