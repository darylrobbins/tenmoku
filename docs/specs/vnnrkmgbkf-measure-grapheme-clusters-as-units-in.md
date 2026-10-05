---
task_uid: "01m41xy8tj598tpkvnnrkmgbkf"
task: "T41"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display test layout::cluster_row_tests::index_measures_clusters_as_units: on a row over 8 KB mixing ZWJ family emoji, e+U+0301 and ASCII, with clusters straddling 4 KB marks, RowIndex x_for_offset, offset_for_x and width equal the fake cluster shaper's width of the prefix at every cluster boundary; offsets inside a cluster clamp to its start, and slice ranges start and end on cluster boundaries"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display test layout::cluster_row_tests::emoji_row_slice_covers_the_view: on a long row of family emoji, the shaped slice placed at start_x covers the whole view when scrolled mid-row and at the row's end, and after moving to the row's end and autoscrolling, the caret's x from caret_bounds is inside the text area"
    required: true
    manual: false
  - id: "C3"
    text: "fe-display test layout::cluster_row_tests::short_slice_is_widened: with a fake shaper narrower than the index, layout_frame extends the slice until start_x + shaped width reaches the view's right edge or the row's end, shaping that row at most 3 times in the frame"
    required: true
    manual: false
  - id: "C4"
    text: "fe-display test layout::cluster_row_tests::ascii_rows_skip_cluster_measuring: on a printable-ASCII long row, RowIndex never calls cluster_width and scans no more bytes than before; existing long_row, layout::long_row_tests and wrap_map tests pass unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "The Known limitation section of docs/decisions/ra1pz1qex0-long-row-slices.md says emoji sequences and combining marks are exact, describes the widening safeguard, and says Arabic and Indic still drift slightly"
    required: true
    manual: false
---

# Measure grapheme clusters as units in long-row indexes so emoji and combining-mark rows place their slice and caret correctly

Follow-up to T29 (review finding F1, option B; option A, documenting the limitation, was accepted
2026-10-03 and is recorded under "Known limitation: additive metrics" in
`docs/decisions/ra1pz1qex0-long-row-slices.md`).

## Problem

`RowIndex` (`crates/fe-display/src/long_row.rs`, from T9) finds x by summing each char's
`CharWidth::char_width` measured alone, but `layout_frame` (`crates/fe-display/src/layout.rs`)
shapes the visible slice in context through `TextMeasure::shape`. Where shaping changes widths, the
two disagree and the error grows along the row. A family emoji (ZWJ sequence) is one 17 px glyph
shaped but 68 px summed in Menlo at 13 px; combining marks and Arabic/Indic joined forms drift too.

