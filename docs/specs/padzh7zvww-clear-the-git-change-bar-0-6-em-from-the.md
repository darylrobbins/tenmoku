---
task_uid: "01m4xeyhj24g8aytpadzh7zvww"
task: "T717"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-display: CHANGE_LANE_EMS is 1.0 and FOLD_COLUMN_EMS 3.25, so gutter_width and text_left are unchanged (layout/tests.rs and every screenshot golden pass unedited). change_lane is the 1 em strip ending at text_left, flush with the fold column. layout::change_lane_beside_text_tests::a_bar_at_the_lane_edge_clears_the_text_by_0_6_em: change_lane.x + 3 + 0.6 em \u003c= text_left at 3, 4 and 5 digits, and chevrons end \u003e= 0.5 em left of the lane"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui element::decorations::change_bar_tests::the_bar_clears_the_text_by_0_6_em: every placed Gutter(Bar) sits at the lane's left edge, 3 px wide at alpha 0.6, with its right edge \u003e= 0.6 em left of the text area's left edge, where em is measured independently from the theme's font and size with the same em_advance and fallback calculation as layout (not derived from change_lane.width)"
    required: true
    manual: false
  - id: "C3"
    text: "change_bar_tests::a_deletion_is_a_full_colour_wedge_half_a_line_tall: a Gutter(Deletion) is placed at x = lane left, width lane.width/2, height line_height/2, centred on the row edge (the first covered row's top edge, or past the end of a text whose last row isn't empty the last row's bottom edge), in theme.change_deleted at alpha 1.0. deletion_wedge_tests: wedge_points gives (left,top), (right,centre y), (left,bottom), painted as a filled path in the gutter layer after the bars"
    required: true
    manual: false
  - id: "C4"
    text: "Clicks: change_bar_click_tests pass unedited. view deletion_wedge_click_tests: clicks at the lane centre x, line_height/4 - 1 px above and below a deletion's edge, emit ChangeBarClicked with its rows and move the caret there. A click at that row's vertical middle emits nothing. The end-of-text case also hits"
    required: true
    manual: false
  - id: "C5"
    text: "The only edited existing expectations are the ones the body lists (lane/fold constants, fold_gutter_tests.rs COLUMN and LANE, change_lane_beside_text_tests order and no_overlap, fold_column_tests widths, and the deletion-mark asserts in decorations_tests, change_bar_tests and change_rail_tests). cargo nextest run -p fe-display -p fe-gpui and cargo clippy -p fe-display -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Clear the git change bar 0.6 em from the text with a 1 em change lane, and mark deleted rows with a full-colour red wedge

## Problem

The user, 2026-10-14, with a screenshot of an editor showing a green added-rows bar in the gutter at rows
4–5 sitting about 3 pt left of the text: "The new line indicator is still too close to the text. Do we not
have an indicator that lines were removed like a red line showing where they once were on the gutter?"

At main 440f8097:

- **The bar crowds the text.** `Frame::change_lane` (`crates/fe-display/src/layout.rs`, in
  `layout_frame_with_blocks`, ~line 625) is the strip `text_left - em * CHANGE_LANE_EMS ..text_left`, with
  `pub const CHANGE_LANE_EMS: f32 = 0.5` (line 40). `Decorations::layout`
  (`crates/fe-gpui/src/element/decorations.rs`, the `GutterMark::Bar` arm) draws the 3 px (`BAR_WIDTH`)
  bar at `lane.x`, so only 0.5 em − 3 px (about 1–3 px at the usual font sizes) separates the bar from
  the first text column. History: 147b9f1a (T484) gave the bar a 0.5 em lane beside the numbers; ef074cdf
  (T587) moved that lane to just before the text, after the fold column.
- **The deletion mark is easy to miss.** The `GutterMark::Deletion` arm draws a 3 px tall, lane-wide
  (0.5 em, ~4–6 px) dash on the row edge where rows were deleted, at `GUTTER_ALPHA` 0.6 of
  `theme.change_deleted` (T583 made it a "small red tick" on purpose). The user didn't see it: they asked
  whether a deletion indicator exists at all.

## Change

### 1. A 1 em lane, taken from the fold column (fe-display)

In `crates/fe-display/src/layout.rs`:

- `CHANGE_LANE_EMS` 0.5 → **1.0** and `FOLD_COLUMN_EMS` 3.75 → **3.25**. Their sum is unchanged, so
  `gutter_width`, `text_left`, the fold column's left edge and the line numbers' positions don't move:
  `layout/tests.rs` (`GUTTER` 97.5) and every screenshot golden stay as they are.
- The chevron (`FOLD_CHEVRON_EMS` 2.25, unchanged) stays centred in the narrower column, so it has 0.5 em
  on each side (was 0.75): 1 em clear of the digits (0.5 gap + 0.5 pad) and 0.5 em clear of the lane.
- Update the doc comments on both constants and on `FOLD_CHEVRON_EMS` ("0.75 em each" → "0.5 em each").

With the bar still at the lane's left edge, the bar-to-text gap becomes 1 em − 3 px: 7 px at em 10,
≥ 0.6 em for any em ≥ 7.5 px. The bar itself is unchanged: 3 px, rounded, inset 1 px, `GUTTER_ALPHA` 0.6.

### 2. The deletion mark as a red wedge (fe-gpui)

