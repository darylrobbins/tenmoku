---
task_uid: "01m4p397nkf4fw2yxgd2jxdebj"
task: "T6"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-display test layout::inlay_tests::inlay_shifts_the_rest_of_its_row: \"abcd\" with inlay \"XYZ\" at offset 2 shapes \"abXYZcd\", x_for_offset gives column 2 for offset 2 and column 6 for offset 3, offset_at inside the inlay gives 2, and Frame::inlay covers columns 2..5"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display test layout::inlay_tests::rects_follow_the_inlay: a selection over \"cd\" and a cursor at offset 3 lay out past the inlay, and the inlay's chars carry its style run"
    required: true
    manual: false
  - id: "C3"
    text: "fe-display test layout::inlay_tests::other_rows_and_tabs_unchanged: rows without the offset lay out as with no inlay, a tab after the inlay keeps its width, and an offset outside a long row's shown slice adds nothing"
    required: true
    manual: false
  - id: "C4"
    text: "Existing fe-display layout tests pass unchanged: layout_frame, layout_frame_with_blocks and FrameInput keep their signatures"
    required: true
    manual: false
---

# Lay out one inlay in a frame in fe-display: text drawn inside a row at a buffer offset, with the rest of the row and every position on it shifted past it (Mac T389)

Filed on argon from Mac T389 (card ~/coordination/argon-cards/T389.json, Mac lead message ~/coordination/from-mac-lead-2026-10-11-argon-b.md). Mac task uid 01m4p1v3scfqnyf8f5pzegthh6.

## Problem

Ghost text (M8, user 2026-10-11: "inline AI suggestions rendered as dimmed virtual text at/after the caret")
needs the first line of a suggestion drawn inside the caret's row, with the rest of that row pushed right, and
every position on the row (caret, selection, search and diagnostic rects, clicks) agreeing with what's drawn.
The plan's crate table gives fe-display "our own inlay/ghost-text and block layers"; `crates/fe-display/src/lib.rs`
says "inlays come later" and nothing exists. All x positions on a row go through one mapping,
`RowLayout::x_for_offset` / `offset_for_x` (`crates/fe-display/src/layout.rs` ~273-315) over the row's
`ExpandedRow` (`crates/fe-display/src/lib.rs`), and rows are shaped at one place in `layout_frame_with_blocks`
(`ExpandedRow::with_start_column` then `row_runs` then `measure.shape`, ~layout.rs:614-621). So one inlay can be
added there without touching the display map, wrapping or movement.

## Change

- `crates/fe-display/src/lib.rs`: `ExpandedRow::insert_inlay(&mut self, buffer_col: usize, text: &str)` inserts
  `text` (one line) into the expanded text at `buffer_col`'s display column. After it, `to_display` of a buffer
  column past `buffer_col` adds the inlay's length; `to_display(buffer_col)` is the column before the inlay;
  `to_buffer` of a display column inside the inlay is `buffer_col`. Tab widths don't change. At most one inlay
  per row (a second call panics in debug). Update the `//!` line ("inlays come later") to say one inlay per frame.
- `crates/fe-display/src/layout.rs`: `pub struct Inlay<S> { pub offset: usize, pub text: String, pub style: S }`
  and `layout_frame_with_inlay(input, blocks, inlay: Option<&Inlay<M::Style>>, measure, long_rows, widths,
  highlights, fold_candidates)`; `layout_frame_with_blocks` keeps its signature and calls it with `None`
  (`FrameInput` is unchanged, so no existing caller changes). The display row that `contains` the inlay's
  offset, if the offset is in its shown range, is shaped with the inlay inserted and a style run of
  `inlay.style` over it (the row's own runs split around it). Its width may pass the wrap width: it is not
  wrapped. `Frame` gains `pub inlay: Option<Rect>`, where it was laid out.

Not in this card: more than one inlay, inlays in the display map or wrap map, fe-gpui.

## Tests

New file `crates/fe-display/src/layout/inlay_tests.rs`, declared `#[cfg(test)]` from `layout.rs`, with the
monospace measurer of `crates/fe-display/src/layout/tests.rs`.
