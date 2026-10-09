---
task_uid: "01m4fbwmee316s53ya7f6wq6af"
task: "T254"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display test blocks::screen_rows_round_trip: with blocks above display rows 0, 3 (two blocks) and row_count, at_screen_row(screen_row(r)) is Text(r) for every display row, every other screen row is a Block with lines 0..rows in order and the blocks' ids in input order, and display rows + block_rows() is the screen row count"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display test blocks::rows_below_a_block_move_down: with a 2-row block above display row 5, layout_frame_with_blocks places rows \u003e= 5 two line heights lower than layout_frame does and rows \u003c 5 at the same tops, Frame::blocks holds that block's rect at row 5's old top and 2 line heights tall, row_count is unchanged and screen_rows is 2 more"
    required: true
    manual: false
  - id: "C3"
    text: "fe-display test blocks::hit_testing_skips_blocks: offset_at at a y inside the block gives the end of display row 4, at row 5's new top gives row 5's start, caret_bounds of an offset on row 5 is at its moved top, and screen_row_at agrees for each y"
    required: true
    manual: false
  - id: "C4"
    text: "fe-display test blocks::scrolling_counts_block_rows: autoscroll to a caret on the last row with blocks above keeps it within the margins, the scroll clamp lets the last screen row reach the top, a block wholly outside the viewport is not in Frame::blocks while a partly visible one is, and to_display_top(to_screen_top(t)) == t for display-row tops"
    required: true
    manual: false
  - id: "C5"
    text: "fe-display test blocks::no_blocks_is_unchanged: with BlockMap::default(), layout_frame_with_blocks gives the same row tops, row ranges, selections, cursors, line numbers and fold toggles as layout_frame; no existing file under crates/fe-display/src/layout/ or crates/fe-display/tests/ changes"
    required: true
    manual: false
---

# Add a block layer to fe-display: whole rows placed between display rows, laid out, scrolled and hit-tested, with display rows and fe-editor untouched

## Problem

The approved plan (2026-10-08) wants a `ProposedChange`'s hunks shown inline in the editor as diff blocks: the
proposed new text in rows of its own under the text it replaces. fe-display can't place anything that isn't
buffer text: `crates/fe-display/src/lib.rs:2` says "inlays and blocks come later", and every row position is
uniform (`Frame::row_top`, `layout_frame`'s `row_top`, autoscroll and the scroll clamp in
`crates/fe-display/src/layout.rs:490-507`, `Frame::offset_at` at :389). The plan's risk list (#4) says to add
display layers "one layer at a time, with round-trip property tests".

## Change

A generic, GPUI-free block layer in fe-display; it knows nothing of proposals. Blocks are whole rows of
`line_height`, placed between display rows, so fe-editor's movement (which counts `DisplayMap` rows) is
untouched: block rows are *screen rows*, never display rows. `DisplayMap` and its API don't change.

- New `crates/fe-display/src/block_map.rs`, exported from `lib.rs`:
  - `pub struct Block { pub above: u32, pub rows: u32, pub id: u64 }`: `rows` rows placed above display row
    `above` (`above == row_count` places it after the last row). `id` is the caller's, echoed back.
  - `BlockMap::new(impl IntoIterator<Item = Block>)` (stable-sorted by `above`; zero-row blocks dropped) and
    `Default` (no blocks). Prefix sums, so lookups are O(log blocks).
  - `screen_row(&self, display_row: u32) -> u32`: `display_row` plus the rows of every block above it.
  - `at_screen_row(&self, screen_row: u32) -> ScreenRow`, `enum ScreenRow { Text(u32), Block { id: u64, line: u32 } }`.
  - `block_rows(&self) -> u32` in all.
  - `to_display_top(&self, screen_top: f32) -> f32` and `to_screen_top(&self, display_top: f32) -> f32`: a
    fractional top in rows; a top inside a block maps to the start of the display row below it. The view needs
    these for `EditorModel::wrap_pending_at`, which takes display rows.
- `crates/fe-display/src/layout.rs`:
  - `pub fn layout_frame_with_blocks(input, blocks: &BlockMap, measure, long_rows, widths, highlights,
    fold_candidates) -> Frame`; `layout_frame` calls it with `&BlockMap::default()` and keeps its signature, so
    `FrameInput` and its existing callers don't change.
  - Rows are placed at `line_height * blocks.screen_row(row) - scroll.y`; the scroll clamp, autoscroll margins
    and first/end row use screen rows; only display rows are shaped.
  - `Frame` keeps the `BlockMap` it was laid out with. New fields: `pub screen_rows: u32` (display rows plus block
    rows) and `pub blocks: Vec<BlockLayout>` (`BlockLayout { id: u64, rows: u32, rect: Rect }`, the text-area-wide
    rect of each block at least partly in view). `row_count` keeps meaning display rows.
  - `Frame::row_top`, `line_origin`, `caret_bounds` and `offset_at` honour blocks. `offset_at` on a block row gives
    the end of the display row above the block (row 0's start for a block above row 0). Add
    `Frame::screen_row_at(&self, y: f32) -> ScreenRow` for hit testing.
- Update the `//!` line in `lib.rs` (blocks now exist; inlays still later).

With no blocks, every frame must be identical to today's: the existing tests under `crates/fe-display/src/layout/`
and `crates/fe-display/tests/` stay unchanged and pass.

Not in this card: fe-gpui, proposals, folds inside blocks, wrapping block text.

## Tests

New file `crates/fe-display/tests/blocks.rs`, through the public API, with its own monospace `TextMeasure`
(as `crates/fe-display/tests/shaping_caps.rs` does; don't edit that file).

## Implementation notes

- `Frame` keeps the map as `pub block_map: BlockMap`, plus a private `block_offsets` beside `blocks`: each
  visible block's screen rows and the offset a click on it goes to, worked out at layout time from the
  `DisplayMap`. That way a block partly in view at the top still hits the end of the row above it even though
  that row is outside the frame. A block row outside the frame (dragging past an edge) clamps like the display
  row below it, as rows outside the frame already do.
- `BlockMap::display_row_from(screen_row)` (crate-private) gives the display row at a screen row, or the one
  below a block; layout uses it for `first_row` and `end_row`, and `offset_at` for its clamp.
- A block's `above` past the row count is treated as the row count when finding the row above it.
- `crates/fe-display/Cargo.toml`'s description still says "inlays and blocks later"; it's outside this
  card's scope.
