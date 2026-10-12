---
task_uid: "01m4r8jv89phzstbm2eymneys3"
task: "T33"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display test layout::change_lane_tests: for files of 3-, 4- and 5-digit row counts at em 10, text_left == gutter_width == em x (digits + 2 + CHANGE_GAP_EMS + CHANGE_LANE_EMS + FOLD_COLUMN_EMS) with CHANGE_GAP_EMS = CHANGE_LANE_EMS = 0.5 and FOLD_COLUMN_EMS 3.75 / FOLD_CHEVRON_EMS 2.25 unchanged; wrap_width == floor((width - gutter_width - 2 em) / em) x em; frame.change_lane == {x: fold_column.x - 5, y: 0, width: 5, height: frame height}; every line number's right edge == change_lane.x - 5 (0.5 em); for every fold toggle, chevron_box(toggle).x - (change_lane.x + 3) \u003e= 5 (0.5 em)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test element::change_bar_tests: a Gutter(Bar) decoration over 3 unwrapped buffer rows places exactly one rect at x = layout origin + change_lane.x, width 3 px at scale 1, top = first row's top + 1 px, height = 3 x line_height - 2 px; with soft wrap spreading those rows over more than 3 display rows it is still one rect covering all of them, inset 1 px; two adjacent Bar decorations (rows 1-2 and 3-4) give two rects 2 px apart; every placed Gutter(_) mark has corner_radius = half its smaller side and every other kind 0"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test element::change_bar_tests: a Gutter(Deletion) at a row start places one rect with x = change_lane.x, width = change_lane.width, height 3 px at scale 1, centred on that row's top edge, its left and right edges within [change_lane.x, change_lane.x + change_lane.width]; a deletion past the end of a text whose last row isn't empty is centred on the last row's bottom edge"
    required: true
    manual: false
  - id: "C4"
    text: "Existing tests pass, edited only where geometry moved: fold_column_tests, layout tests.rs, long_row_tests and slice_cap_tests change only gutter-derived numbers and the line-number edge assertion; decorations_tests changes only the bar's x, width and count and the deletion mark's bounds; crates/fe-gpui/src/view/diagnostics_tests.rs and crates/fe-gpui/src/view/fold_hover_tests.rs are unchanged and pass"
    required: true
    manual: false
  - id: "C5"
    text: "On helium (Linux) cargo run -p fe-gpui --example screenshots and cargo run -p fe-workbench --example screenshots build and exit 0 (they skip without Metal); the golden PNGs are left unchanged, to be regenerated on the Mac with UPDATE_SCREENSHOTS=1 when the batch lands"
    required: true
    manual: false
  - id: "C6"
    text: "git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md; crates/fe-editor is unchanged"
    required: true
    manual: false
---

# Give the git change bar its own gutter lane, 0.5 em clear of the line numbers and the fold chevrons, drawn as one rounded 3 px bar per run (Mac T484)

