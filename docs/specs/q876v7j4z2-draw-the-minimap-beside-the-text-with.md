---
task_uid: "01m5f7gszz41x6daq876v7j4z2"
task: "T1033"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "EditorView::set_minimap is off by default: every existing fe-gpui golden is byte-identical and DiffView and commit-message editors show no minimap"
    required: true
    manual: false
  - id: "C2"
    text: "With it on, the text area and soft-wrap width narrow by the minimap's width; token runs paint in their syntax colours on minimap_background over display rows; the slider covers the visible rows, shown on hover with mouseover and always with always (element/minimap_tests.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "Clicking the minimap centres the clicked row without moving the caret, selection or vim mode; dragging the slider scrolls and continues outside the minimap; the wheel over it scrolls; a click at the text's right edge still lands in the text; dragging works in screen rows: MinimapDrag::screen_top returns the start row when dy is 0 and clamps only in screen rows to [0, to_screen_top(last_row)], so a horizontal move inside a trailing block keeps scroll (301 display rows + a 15-row trailing block: screen top 306 stays 306) and a drag from inside an interior block (screen top 105) reaches both the top and the bottom (315) (view/minimap_tests.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "Workbench file editors show the minimap by default; reloading settings with [editor.minimap] enabled = false hides it in open editors and max_column = 40 narrows it, live (tests/minimap_setting.rs)"
    required: true
    manual: false
  - id: "C5"
    text: "New minimap screenshot golden committed and listed in TESTING.md; workbench.png regenerated with only the minimap and narrower text changed"
    required: true
    manual: false
---

# Draw the minimap beside the text with its slider, scroll by click, drag and wheel over it, and show it in the workbench's file editors from editor.minimap, applied live

# Minimap 2/5: draw the minimap beside the text with its slider; click, drag and wheel scroll; on in the workbench's editors

Builds on card 1's `fe_display::minimap` geometry and `row_runs`, settings and theme colours. Mac-only:
GPUI paint and screenshot goldens.

## Where it plugs in

- `EditorView::set_minimap(Option<MinimapOptions>, cx)` (like `set_gutter`, not through
  `EditorTheme::set_metrics`, which every editor shares): off (`None`) by default, so diff views, the commit message box, chat code blocks, tests and fe-gpui's existing goldens are
  unchanged. `fe_gpui` converts `fe_config::EditorSettings` to options (`None` when `editor.minimap.enabled = false`).
- crates/fe-workbench/src/pane.rs: `new_editor` sets it from `self.settings`, and `reload_settings` sets it
  on every open editor, so changing any `editor.minimap.*` setting applies live without reopening.
- `EditorElement::prepaint` (crates/fe-gpui/src/element.rs): when on, take the minimap's width off the
  right (or, card 5, left) of `bounds` before the wrap width and `FrameInput.width` are computed, so soft
  wrap, horizontal scroll and hit testing see the narrower text area. Lay out the minimap in a new
  `element/minimap.rs` (kept out of element.rs, which is near 2,900 lines), stored on `FrameLayout`.
- If sticky scroll has landed, its band spans the narrowed text area, not the minimap.
- Rows: display rows (folds hidden, wrapped rows split), VS Code's view lines. The editor's scroll is in
  screen rows; convert through the frame's block map (`to_display_top`), so code lenses and proposal
  hunks above rows don't skew the slider.
- Colours: for the display rows the minimap shows, the same `styled_highlights` spans the text gets
  (syntax, bracket colours, foreground decorations), turned into `row_runs` and painted as quads, one per
  run, the row's height (checked side by side with VS Code's block rendering), each run in
  its colour at a fixed alpha over `minimap_background` (a const, tuned side by side with VS Code).
  Rows outside the syntax window of a large file take `foreground`. This card paints directly each frame;
  card 3 caches it. All rows render as blocks for now, whatever `render_characters` says.
- Slider: `minimap_slider` over the visible rows; with `show_slider = mouseover` only while the pointer
  is over the minimap (or dragging), always with `always`; `minimap_slider_hover` under the pointer,
  `minimap_slider_active` while dragging.

