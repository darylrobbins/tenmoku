---
task_uid: "01m46hhmhwf2b7gy8r195sn7x7"
task: "T60"
rev: 4
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display integration test crates/fe-display/tests/shaping_caps.rs::huge_cluster_is_never_shaped_whole (public API only, its own fake TextMeasure/CharWidth recording every string passed to shape and cluster_width): for a row of \"a\" + U+0301 x (1\u003c\u003c20) and for a ZWJ chain of 256K man+ZWJ then woman, layout_frame at scroll 0, mid-row and row end, and RowIndex::slice over the cluster, never pass a string longer than MAX_SHAPED_ROW_BYTES plus 4 bytes of char-boundary slack to shape or cluster_width; fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display integration test crates/fe-display/tests/shaping_caps.rs::widening_never_shapes_past_the_cap: a row of 500 x \"a\" then FAMILY emoji x 400K, with a fake shaper at scale 0.5 / cluster_scale 0.1 and one that returns zero width, makes layout_frame shape at most 3 times per row and never more than MAX_SHAPED_ROW_BYTES (plus char-boundary slack) per shape; fails at base"
    required: true
    manual: false
  - id: "C3"
    text: "Well-behaved shapers keep T41's guarantees: layout::cluster_row_tests (emoji_row_slice_covers_the_view, short_slice_is_widened, index_measures_clusters_as_units for clusters under the size limit, ascii_rows_skip_cluster_measuring) and layout::long_row_tests pass; slices_cover_the_view_however_narrowing_changes is updated to assert coverage where the capped slice can reach the view and the capped range otherwise"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui GlyphWidths::cluster_width does not shape input over the cluster size limit (falls back to summed char widths), with a doc comment naming the limit; cargo nextest run -p fe-gpui passes (the gate runner; plain cargo test already fails at base on tests::injected_languages_compiled_meanwhile_draw_again, which relies on process-per-test)"
    required: true
    manual: false
  - id: "C5"
    text: "docs/decisions/ra1pz1qex0-long-row-slices.md no longer accepts unbounded shaping: it states the slice cap, the cluster size limit, the rendering degradation for pathological clusters and narrowing shapers, and that bounded work is chosen over exact coverage"
    required: true
    manual: false
---

# Cap long-row slices and cluster measuring so one huge grapheme cluster or a narrowing shaper can't shape megabytes

Security finding from the background review of the T41 push (c896164), confirmed with the fake shaper.
T41 made long-row slices snap to grapheme clusters and widened short slices; both paths can shape
an unbounded amount of text, defeating the 8 KB `MAX_SHAPED_ROW_BYTES` cap that T29 introduced.
`docs/decisions/ra1pz1qex0-long-row-slices.md` (lines ~57-77) currently accepts both costs; this
task reverses that: **bounded work wins over exact coverage for pathological rows.**

## Finding 1: one huge grapheme cluster (long_row.rs)

Input: `"a" + "\u{301}" x 1M` (2 MB, one cluster), or a ZWJ chain `"👨\u{200d}" x 256K + "👩"`
(1.8 MB, one cluster under UAX29 GB11).
- `RowIndex::slice` (long_row.rs ~166-179) snaps to the cluster start via a `cluster: true` checkpoint
  and to the cluster end via `boundary_after` (~211-223), so any view touching the cluster yields a
  slice equal to the whole cluster.
- `walk` (~324-362) doubles its window and calls `slice_to_string` until it has copied the whole
  cluster; `measure_cluster` (~448-452) passes the full text to `CharWidth::cluster_width`, which in
  GPUI shapes it with `layout_line` (crates/fe-gpui/src/element.rs `GlyphWidths::cluster_width`).
- Observed: `layout_frame` shaped a 2,097,154-byte string and a 1,835,012-byte string.

## Finding 2: widening shapes the rest of the row (layout.rs)

`layout_frame`'s widening (layout.rs ~607-633): if the first shape has zero width the slice becomes
`slice.range.start..row_end`; if the second still ends short of the view, the third sets
`slice.range.end = row_end`. Observed with a row of 500 x "a" then FAMILY emoji x 400K (10 MB) and a
fake shaper at scale 0.5 / cluster_scale 0.1: shapes of [432, 10450, 10000500] bytes. Real macOS
fonts may rarely trigger it (strong contextual narrowing, fallback mismatch), but the cap must hold
regardless of the shaper.

## Fix direction

