---
task_uid: "01m417z4m1dcmwt5ra1pz1qex0"
task: "T29"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display test layout::long_row_tests::shapes_only_the_visible_slice: on a 10 MB single-line text scrolled to start, middle and end, the fake measurer shapes at most a fixed number of bytes per frame, the same at every position"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display test layout::long_row_tests::carets_selections_and_clicks_far_right: at column ~5M, caret_bounds, selection rects and offset_at round-trip, and autoscroll brings a caret at the row's end into view"
    required: true
    manual: false
  - id: "C3"
    text: "fe-display test layout::long_row_tests::highlights_query_the_visible_slice: highlights are queried only for the slice's buffer range and runs land on the right bytes"
    required: true
    manual: false
  - id: "C4"
    text: "Existing layout tests (horizontal_scroll, hit_testing, wrapped_rows, folded_rows) pass, and long_rows_are_cut_off_and_end_the_highlight_query is replaced"
    required: true
    manual: false
  - id: "C5"
    text: "Manual, numbers in the decision file: release build, 10 MB minified file: move to the line end and click there; prepaint is within 1 ms of a short-line file"
    required: true
    manual: false
---

# Shape only the visible slice of rows longer than 8 KB so long lines are fully reachable

Paddock T-17, part 2 of 2: shape only the visible slice of long rows (P2, draft: needs the user's approval
before `rgt task ready`). Depends on T-17a (`RowIndex`, `LongRows`, `ExpandedRow::with_start_column`).

## Problem

Rows are cut at `MAX_SHAPED_ROW_BYTES` (8 KB) in `layout_frame` (`crates/fe-display/src/layout.rs`, the
`shown` ranges). So in minified files and data dumps, everything past 8 KB is invisible and unreachable.
`Frame::offset_at`, `RowLayout::x_for_offset` and `caret_bounds` (which the IME's `bounds_for_range` in
`crates/fe-gpui/src/view.rs` uses) all clamp to the cut. The highlight query stops at the cut too
(`visible_spans`).

## Change

In `crates/fe-display/src/layout.rs`:

- When soft wrap is off and a row is longer than `MAX_SHAPED_ROW_BYTES`, get its `RowIndex` from
  `LongRows` and shape only `index.slice(scroll.x .. scroll.x + text_width, margin)`, where the margin is
  a few hundred pixels either side. Expand it with `ExpandedRow::with_start_column`, query highlights for
  the slice's buffer range only, and keep `MAX_SHAPED_ROW_BYTES` as the threshold for "long". Short rows,
  and every row while soft wrap is on, are laid out exactly as now.
- `RowLayout` records the slice: buffer range, `start_x`, display start. `x_for_offset` and the hit-test
  path in `Frame::offset_at` use the shaped line inside the slice and the index outside it. A caret,
  selection edge or click at any column of the row is therefore placed correctly, and `caret_bounds` (IME)
  is right at any column. Paint each slice at `row_origin.x + start_x`.
- Horizontal scrolling and autoscroll use `index.width()` for long rows instead of the shaped width, so
  the scroll range covers the whole row and the caret can be scrolled to at its end.
- `layout_frame` takes `&mut LongRows` and the snapshot version (a new `FrameInput` field). Update the
  existing layout tests' helper for the new parameter. Replace
  `long_rows_are_cut_off_and_end_the_highlight_query` with tests of the slice. New tests go in
  `crates/fe-display/src/layout/long_row_tests.rs`, declared from `layout.rs`.

In `crates/fe-gpui/src/element.rs`: keep a `LongRows` and a `GlyphWidths` in `FrameLayout`, carry them
over from the previous frame's layout (the view keeps it in `view.layout`), and pass them to
`layout_frame`. Paint uses each row's slice origin.

Out of scope, a follow-up: `EditorModel`'s caret movement (`crates/fe-editor/src/lib.rs`, `row_at` →
`row_to_string`) copies the whole row on every left/right key. That is O(row length) per keystroke on a
10 MB line: correct, but slow.

## Open questions

- Soft wrap is the other half of long-line support. With wrap on, long rows are already wrapped (T-20)
  and are never this long on screen. Nothing to decide unless the user wants slicing inside wrapped rows
  too.
- A 10 MB single-line file is over `MAX_PARSE_BYTES` (8 MB), so it isn't highlighted. Highlighting
  correctness at far columns is tested here with smaller long rows (and fake highlights). Highlighting
  above the parse limit is T-16.

## Implementation notes

- `layout_frame` takes `&mut LongRows` and a `&mut dyn CharWidth`. The snapshot comes from the new
  `DisplayMap::snapshot()` rather than a `FrameInput` field: the display map already holds the
  snapshot its rows are of, so the two can't disagree.
- Short rows are shaped first. Long rows are sliced after the horizontal scroll is settled, since the
  scroll depends on the short rows' widths and the slice on the scroll. Each slice gets its own
  highlight query.
- `Frame::offset_at`, `Frame::caret_bounds` and `RowLayout::x_for_offset` take the long rows and char
  widths the frame was laid out with. `LongRows::get` looks a row's index up without a snapshot.
  `x_for_offset` returns an `f64`, and `Frame::view_x` and `Frame::line_origin` (which replaces
  `row_origin`) subtract the scroll position before converting to `f32`.
- Selection rectangles are clamped to just outside the frame, so a selection far off screen doesn't
  lose its visible edge to `f32` precision on the GPU.
- `fe-gpui`'s `shown_text` (the syntax window) uses each row's shaped range, `RowLayout::shown`.
- C5 numbers are in `docs/decisions/ra1pz1qex0-long-row-slices.md`.
