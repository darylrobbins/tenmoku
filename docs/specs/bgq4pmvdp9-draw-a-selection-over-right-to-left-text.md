---
task_uid: "01m6w2wg0fwm6vc8bgq4pmvdp9"
task: "T306"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-display --test bidi_selection_rects passes, with rtl_selection_across_a_direction_change_draws_one_rect_per_visual_piece"
    required: true
    manual: false
  - id: "C2"
    text: "lay_out_frame takes each row selection rects from MeasuredLine::visual_ranges, through the same slice, hint and scroll mapping as RowLayout::x_for_offset; cargo nextest run -p fe-display passes with the existing layout tests unchanged (rects on LTR rows are as before)"
    required: true
    manual: false
  - id: "C3"
    text: "A selection running past the end of a row that ends in RTL text draws its line-break mark once, at the right end of the line (a test in the new file); a range wholly inside an RTL run is one rect over exactly its glyphs"
    required: true
    manual: false
---

# Draw a selection over right-to-left text as one rect per visual piece, covering exactly the selected glyphs (T1431 G3) (Mac T1504)

Follow-up of T1431 (docs/decisions/yzegn08wk2-non-latin-text-audit.md, G3), after T1503, which made the fe-gpui Line bidi-aware and added MeasuredLine::visual_ranges.

The defect: fe-display lay_out_frame (crates/fe-display/src/layout.rs, the selection loop around :1495) pushes one rect per display row, from x(start) to x(end). When a range crosses a direction change, that rect covers glyphs that are not selected. Repro: buffer `let s = "שלום"; // مرحبا`, select 5..20 (`= "שלום";`); the highlight is not the selected glyphs. Expected (VS Code): one rect per visual piece, covering exactly the selected glyphs.

Fix: for each row, map the selected range to the line's indices the way RowLayout::x_for_offset does (shown slice, inlay hints, scroll). Ask line.visual_ranges for the pieces and push one rect per piece, clamped as today. LTR rows give one piece, so their rects do not change. A selection running past the row end keeps its line-break mark (em times 0.6), drawn once from the right end of the line. For LTR-only rows that is where it is today.

Regression test: rtl_selection_across_a_direction_change_draws_one_rect_per_visual_piece, in a new file, crates/fe-display/tests/bidi_selection_rects.rs. The tests/** overlay carries it to base. Base for this card is main after T1503, so overriding visual_ranges compiles there.
- Script a Line the way crates/fe-display/tests/read_only.rs does, for the text `ab\u{5e9}\u{5dc}\u{5d5}\u{5dd}cd`: each char one em wide, painted in the order a b ם ו ל ש c d.
- x_for_index and index_for_x follow logical-edge affinity, and visual_ranges is overridden to return the exact pieces.
- Lay out a frame that selects bytes 1..4 (b and ש). assert_eq the x ranges of the selection rects, relative to the text origin, to [1em..2em, 5em..6em]. At base the frame has the single rect 1em..5em.
- Also cover a range wholly inside the RTL run (4..8, ל and ו, which should give 3em..5em), the line-break case, and an LTR-only row whose rects are unchanged.

## Implementation notes

- `RowLayout::x_ranges` maps a buffer range of a row to its visual pieces: the
  part in the shaped slice goes through `ExpandedRow::to_display` and
  `MeasuredLine::visual_ranges`, offset by `start_x`; of a long row, each part
  outside the slice is one piece from `x_for_offset` (the index is
  left-to-right), bounded at the slice by its edge as drawn (`start_x`, and
  the right end of its shaped pieces), not by the caret at `shown.start` or
  `shown.end`, which in RTL text is across the slice (review 52xe9s F1). On
  LTR rows those are the same x. `RowLayout::text_right_x` is the right end of all the row's
  pieces, where the line-break mark goes.
- The selection loop converts the pieces to screen x as before, adds the mark
  (em × 0.6) at `text_right_x`, merges touching pieces and clamps each. On an
  LTR row the pieces and the mark touch at `x(row.end)`, so the rect is the one
  it was, bit for bit.
