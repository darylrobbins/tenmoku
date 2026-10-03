---
task_uid: "01m417z3wc5wt009ybj8awq9na"
task: "T10"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display test display_map::pending_wrap_tests::set_wrap_width_defers_rewrap: on 1M lines, set_wrap_width (turning wrap on, and a later width change) wraps no rows and builds no per-row items; rows show their previous wrap and wrap_is_pending() is true"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display test display_map::pending_wrap_tests::slices_wrap_the_viewport_first: one wrap_pending(around, budget) call wraps pending rows from 'around' onward first and stops after budget bytes (at least one row)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-display proptest display_map::pending_wrap_tests::pending_wrap_with_edits_matches_fresh: interleaved edits+sync, width changes and partial slices, run to completion, equal a fresh map, and row/offset mappings round-trip at every step"
    required: true
    manual: false
  - id: "C4"
    text: "fe-display proptest wrap_map::tests::ascii_fast_path_matches_measuring: the cached ASCII table and the fits-without-measuring path give the same breaks as measuring every char"
    required: true
    manual: false
  - id: "C5"
    text: "Manual, numbers in the decision file: cargo bench -p fe-display on 1M lines shows set width plus one slice under 2 ms, and sync/keystroke (wrapped) still in microseconds"
    required: true
    manual: false
---

# Re-wrap soft-wrapped rows in viewport-first slices instead of all at once

Paddock T-24, part 1 of 2: the fe-display half (P2, draft: needs the user's approval before
`rgt task ready`). Part 2 (T-24b) drives it from the editor and the frame loop.

## Problem

Turning soft wrap on, or resizing by an em while it's on, re-wraps every row in one call:
`DisplayMap::set_wrap_width` / `rewrap` → `WrapMap::rebuild` (`crates/fe-display/src/wrap_map.rs`) wraps all
`text.row_count()` rows and builds a new `SumTree`. On a 1M-line file (about 75 MB) that takes about
465 ms (`cargo bench -p fe-display`, `wrap/1M lines at 80 columns`; PERF.md, T-20), and the call happens
on the UI thread inside the editor element's prepaint. Keystrokes are fine at about 15 µs
(`sync/keystroke, 1M lines wrapped`), because `splice` only re-wraps the rows an edit touched.

Also, `Wrapper::new` builds the 128-entry ASCII width table through `CharWidth` on every `splice`, that
is, on every keystroke.

## Change

Make a full re-wrap incremental: it is done in slices of bounded size, rows around the viewport first.
fe-display stays synchronous and GPUI-free. It does no threading or timing; the caller decides when
to run a slice.

- **Pending rows.** `WrapMap` keeps a set of buffer rows still to be wrapped at the current width
  (sorted, merged `Range<u32>`s, shifted by `splice` like folds are). `rebuild` (new width, tab size, font)
  no longer wraps anything: it records the width and marks every row pending. A pending row keeps its
  previous breaks, so it shows at its old wrap; with no previous width it shows unwrapped. Represent runs
  of unwrapped rows as one `SumTree` item with a row count, so turning wrap on for 1M rows doesn't build
  1M items up front.
- **Slices.** `DisplayMap::wrap_pending(&mut self, around: u32 /* buffer row */, budget_bytes: usize,
  widths: &mut dyn CharWidth) -> bool` wraps pending rows nearest `around` first (the rows from `around`
  onward, then the rows before it, then the rest), until it has wrapped `budget_bytes` of row text (at
  least one row). It splices them into the tree and returns whether any rows are still pending.
  `DisplayMap::wrap_is_pending()` reports the same. Folds are rebuilt after each slice. A byte budget, not a
  time budget, keeps tests deterministic. Pick the default slice (`WRAP_SLICE_BYTES`) from the bench so one
  slice stays under about 2 ms in release.
- **Edits while pending.** `sync` re-wraps the rows an edit touched at the new width, as now, and takes
  them out of the pending set. `sync` from another buffer, or from history it can't follow, marks
  everything pending instead of re-wrapping.
- **ASCII.** Cache the ASCII width table in `WrapMap`. Build it when `rebuild` is called (new widths) and
  reuse it on every `splice` and slice. Add a fast path: a row of ASCII without tabs whose byte length ×
  the widest ASCII advance fits the width has no breaks, so skip measuring it.
- **Bench.** In `crates/fe-display/benches/display.rs`, replace the whole-file wrap case with: set the
  width plus one default slice; one slice alone; and wrapping all pending rows to completion. Keep the
  keystroke cases.

New tests go in `crates/fe-display/src/display_map/pending_wrap_tests.rs` (declared from
`display_map.rs`), plus the fast-path test in `wrap_map.rs`'s own test module.

## Open questions

- The source card lists three options: wrap in the background against a snapshot, wrap around the
  viewport first, or an ASCII fast path. This card picks viewport-first slices on the UI thread plus the
  fast path. That avoids sending `CharWidth` (GPUI's text system) to another thread and avoids depending on
  T-05's spawner seam. The cost is that a full wrap of 1M lines finishes over a few hundred frames
  (seconds), not one background pass. Confirm.

## Implementation notes

- Numbers and the decision are in `docs/decisions/sliced-soft-wrap.md`:
  set width + one slice 0.58 ms, wrapped keystroke 13 µs, everything in slices
  394 ms (was 474 ms in one call). `WRAP_SLICE_BYTES` is 128 KiB.
- "Then the rest" turned out to be empty: rows from `around` onward, then
  those before it, nearest first, covers every row.
- The byte budget counts each row's newline, so runs of empty rows still use
  it up.
- Outside the listed scope: `fe-editor`'s `EditorModel` now drains pending rows
  in one `wrap_pending(0, usize::MAX, …)` call after a width, tab size, font
  or sync change, so the editor behaves as before until T-24b drives slices
  from the frame loop. `layout/tests.rs` and the existing `display_map` tests
  also wrap everything after `set_wrap_width`.
- A sync to another buffer resets the tree to unwrapped rows, since the old
  breaks don't apply to the new text.