In `crates/fe-gpui/src/element/decorations.rs`, the `GutterMark::Deletion` arm keeps its edge rule (the
top edge of the first covered row; past the end of a text whose last row isn't empty, the bottom edge of
the last row) and places, with `edge` that row edge, `lane` the frame's change lane and `lh` the line
height:

- bounds `x = lane.x`, `width = lane.width / 2` (0.5 em), `y = edge - lh / 4`, `height = lh / 2`
  — half a line tall, centred on the edge, base flush with the bars' left edge, its tip 0.5 em clear of
  the text;
- colour `theme.change_deleted` at a new `DELETION_ALPHA = 1.0` (full colour, not `GUTTER_ALPHA`);
- no corner radius.

Add `fn wedge_points(bounds: Bounds<Pixels>) -> [Point<Pixels>; 3]`: (left, top), (right, vertical
centre), (left, bottom) — a triangle pointing toward the text, its tip on the row edge. In
`Decorations::paint`'s `Layer::Gutter`, paint deletion marks after the bars (so a wedge sits over the end
of an adjacent bar) as a filled path (`PathBuilder::fill()`, `move_to`/`line_to` the three points,
`close`, `window.paint_path`), as `paint_chevron` in `element.rs` does for strokes; bars stay rounded
quads. A wedge on the first row's top edge at scroll 0 is clipped like the rest of the editor's paint.

### 3. Clicks still hit it (fe-gpui view)

`crates/fe-gpui/src/view/change_bar.rs` hit-tests the placed bounds widened by `MARK_REACH` vertically,
across the whole lane width (`change_at` checks the lane first), and matches the change by the mark's
centre y against row edges, so the taller wedge needs no new logic. Update `MARK_REACH`'s doc (it is now
slack beyond the wedge, not "the mark's half height"); keep the value 3 px. The hand-cursor hitboxes in
`element.rs` already follow the placed bounds' height (not in scope).

### Tests

The regression tests go in existing test files the change edits anyway, because Regatta's base run only
overlays changed test files and a new file's `mod` line lives in src: each uses only base API so it
compiles, and fails, at base.

- `crates/fe-display/src/layout/change_lane_beside_text_tests.rs`: new
  `a_bar_at_the_lane_edge_clears_the_text_by_0_6_em` — for 3-, 4- and 5-digit gutters, scrolled to the top
  and the bottom, `change_lane.x + 3 + 0.6 * CELL <= text_left`, the lane is 1 em wide ending at
  `text_left`, and every chevron box ends ≥ 0.5 em left of `change_lane.x`. Fails at base (2 px < 6 px).
- `crates/fe-gpui/src/element/change_bar_tests.rs`: new `the_bar_clears_the_text_by_0_6_em` — every
  placed `Gutter(Bar)`'s right edge is ≥ 0.6 em (the editor font's em the frame was laid out with) left
  of the text area's left edge; and new `a_deletion_is_a_full_colour_wedge_half_a_line_tall` — the
  bounds and colour of section 2, on a row's top edge and on the bottom edge past the end of the text.
  Both fail at base.
- New `crates/fe-gpui/src/element/deletion_wedge_tests.rs` (declared from `decorations.rs` beside
  `change_bar_tests`): `wedge_points` of a bounds gives the three points above; a drawn deletion paints
  in the gutter layer.
- New `crates/fe-gpui/src/view/deletion_wedge_click_tests.rs` (declared from `change_bar.rs` with
  `#[cfg(test)] #[path = "deletion_wedge_click_tests.rs"] mod …;`, so `view.rs` isn't touched): on a
  text with one deleted row, clicks at the lane's centre x at `edge - (lh / 4 - 1 px)` and
  `edge + (lh / 4 - 1 px)` each emit `ChangeBarClicked { rows }` with the deletion's rows and put the
  caret at its row; a click at that row's vertical middle emits nothing; the same at the end of the text.

Edited expectations, and only these: `change_lane_tests::the_constants_keep_their_values` (1.0, 3.25);
`change_lane_beside_text_tests::order` (lane `x: text_left - CELL, width: CELL`) and `no_overlap` (chevron
gap 0.75 → 0.5 em); `fold_column_tests` (column width 37.5 → 32.5, padding 7.5 → 5, centre
`32.5 / 2`); the deletion-mark expectations in `decorations_tests.rs`, `change_bar_tests.rs`
(`assert_deletion`, `a_deletion_spans_the_lane_on_a_row_edge` renamed to describe the wedge) and
`change_rail_tests.rs`. `change_bar_click_tests.rs`, `fold_gutter_tests.rs`, `layout/tests.rs`,
`fold_hover_tests.rs` and the screenshot goldens pass unedited.

Not in this card: moving the lane elsewhere in the gutter, changing the bar's width or alpha, or the
bar/wedge colours.

## Implementation notes

- `fold_gutter_tests.rs` hardcoded the old 3.75 em column and 0.5 em lane, so it could not pass unedited; its `COLUMN`/`LANE` constants now read 3.25/1.0 em (the only edit).
- `deletion_at` hit-tested the placed bounds horizontally, which are now half the lane; it now spans the whole lane (width) while keeping the wedge's vertical extent plus `MARK_REACH`, as the spec intends.
- The new click tests keep their own helpers, since `change_bar_click_tests.rs` must stay unedited.