What the user sees (T29 worker's note): in a long row of family emoji, a slice taken mid-row is drawn
at the index's x but comes out narrower than the index says, so it falls left of the view and the
view is blank; after moving to the row's end, autoscroll uses the index's x and leaves the caret off
screen. Monospace ASCII is exact. Soft wrap (`wrap_map.rs`, T10/T28) uses the same additive metrics
but only moves a wrap point, so it is out of scope here.

## Change

Measure grapheme clusters as units in the row index, and widen a slice that still falls short.

- `crates/fe-display/src/lib.rs`: add `CharWidth::cluster_width(&mut self, cluster: &str) -> f32`,
  with a default that sums `char_width` over the cluster's chars, so `Monospace`, wrap and every
  existing implementation keep their behaviour.
- `crates/fe-display/src/long_row.rs`: `RowIndex` walks extended grapheme clusters
  (`unicode-segmentation`) and adds `cluster_width` for each multi-char cluster (single-char clusters
  keep `char_width` and the ASCII table). Checkpoints sit on the first cluster boundary at or after
  each 4 KB mark, so no cluster straddles a chunk. Offsets inside a cluster clamp to its start;
  `offset_for_x` and `slice` return cluster boundaries only. The printable-ASCII fast path stays,
  valid for a chunk only when the chunk is printable ASCII and the char after it does not extend the
  last cluster (a combining mark). Display columns still count chars, as `ExpandedRow` does.
- `crates/fe-display/Cargo.toml`: `unicode-segmentation.workspace = true`. It is already a workspace
  dependency (used by fe-editor, MIT OR Apache-2.0, allowed by `deny.toml`), so `Cargo.lock` gains only
  fe-display's dependency edge; no `deny.toml`, `THIRD_PARTY.md` or `CLEAN_ROOM.md` change.
- `crates/fe-display/src/layout.rs`: after shaping a long row's slice, if
  `slice.start_x + line.width()` is short of the view's right edge (plus the slice margin) and the
  slice does not reach the row's end, extend the slice's end by the shortfall, re-query highlights for
  the new range and shape again, at most twice per row per frame. Hit-testing and carets inside the
  slice keep using the shaped line.
- `crates/fe-gpui/src/element.rs`: `GlyphWidths` implements `cluster_width` by shaping the cluster
  string with the editor font through the text system, caching widths by cluster string (cache bounded,
  e.g. cleared past 4,096 entries). Single-char clusters go through `char_width`'s cache.
- `docs/decisions/ra1pz1qex0-long-row-slices.md`: rewrite the "Known limitation" section to say
  emoji sequences and combining marks are now exact, the widening safeguard exists, and Arabic and
  Indic still drift slightly (their contextual forms are not cluster-local).

Tests go in a new `crates/fe-display/src/layout/cluster_row_tests.rs`, declared from `layout.rs`.
They use a fake `CharWidth` whose `cluster_width` for a multi-char cluster differs from the sum of
its chars (e.g. a ZWJ family is 2 units, its chars sum to 11), and a fake `TextMeasure` that shapes
by the same cluster rule, standing in for a real shaper. A real-shaper regression needs a test window
with the macOS text system, which fe-gpui tests don't have; that's out of scope here. No manual or
timing criteria.

## Implementation notes

- `long_row::tests::index_matches_measuring_the_prefix` (an existing test)
  had to change its reference model: it checked x at every *char* boundary,
  including the one inside `e` + U+0301, which C1 now clamps to the
  cluster's start. It now sums over cluster boundaries and also checks that
  offsets inside a cluster clamp. The other existing tests are unchanged.
- Segmentation is skipped between chars that can never join a cluster
  (`is_own_cluster`: Latin, Greek, Cyrillic, kana, CJK ideographs, most
  emoji), so CJK-heavy rows don't pay for it. The ranges were checked
  against unicode-segmentation for every char they cover. Without that,
  non-ASCII tests ran about 10× slower in debug builds.
- Clusters stay whole however long. A cluster that crosses a 4 KB mark gets
  a checkpoint at its start, flagged as running to the next checkpoint, so
  queries inside a cluster longer than a chunk (a row of `a` and a megabyte
  of combining marks) return without measuring it again.
  `GlyphWidths` caches cluster widths by a hash of the text, so long
  clusters are shaped whole without being kept. A slice with such a cluster
  in view still shapes all of it (GPUI's line cache reuses that between
  frames); cutting it, tried in review round 1, hid the text after it and
  let clicks land inside the cluster.
- Review rounds 1–3: adding the shortfall in index units, capping the
  ratio of indexed to shaped width, and taking the ratio over the first
  widening's extension each left a 4,000 px view uncovered. Any estimate from
  text already shaped can miss narrowing that strengthens past it. So the
  first widening adds twice the shortfall at the slice's ratio, uncapped,
  and the second, if the text still ends short of the view's right edge,
  takes the rest of the row. The third shape then always covers the view or
  reaches the row's end. `slices_cover_the_view_however_narrowing_changes`
  checks round 3's case (500 ASCII chars then 2,000 families, at a half
  and a tenth), that families at three tenths need no third shape, and
  rows whose narrowing changes once, twice or all along them. Hit-testing
  outside the slice compares against the next cluster boundary
  (`RowIndex::next_boundary`) rather than the next char, and a click past
  the text of a slice that runs to the row's end lands on the row's end
  rather than where the index, measuring the row wider than it's drawn,
  puts that x (`clicks_past_a_narrowed_row_land_on_its_end`).
- Printable ASCII is never segmented, whatever the font: each of its chars
  is a cluster of its own. With ASCII all one width it advances in one step,
  as before; otherwise it's walked a byte at a time, as before T41, up to the
  first char that isn't printable ASCII, from where segmenting takes over.
  Segmenting alone read 64 bytes past each checkpoint, then 64 more to find
  the next walk already there: 946,048 bytes to measure the 917,504-byte
  proportional fixture in `ascii_rows_skip_cluster_measuring`, which now
  checks scanned bytes for it as for monospace.
- `GlyphWidths::cluster_width` shapes through a throwaway `WindowTextSystem`
  per cache miss, since the app's `TextSystem` can only shape a single char
  and a window's line cache would keep each cluster until the frame ends.
