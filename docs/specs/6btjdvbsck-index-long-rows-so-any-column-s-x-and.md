---
task_uid: "01m417z3vbhyx8c16btjdvbsck"
task: "T9"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display proptest long_row::tests::index_matches_measuring_the_prefix: with tabs, wide and non-ASCII chars, x_for_offset and the display column at any char boundary equal summing CharWidth over the expanded prefix"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display test long_row::tests::slice_is_bounded_and_covers_the_view: for 10 MB ASCII and mixed rows, slice(x_range, margin) covers x_range plus the margin on char boundaries, and its byte length doesn't grow with row length"
    required: true
    manual: false
  - id: "C3"
    text: "fe-display proptest long_row::tests::expanded_slice_matches_whole_row: ExpandedRow::with_start_column on any suffix equals the matching part of expanding the whole row"
    required: true
    manual: false
  - id: "C4"
    text: "fe-display test long_row::tests::index_builds_lazily: a query near the start of a 10 MB row measures only the first checkpoints (counting CharWidth)"
    required: true
    manual: false
---

# Index long rows so any column's x and display column are found without shaping the row

Paddock T-17, part 1 of 2: a GPUI-free index of positions in long rows (P2, draft: needs the user's
approval before `rgt task ready`). Part 2 (T-17b) uses it to shape only the visible slice.

## Problem

`layout_frame` (`crates/fe-display/src/layout.rs`) shapes only the first `MAX_SHAPED_ROW_BYTES` (8 KB) of
each row. In minified files and data dumps, everything past that is invisible and unreachable, and clicks
and the cursor clamp to the cut (`layout::tests::long_rows_are_cut_off_and_end_the_highlight_query`). To
shape only the part of a row that's on screen, the layout must find, without shaping the row:

- the byte offset at a given x;
- the x and display column of a given byte offset (tabs expand by display column, so a slice starting
  mid-row needs its starting column).

Doing that by summing char widths from the row start each frame is O(row length): about 10 MB of work
per frame for a 10 MB line.

## Change

Add `crates/fe-display/src/long_row.rs`, declared and re-exported from `crates/fe-display/src/lib.rs`:

- `RowIndex`: checkpoints every 4 KB of one buffer row, each holding `(byte offset, display column,
  x)`. It is built lazily from the row's text (`fe_text::Text` plus the row's range) through
  `CharWidth`, with the same tab rule as `ExpandedRow` (tab stops by display column, `tab_size`). It
  extends only as far as a query needs. Fast path: when the 128 ASCII widths are all equal (a monospace
  font), a chunk that is ASCII with no tabs advances by `len × advance` without per-char work.
- Queries: `x_for_offset(offset)`, `offset_for_x(x)` (the char boundary at or before `x`), `width()` (the
  whole row, which forces a full build), and `slice(x_range, margin_px) -> RowSlice { range, start_x,
  start_column }`. The slice runs from a char boundary at or left of `x_range.start - margin` to one at
  or right of `x_range.end + margin`.
- `ExpandedRow::with_start_column(text, start_column, tab_size)` in `lib.rs`. It expands a slice whose
  first char is at display column `start_column`, so its tabs land where they would in the whole row.
  `ExpandedRow::new` becomes `with_start_column(text, 0, ..)`.
- `LongRows`: a small cache of `RowIndex`es keyed by `(buffer row, snapshot version)`, cleared when the
  version changes. T-17b owns it frame to frame.

Tests go in `long_row.rs`'s own test module, against `Monospace` and a fake measurer with wide chars.

## Open questions

- The source card offers "a fixed advance for runs of monospace ASCII, or a per-row cache of chunk
  widths". This card does both: checkpoints, with the fixed-advance fast path inside them. An edit to a
  long row drops its index, so the next frame re-scans up to the visible column. With the ASCII fast path
  that's about a memory scan (around 1 ms for 10 MB). Confirm that's acceptable for typing inside a 10 MB
  line.

## Implementation notes

- Offsets in `RowIndex` and `RowSlice` are buffer offsets, the same as `RowLayout::start`, so T-17b
  can pass `slice.range` straight to `Text::slice_to_string`. `column_for_offset` exposes the display
  column alongside `x_for_offset`.
- The fast path checks the 95 printable ASCII widths (`' '..='~'`), not all 128. A real font measures
  control chars as zero or a fallback glyph, so requiring all 128 to match would never take the fast
  path. A chunk takes it only if every byte is printable ASCII, which also rules out tabs.
- All ASCII widths are measured once, when the index is made. Non-ASCII chars go to the `CharWidth` on
  each walk, and each query passes the measurer in (`&mut dyn CharWidth`, as `DisplayMap::sync` does).
- X is `f64` throughout the API (`x_for_offset`, `offset_for_x`, `width`, `slice`, `RowSlice::start_x`),
  and `CharWidth` stays `f32`. At 10 MB along a row, x is tens of millions of pixels, where an `f32`
  is off by several and summing `f32` widths drifts by millions. Sums and multiples of `f32` widths are
  exact in `f64` below about 2^31, so the fast path's `len × advance` equals summing the chars one by
  one, bit for bit. C1 asserts exact equality with fractional widths (7.2, 3.3, 14.4). A tab adds one
  space width per column, the same as measuring its expanded spaces. T-17b should keep scroll x in
  `f64` and hand GPUI only slice-relative positions.
- `offset_for_x` on a fast chunk treats the quotient as an estimate and corrects it against
  `from.x + n × advance`, so it always returns the last boundary at or before x.
- A query left of the row's start walks at most to the first checkpoint, so a fresh index's first
  view looks at only a few KB. `index_builds_lazily` checks this by counting the bytes scanned, using a
  `#[cfg(test)]` counter.
- `LongRows::index` takes a `BufferSnapshot` and keys on its buffer id and version, so switching
  buffers can't reuse a stale index. It holds up to 128 rows, then starts over. A font or tab size
  change must call `LongRows::clear`. T-17b will need `DisplayMap` to expose its snapshot.