## Mouse (crates/fe-gpui/src/view/minimap.rs, new; called from `press_proceeds` before text hit testing,
as `click_change_bar` is)

- Press on the slider starts a drag (`Drag::Minimap { start_first_row, start_y }`); moving scrolls the
  editor via `scroll_for_drag`, release ends it, `on_mouse_up_out` too.
- Press off the slider scrolls so the clicked row is centred (`scroll_for_click`), then drags from there
  as VS Code does.
- Neither moves the caret or the selection, changes vim's mode, or starts a text selection; vim owning
  the mouse doesn't change this.
- The wheel over the minimap scrolls the editor as over the text.
- The pointer is an arrow over the minimap, not the I-beam; hovering it hides no popover beyond what
  leaving the text already does.

## Screenshots

- crates/fe-gpui/examples/screenshots/scenes.rs: give `Scene` a `minimap: Option<MinimapOptions>` (None
  for existing scenes, whose goldens must not change) and add a `minimap` scene: a ~300-row Rust file
  scrolled to the middle, slider `always`; commit its golden and list it in TESTING.md's screenshot list.
- crates/fe-workbench/examples/screenshots/golden/workbench.png: regenerate (the workbench's editor now
  shows the minimap by default) and check the diff is only the minimap and the narrower text.

## Tests (new files)

- crates/fe-gpui/src/element/minimap_tests.rs: the text area narrows by the minimap's width (soft wrap
  wraps earlier); a narrow editor hides it; runs land at the right x/y; slider rect for top, middle, end.
- crates/fe-gpui/src/view/minimap_tests.rs: click centres the row and leaves the selection; slider drag
  to the bottom reaches the last page and back; drag continues outside the minimap; wheel scrolls; a
  click at the text's right edge still lands in the text; vim normal mode click on the minimap leaves
  the mode and caret.
- crates/fe-workbench/src/tests/minimap_setting.rs (register in tests/mod.rs): file editors show it by
  default; `enabled = false` under `[editor.minimap]` in a reloaded settings file hides it in open
  editors and `true` brings it back; `max_column = 40` narrows it live; the commit message box has none.

## Not in this card