Filed on helium from Mac T484 (~/coordination/helium-cards/T484.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-p.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note (Mac note p): criteria saying nothing outside the scope changes exempt this lane's own docs/specs file. Title the host commit "... (Mac T<n>)".

# Give the git change bar its own gutter lane, clear of the line numbers and the fold chevrons

## Problem

The user, 2026-10-12: "Don't like the formatting of the git change indicator. It's too close to the
line number. It looks cramped. Think of how we make it look better visually."

At main b6a3275f:

- `crates/fe-display/src/layout.rs`: `gutter_width` is `em * (digits + 2. + FOLD_COLUMN_EMS)`. Line
  numbers are right-aligned to end at `text_left - fold_column_width`, which is `frame.fold_column.x`.
  The fold column is 0.75 em pad | `FOLD_CHEVRON_EMS` (2.25 em) chevron box | 0.75 em pad. There is no
  space reserved for change marks.
- `crates/fe-gpui/src/element/decorations.rs` (`Decorations::layout`): `GutterMark::Bar` is placed at
  `gutter_x = frame.fold_column.x`, so flush against the digits' right edge, `BAR_WIDTH` 2 px, one rect
  per display row (`for row in rows { whole_row(row.row, gutter_x, bar) }`). Stacked per-row rects read
  as one strip only by accident, and two adjacent hunks (an added run right under a modified run) merge
  into one strip. `GutterMark::Deletion` is a `DELETION` 6 × 2 px dash at the same x, centred on a row
  edge, so it pokes into the fold column's pad.
- The decorations come from `crates/fe-editor/src/row_change_decorations.rs`: one `Gutter(Bar)` (and one
  `RowBackground`) decoration per added or modified change, spanning all its rows, and one
  `Gutter(Deletion)` per deleted change. That file does not change here.
- The diagnostics' gutter dot (`diagnostic_marks` in `crates/fe-gpui/src/element.rs`) sits at the
  gutter's left edge, left of the digits. It must not move.

## Change

### Layout (fe-display)

In `crates/fe-display/src/layout.rs`:

- Add `pub const CHANGE_GAP_EMS: f32 = 0.5;` (clear space between the digits' right edge and the change
  lane) and `pub const CHANGE_LANE_EMS: f32 = 0.5;` (the lane the change bar and deletion mark go in),
  each with a doc comment. The gutter grows by their sum:
  `gutter_width = em * (digits + 2. + CHANGE_GAP_EMS + CHANGE_LANE_EMS + FOLD_COLUMN_EMS)`. `text_left`
  and `wrap_width` keep following `gutter_width` as now. `FOLD_COLUMN_EMS` and `FOLD_CHEVRON_EMS` keep
  their values: the fold column does not shrink and stays flush against the text.
- Add `pub change_lane: Rect` to `Frame`, the frame's height, with
  `x = fold_column.x - em * CHANGE_LANE_EMS`, `width = em * CHANGE_LANE_EMS`, `y = 0`.
- Line numbers end `em * CHANGE_GAP_EMS` left of `change_lane.x` (origin
  `x = change_lane.x - em * CHANGE_GAP_EMS - line.width()`). The gutter's left part (where the
  diagnostics' dot goes) keeps its 2 em, so the dot and the digits keep their distance from the gutter's
  left edge.

So, left to right: diagnostics dot area | digits | 0.5 em gap | 0.5 em change lane | fold column (0.75 em
pad, chevron box, 0.75 em pad) | text. The bar's left edge is the lane's left edge, so it is 0.5 em clear
of the digits and, with a 3 px bar, at least `1.25 em - 3 px` clear of the chevron box.

### Painting (fe-gpui)

In `crates/fe-gpui/src/element/decorations.rs`:

- Change marks are placed in `frame.change_lane`, not at `frame.fold_column.x`.
- `BAR_WIDTH` becomes 3 px, rounded to device pixels as now (`(BAR_WIDTH * scale).round().max(1.) / scale`).
- `GutterMark::Bar`: one rect per run of consecutive display rows (each `row.row` one more than the
  previous) among the frame's rows of the decoration's covered rows, not one per row. A soft-wrapped
  covered row adds its pieces to the run; a fold or scrolling just shortens the shown rows; a block row
  between two rows (row numbers not consecutive) ends a run. The rect is
  `x = change_lane.x`, `width = bar`, `y = row_top(first) + 1 px`, `height = run_rows * line_height - 2 px`:
  inset 1 px at both ends, so two adjacent hunks show a 2 px gap between their bars.
- `GutterMark::Deletion`: a short bar spanning the lane, `x = change_lane.x`,
  `width = change_lane.width`, height 3 px rounded to device pixels like the bar, centred on the same row
  edge as today (top edge of the first covered row, or the last row's bottom edge for a deletion past the
  end). It never extends left of the lane, toward the digits. Drop the `DELETION` constant.
- Rounded ends: `Placed` gets a `corner_radius: Pixels`, half the smaller side for `Gutter(_)` marks and
  zero for every other kind; `Decorations::paint`'s `Layer::Gutter` arm paints
  `fill(bounds, color).corner_radii(corner_radius)` (`PaintQuad::corner_radii` exists in gpui-pre 0.3.7).
  The other layers paint as now.
- Colours are unchanged here (`tint_color` and `GUTTER_ALPHA` 0.8 stay); theme slots for change colours
  are a separate card.

### Tests

- New `crates/fe-display/src/layout/change_lane_tests.rs` (declared `#[cfg(test)] mod change_lane_tests;`
  in `layout.rs`), against the fake monospace measurer of `layout::tests` (em 10), laying out files of
  3-, 4- and 5-digit row counts.
- New `crates/fe-gpui/src/element/change_bar_tests.rs` (declared from `decorations.rs` beside
  `decorations_tests`), driving `EditorView` like `decorations_tests.rs` does.
- Existing tests change only where geometry moved: the 1 em wider gutter in
  `layout/tests.rs` (`GUTTER` and its doc line), `fold_column_tests.rs` (`text_left`, the column's x, and
  "line numbers end at its left edge", which now end `CHANGE_GAP_EMS` left of the lane),
  `long_row_tests.rs` and `slice_cap_tests.rs` where they hard-code gutter-derived numbers; and in
  `decorations_tests.rs`, the bar's x, width and per-run count and the deletion mark's bounds in
  `row_backgrounds_and_bars_cover_every_display_row` and `a_deletion_past_the_end_sits_under_the_last_row`.
- The screenshot goldens move with the gutter: regenerate them with `UPDATE_SCREENSHOTS=1` for both
  examples (as T167 did when the fold column changed).

Put design notes in code docs. Don't append an Implementation notes section to this spec.

## Helium: goldens (lead, respec rev 2)

Both screenshot examples are Metal-only; on helium (Linux) they print "skipped" and exit 0, so the 9 goldens
(8 fe-gpui + fe-workbench workbench.png) can't be regenerated here. C5 is relaxed for helium: leave the golden PNGs
unchanged in this lane. The Mac regenerates them with UPDATE_SCREENSHOTS=1 when this card lands there in a batch
(the helium lead flags it in the batch notes). Stale goldens are expected on helium and are not a review finding.
