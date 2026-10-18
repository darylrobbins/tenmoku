---
task_uid: "01m56d1t4nteh9naw97dx7hsvq"
task: "T191"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-display unit test layout::inlay_hints_tests::long_row_hint_inlay_tests::an_inlay_before_a_hint_still_fills_the_view_past_the_hint (9,000-char row, 100-char inlay at 0, 100-char hint at 8,250, scroll.x 83,000, autoscroll off) fails on T931's code and passes: the slice covers the view and the hint is in Frame.hints"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display unit test layout::inlay_hints_tests::long_row_hint_inlay_tests::long_rows_fill_the_view_with_hints_and_the_inlay_anywhere passes: with the inlay before, between, after and at the same offset as a hint, at every 2,000 px of scroll the slice covers the view and x_for_offset/offset_for_x round-trip every offset in it"
    required: true
    manual: false
  - id: "C3"
    text: "layout::inlay_hints_tests, inlay_tests, long_row_tests, slice_cap_tests and tests/inlay_props.rs pass unchanged (inlay_hints_tests.rs gains only the #[path] mod line for the new file); cargo nextest run -p fe-display and cargo clippy -p fe-display --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Keep a long row's view filled when it scrolls past an inlay hint with an IME composition or ghost text earlier in the row, by mapping the view through hints and the inlay together (T931 F1) (Mac T977)

Filed on argon from Mac T977 (card ~/coordination/argon-cards/zzu/T977.json rev 1; note zzu). Mac T931 = argon T168.

**Argon adaptation (lead, 2026-10-18):** rgt's regression check on argon runs the regression test at the base with test files carried over, but not `layout.rs`. A `mod` line in `layout.rs` would leave the base with 0 tests (an infra block, as T105 and T102 hit). So declare the new file from the existing test module `crates/fe-display/src/layout/inlay_hints_tests.rs` instead, with `#[path = "long_row_hint_inlay_tests.rs"] mod long_row_hint_inlay_tests;`. That line is the only change to `inlay_hints_tests.rs`, and it is the one exception to C3's "unchanged".

The tests' module path is therefore `layout::inlay_hints_tests::long_row_hint_inlay_tests::...`. Everything else in the spec below holds as written; read its "declared in layout.rs" as this.

## Problem

Review finding F1 (medium) on argon's T168 = Mac T931 (landed on argon as 6c85b8e, review xjk3rh by codex,
2026-10-18; argon-lead.md "T168 F1"). In `crates/fe-display/src/layout.rs`, the long-row closure
`shape_long` (6c85b8e:1045-1110) maps the view into the row index's coordinates in two separate passes:

- `unhint` (6c85b8e:1062) takes out every hint's width, as if no inlay were in the row, and
  `let view = unhint(row_view.start)..unhint(row_view.end);` (6c85b8e:1074) picks the slice from that;
- only if that slice can't take the composition / ghost-text inlay in does the second pass `unshift`
  (6c85b8e:1085-1105) take out the inlay's width, and it unshifts the already-unhinted view, so a hint after
  the inlay is counted against the wrong x, and a view whose edge falls on a hint collapses onto the hint.

The reviewer's reproduction, with the 10 px monospace measure the layout tests use: a 9,000-character row, a
100-character inlay at offset 0, a 100-character hint at offset 8,250, `scroll.x = 83000`, autoscroll off.
The slice chosen is 0..8192; its painted text ends at x = 82,920, before the view starts, and the hint is
left out, so the view paints blank. It needs an IME composition (or ghost text) before a hint in a row over
the long-row threshold, scrolled past the hint. T931's tests cover long rows with hints and a hint sharing an
offset with the inlay, but not the two together.

## Change

1. `crates/fe-display/src/layout.rs`, `shape_long`: map the view through every span in the row in one pass,
   in display order: the row's hints (`hint_order[hints_in(display_row)]`, each at
   `index.x_for_offset(hint.offset)` with `hint_width`) and the inlay when it's in the row (at
   `index.x_for_offset(inlay.offset)` with its shaped width, `inlay_shaped`), the inlay after any hint at the
   same offset, as `ExpandedRow` orders them. A view edge inside a span maps to the span's index x, as
   `unhint` does today. This replaces `unhint` and the separate `unshift` pass; `extend_to` the inlay's
   offset stays.
2. Before a slice is shaped, check that the row as laid out from it covers the view: its laid-out x range
   (text plus every span shaped with it) starts at or before `row_view.start` and ends at or after
   `row_view.end` or at the row's end. When the inlay can't fit in the slice, map the view again without the
   inlay's width (it isn't drawn in the row then) and take that slice. The existing narrow-shaper widening
   after `shape_slice` is unchanged.
3. Tests: new `crates/fe-display/src/layout/long_row_hint_inlay_tests.rs`, declared in `layout.rs` beside
   `mod inlay_hints_tests;`, with the monospace measure the other layout tests use, calling
   `layout_frame_with_hints`:
   - `an_inlay_before_a_hint_still_fills_the_view_past_the_hint`: the reviewer's case above. The long row's
     slice covers the view (its first offset's x <= 83,000, and its end x >= 83,000 + view width or the row
     ends), and the hint at 8,250 is in `Frame.hints`. It fails on T931's code.
   - `long_rows_fill_the_view_with_hints_and_the_inlay_anywhere`: a 9,000-character row with two hints and
     the inlay, the inlay before, between, after and at the same offset as a hint; at scroll x every 2,000 px
     from 0 to the row's end, the row's slice covers the view and `x_for_offset` / `offset_for_x` round-trip
     every offset in the slice.

Linux-OK (fe-display is GPUI-free).
