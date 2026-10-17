---
task_uid: "01m543dsschp8pspec8jcx3ps7"
task: "T739"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "kiln-shell test dock_bottom_min_centre a_shortened_window_shrinks_the_bottom_dock_before_the_centre passes: after a 500px bottom dock's window shortens to 560px the centre is at least MIN_PANE_HEIGHT high, the dock column is drawn under 500px and its saved size stays 500px"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln-shell passes with no existing test edited"
    required: true
    manual: false
---

# Keep the editor centre at least MIN_PANE_HEIGHT high above a bottom dock when the window shortens, shrinking the dock first (T658 review F1)

## Problem

T658 review F1 (codex, review 8n5v6p, medium): `kiln/crates/kiln-shell/src/shell.rs` `Render for Shell`
(around line 357) gives the editor centre's own div `.min_h_0()` inside the centre-and-bottom-dock column,
while `MIN_PANE_HEIGHT` (80px, `pane_group.rs`) only constrains the whole main row. The centre is `flex_1`
(basis 0), so when the window gets shorter than the bottom dock's size the centre shrinks first, below its
least, and the dock keeps its size. This regresses the resize rule dock.rs documents ("the dock's column
shrinks to its least before the centre"); tests/dock_resize.rs misses it because its `Host` still uses the
old layout.

Reproduced at Mac main 5ce93c05: a `Shell` with a `Pad` centre and a bottom `Dock` built at 500px
(`DockState::new(DockPosition::Bottom).size(px(500.))`), drawn in the 1920x1080 test window, then
`simulate_resize` to 1920x560: the centre (`debug_bounds("pad")`) is 60px high and the dock column 500px.
With the change below it is 80px and 480px, and the saved size stays 500px; tests/dock_full_height.rs,
dock_resize.rs and shell.rs still pass.

## Change

- `shell.rs` `Render for Shell`: the centre's div (the one holding `center` and `dock::track_centre()`)
  takes `.min_h(MIN_PANE_HEIGHT)` instead of `.min_h_0()`, so the bottom dock shrinks first. Nothing else.
- New integration test file `kiln/crates/kiln-shell/tests/dock_bottom_min_centre.rs` (`mod support;`, a small
  `DockItem` panel of its own; tests/dock_full_height.rs shows the setup), test
  `a_shortened_window_shrinks_the_bottom_dock_before_the_centre` at the file's top level: the case above,
  asserting the centre is at least `MIN_PANE_HEIGHT` high, the dock column is drawn under 500px and the
  dock's `current_size()` is still 500px. It fails at the base.