- long_row.rs: a slice cap (e.g. `MAX_SLICE_BYTES`, tied to `MAX_SHAPED_ROW_BYTES`) that `slice` and
  widening enforce. When a cluster boundary would push a slice past the cap, cut at a char boundary
  inside the cluster instead (the pathological cluster may render slightly wrong; that's accepted).
- Cluster measuring: clusters longer than a small limit (e.g. 256 bytes) are measured without
  `cluster_width` (e.g. as the sum of `char_width`), and the cluster scanner never copies or segments
  more than a bounded window. `GlyphWidths::cluster_width` in fe-gpui refuses over-long input.
- layout.rs: the zero-width path and the third shape clamp to `slice.range.start + cap` (snapped to a
  char boundary) instead of `row_end`. If the capped slice still ends short of the view, the rest of
  the view is left unpainted for that frame; that is the documented degradation.
- Update the existing test `slices_cover_the_view_however_narrowing_changes`, which asserts
  `shown == 0..row.len()` for the narrowing shapers, to the new capped behaviour (it must still assert
  coverage for well-behaved shapers).
- Decision doc: replace the accepted-cost text with the cap, the degradation and why.

## Regression check: keep changed test files compiling at base

The regression check (`rgt finish`) builds the base with this task's changed test files overlaid, using
`cargo test --workspace`. So `crates/fe-display/src/layout/cluster_row_tests.rs` must compile against
the base API: it may not use `MAX_CLUSTER_BYTES`, `RowIndex::slice_limit` or any other item this task
adds. Put unit tests that need new items in the new file `crates/fe-display/src/layout/slice_cap_tests.rs`
(declared by a `#[cfg(test)] mod slice_cap_tests;` line in layout.rs, which the base doesn't have, so the
base never compiles it). `crates/fe-display/src/lib.rs` is in scope for the new constant's export.

## Implementation notes

- **Units.** `RowIndex` now walks units: grapheme clusters, except that a cluster longer than
  `MAX_CLUSTER_BYTES` (256, exported from `fe_display`) is cut at a char boundary at most 256 bytes
  in. The piece is measured as the sum of its chars' `char_width`, and the text after the cut is
  segmented afresh. `Clusters` segments each unit from its own start, never more than 260 bytes at
  a time, so unit boundaries depend only on the text from a boundary on. Each walk window copies
  `MAX_CLUSTER_BYTES + 4` bytes of lookahead past its end and measures only units that start before
  the end. Without that, a 256-byte piece ending exactly at a checkpoint (the window's bound) was
  classified as a whole cluster. `layout::slice_cap_tests::long_clusters_are_cut_into_pieces`
  caught it.
- **Cap.** `RowIndex::slice_limit(start)` is the last unit boundary at most `MAX_SHAPED_ROW_BYTES`
  past `start`, or the row's end. `slice` clamps to it. When clamping is needed, the slice starts
  at most 4 KB left of the view, so the view keeps at least about half the slice. `widen` clamps to
  it too, and `layout_frame`'s zero-width path and third shape use it instead of `row_end`. The
  limit is computed only when a slice falls short, so frames without widening do no extra work.
- **Tests changed.**
  - `long_clusters_stay_whole` (301-byte and 12 KB clusters) is now
    `long_clusters_are_cut_into_pieces`, in the new `layout/slice_cap_tests.rs`: it checks a
    256-byte cluster stays whole, longer ones are cut, and nothing over the limit reaches
    `cluster_width`. It uses `MAX_CLUSTER_BYTES`, so it lives outside `cluster_row_tests.rs`,
    which the regression check builds against base and so must use only the base API.
  - `short_slice_is_widened`: its `(0.05, 4070 px)` case needs about 42 KB of slice to cover a
    4000 px view and is past the cap by design. It's replaced with `(0.1, 400 px)`, which still
    widens and is still covered, margin and all, in at most 2 shapes. The capped extreme is
    asserted in `slices_cover_the_view_however_narrowing_changes`. That test now expects
    `0..8175` (500 `a`s and 307 families) where it expected `0..row.len()`. For each sampled view,
    it asserts that the slice covers the view, reaches the row's end, or ends on the last grapheme
    boundary within `MAX_SHAPED_ROW_BYTES` of its start, computed from the row's own clusters.
  - `clicks_past_a_narrowed_row_land_on_its_end` keeps the row-end case. At scroll 0 it now
    asserts the capped slice and a click landing where the index puts it.
  - The fe-gpui test now shapes a cluster exactly at the limit whole, and sums char widths without
    caching for a 196 KB cluster.
- `tests::injected_languages_compiled_meanwhile_draw_again` fails under plain `cargo test -p
  fe-gpui` because it shares process state with other tests. Its comment says it relies on
  nextest's process per test, and it passes alone and under nextest, as the gate runs it.
