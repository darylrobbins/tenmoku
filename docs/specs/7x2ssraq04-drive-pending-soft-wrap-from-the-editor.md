---
task_uid: "01m417z4jp3zqcgg7x2ssraq04"
task: "T28"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor test tests::pending_wrap::toggle_wraps_the_cursor_rows_first: on 200k lines, toggle_soft_wrap returns with the cursor's rows wrapped and the rest pending; repeated wrap_pending calls finish and match a synchronous wrap"
    required: true
    manual: false
  - id: "C2"
    text: "Small files still wrap within the command: fe-gpui tests::soft_wrap_wraps_to_the_window and fe-editor's existing wrap tests pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Frame work stays bounded on a 1M-line buffer: an fe-editor test shows each wrap_pending call measures at most WRAP_SLICE_BYTES of text, and an fe-gpui test shows prepaint advances wrapping at most one slice per frame and requests another frame while rows are pending, for both a toggle and an em-wide resize. Release frame_timings from fastedit --perf go in the decision file when the screen is unlocked, but aren't required"
    required: true
    manual: false
---

# Drive pending soft wrap from the editor and the frame loop so toggling or resizing never stalls a frame

Paddock T-24, part 2 of 2: wire incremental soft wrap into the editor and the frame loop (P2, draft:
needs the user's approval before `rgt task ready`). Depends on T-24a, which adds
`DisplayMap::wrap_pending`.

## Problem

After T-24a, `DisplayMap::set_wrap_width` only marks rows pending. Nothing wraps them yet. Today the
wrap runs from two places:

- `EditorModel::apply_wrap` (`crates/fe-editor/src/lib.rs`), behind `set_soft_wrap`, `toggle_soft_wrap` and
  `set_wrap_width`; and `set_char_widths`, which calls `display.rewrap`.
- `EditorElement::prepaint` (`crates/fe-gpui/src/element.rs`), which calls `model.set_wrap_width(
  layout::wrap_width(..))` and `model.sync_display()` every frame. A resize that fits another em
  re-wraps the whole file inside that frame.

On a 1M-line file that is a ~465 ms frame (PERF.md / `cargo bench -p fe-display`).

## Change

- `crates/fe-editor/src/lib.rs`: add `EditorModel::wrap_pending(&mut self, around_row: u32) -> bool`. It
  runs one `WRAP_SLICE_BYTES` slice through `self.widths` and returns whether rows are still pending.
  `apply_wrap` and `set_char_widths` run one slice around the primary cursor's buffer row right away, so
  small files (every existing test) are fully wrapped by the time the command returns, exactly as now.
- `crates/fe-gpui/src/element.rs`: in `prepaint`, after `sync_display`, if wrapping is pending, run one
  slice around the buffer row at the top of the viewport (from the current scroll and the display map),
  then call `window.request_animation_frame()` so the next frame continues. Frames keep painting with
  the rows wrapped so far. When nothing is pending, nothing changes.
- Model-level tests go in a new `crates/fe-editor/src/tests/pending_wrap.rs`. Declaring it in
  `tests/mod.rs` is a one-line touch outside scope.

## Open questions

- None beyond T-24a's. The 1 ms parse budget and this wrap slice share a frame. If both are busy at once
  (a resize while a reparse runs), a frame can spend ~3 ms on them. That is acceptable at 120 Hz.

## Implementation notes

- `sync_display` no longer wraps. `set_wrap_width` and `set_soft_wrap` run one slice around the cursor, and only when the width changed. `fit_wrap_width` is the frame loop's variant: it changes the width and wraps nothing.
- `prepaint` calls `EditorModel::wrap_pending_at(top)`: one slice around the top viewport row's buffer row, returning the top that keeps the top row's first char (its buffer offset, through `row_for_offset`) at the same height, fraction included. Explicit autoscroll still wins. This replaces the continuation-row index that review F3 flagged; `widening_from_a_continuation_row_keeps_its_text_in_place` (model) and `large_files_wrap_across_frames_keeping_the_top_row` (GPUI, toggle and resize through completion) cover it, and the GPUI test fails without the restore.
- C3: a slice's budget is soft in `fe-display`: it stops at the first row that reaches `WRAP_SLICE_BYTES`, so it measures at most `WRAP_SLICE_BYTES` plus that one row. The tests assert exactly that bound (`every_slice_measures_at_most_a_slice_of_text`, `each_frame_wraps_at_most_one_slice`), on all-non-ASCII text so every char is really measured (ASCII widths are cached). The GPUI test also measures the frames that start the work: alt-z's command (one slice around the caret, plus the 128-char ASCII width cache) is read before its frame draws, and the frame after it and the first frame at the narrower width each wrap one slice (the latter after caching ASCII widths once). It fails if prepaint stops requesting frames, wraps a second slice, or wraps an extra slice only on a frame where the width changes. The test window can redraw for the keystroke itself before the toggle; that draw wraps nothing, so the toggle's draws are held to one slice in total.
- `fastedit --perf` gained a soft-wrap phase (alt-z, an em narrower, an em wider, alt-z), printing `frame_timings` and time to finish for each. It wasn't run: the screen was locked.
- In the shared target dir, other lanes' crates can end up in this lane's test binary (`fe-syntax` signature errors); touching the sources and rebuilding fixes it.