Caching/tiles and the bench (3), selection/find/git/diagnostic marks (4), glyph rendering and
`side = "left"` (5; left draws as right until then), a vertical scrollbar beside it (none exists
in fastedit's editor), DiffView minimaps.

## Implementation notes

- `EditorView::set_minimap(Option<MinimapOptions>, cx)`, `minimap()` and `minimap_bounds()` (the
  last frame's, `None` when off or too narrow); `fe_gpui::minimap_options(&EditorSettings)` converts
  the settings, and `fe_gpui::MinimapOptions` re-exports fe-display's. The workbench's
  `Pane::sync_tabs`, which every tab passes through however its editor came (opened, split by
  `PaneItem::split`, restored by `PaneItem::restore`), and `Pane::reload_settings` set it on every
  `EditorView` tab; diff tabs and the commit box never get it, and a standalone `EditorView` keeps
  it off. (Review F1.)
- Prepaint takes `minimap::width` (fe-display's `MinimapOptions::width`, side forced right) off the
  editor's width before the wrap width and `FrameInput.width`, so the frame, its text hitbox and
  soft wrap all see the narrower area. Once the frame's scroll is final, `element/minimap.rs` lays
  out `MinimapLayout` with both edges of the view taken through `block_map.to_display_top`, so lens
  rows, host blocks and proposal hunks on screen don't count as visible text rows (review F2), and
  the runs of the rows it draws.
- Runs: rows are grouped into chunks with at most `CHUNK_GAP` (16 KB) of text between them (what a
  fold hides, or a long row's rest), and each chunk asks once for `styled_highlights` with the
  `Foreground` decorations over it, so the colours are the text's and the syntax's 8-range cache
  isn't thrashed by folds. `load_injected_languages` now runs after the minimap's queries, so a
  cold injected language or a cut-short query found only there loads or retries without other
  input (review F3). A
  long row is read only as far as `4 * max_column` bytes (a char is at least a column). Each run is
  painted at `RUN_ALPHA` = 0.6 of its colour over `minimap_background`. That alpha was judged from
  the `minimap` golden alone: the worker's shell can't capture a VS Code window to compare side by
  side, so a person comparing the two may retune the const (card 3 or 5).
- Hover: a hitbox over the minimap gives the arrow cursor; window-wide pointer listeners and a
  deferred check after each frame keep `minimap_hover` (over, on the slider) as the fold column's
  hover is kept, so the slider shows on hover with `mouseover` and follows a slider moving under a
  still pointer. Each of them clears the window's `PointerOutside` mark before reconciling, since
  GPUI runs the minimap's listeners before the fold column's (review F4).
- Mouse: `press_minimap` runs first in `press_proceeds`, before a composition is dropped or vim
  sees the press. Off the slider it scrolls so the clicked row's centre is the editor's centre in
  screen rows (`to_screen_top(row + 0.5)` less half the screen rows, clamped), blocks counted, then
  drags from there (review F2). A drag (`MinimapDrag`) keeps what it started with: the minimap's
  geometry then, and the screen row and display row at the top. A move scrolls by the display rows
  that geometry gives the pointer's travel, taken to screen rows through the block map, from the
  screen row it started at, so a top partway into a block stays put while the pointer only moves
  sideways, and a pointer height always gives the same scroll however blocks entering the view
  change the slider (review F5, F6).
- Rev 3's C3 (review round 3): `MinimapDrag::screen_top` returns the start screen row when `dy` is 0,
  and otherwise moves it by the screen rows of the display rows the travel takes, clamped only in
  screen rows to `[0, to_screen_top(last_row)]`. The display rows aren't clamped first: the travel
  is read as `scroll_for_drag(0, |dy|)`, and travel that reaches the geometry's limit (the whole
  file) goes straight to the top or the bottom. So a sideways move inside a 15-row block after the
  last of 301 rows keeps screen top 306, and from screen top 105 inside the block above row 100 a
  drag reaches 0 and 315. One reading beyond the criterion's words: when the drag starts past the
  last row's screen row (306 > 300), the upper bound is the start, not 300, so a small drag down
  leaves 306 rather than scrolling up against the pointer; up, it clamps as stated. A drag that
  starts inside a block (review round 4, F6) first scrolls through that block's rows, a screen row
  per row of travel, to its edge in the direction of travel; the rest of the travel then goes to
  the screen top of the display row it reaches, so the block isn't counted again. From 105 inside
  the block above row 100 (screen rows 100-114), a row up is 104 and six rows up is 99; a row down
  is 106 and eleven is 116. From 306 inside the trailing block, a row up is 305 and six is 300.
  Travel up past row 0 is the top (review round 5, F7): `to_screen_top` floors a negative row to
  row 0 and would count a block above row 0 again, so from 5 inside a 15-row block there, five
  rows up and anything further is 0, never back down at 14.
  Other blocks the drag crosses are passed at once, as the slider, which has no rows for them,
  stays with the pointer. The drag
  (`Drag::Minimap`) is followed by a window-wide mouse-move listener, so it carries on over the
  text and outside the window; the div's own move listener ignores it. Scrolling goes through the
  last frame's block map (`to_screen_top`), sets `autoscroll = false` and never calls `changed`,
  so the caret stays out of view. The wheel needs nothing new: the editor's div gets it over the
  minimap too. A press tests the minimap's hitbox, so a popover or menu over it keeps the press.
- Goldens: every existing fe-gpui scene compares unchanged; the new `minimap` scene is 9 modules of
  the `RUST` sample (~300 rows), scrolled with `set_scroll_top(140)`, slider `always`. The
  workbench golden's diff was only the minimap and row 9's text cut at the narrower edge.
