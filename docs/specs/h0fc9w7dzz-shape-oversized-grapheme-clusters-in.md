---
task_uid: "01m4sz83y11enhs2h0fc9w7dzz"
task: "T97"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "host_blocks::clusters cuts a grapheme cluster longer than fe_display::MAX_CLUSTER_BYTES into pieces of at most that many bytes at char boundaries, each measured as the sum of its chars' widths"
    required: true
    manual: false
  - id: "C2"
    text: "view::ghost_text_tests::oversized_cluster_tests::oversized_cluster_is_shaped_in_bounded_pieces passes: a ghost row of 'a', 10,000 U+0301 and 'tail!' paints a block row of at most MAX_SHAPED_ROW_BYTES bytes, unscrolled and scrolled to its end"
    required: true
    manual: false
  - id: "C3"
    text: "Existing ghost text and host block tests (view::ghost_text_tests, view::diff_seams_tests) pass unchanged"
    required: true
    manual: false
---

# Shape oversized grapheme clusters in bounded pieces so no ghost text or host block slice exceeds MAX_SHAPED_ROW_BYTES (T87 review F1) (Mac T390 follow-up)

## Problem

T87 (ca70bf1, "Let wide ghost text rows scroll into view ...") replaced ghost text's hard cut of each block row at
`MAX_SHAPED_ROW_BYTES` (the removed `floor_char_boundary` in `crates/fe-gpui/src/element/ghost_text.rs`) with
`host_blocks::slice`, which cuts a long row around the view between grapheme clusters. Review finding T87 F1
(codex, medium, `crates/fe-gpui/src/element/ghost_text.rs:192`):

> For a ghost row containing `a` followed by 10,000 U+0301 combining marks and `tail!`, host_blocks::slice selects
> the entire 20,001-byte first cluster: its byte-limit check runs only after selecting the starting cluster. That
> oversized slice reaches shaper.shape, whereas the previous implementation capped it at 8,192 bytes. Arbitrarily
> large suggestion clusters can therefore cause excessive shaping work on the UI thread.

The cause is in `crates/fe-gpui/src/element/host_blocks.rs`: `clusters` yields whole grapheme clusters however long
they are, and `slice` takes the first cluster ending past the left edge as its start without checking its length, so
one cluster longer than `MAX_SHAPED_ROW_BYTES` is shaped whole. Host block lines (`host_blocks::layout`) go through
the same `slice` and have the same hole. fe-display already bounds this: its row index (`Clusters` in
`crates/fe-display/src/long_row.rs`) cuts a cluster longer than `MAX_CLUSTER_BYTES` (256) at a char boundary that
long and measures the pieces char by char.

## Change

- In `host_blocks::clusters`, yield bounded units as fe-display does: a grapheme cluster longer than
  `fe_display::MAX_CLUSTER_BYTES` is cut into pieces of at most that many bytes at char boundaries
  (`floor_char_boundary(MAX_CLUSTER_BYTES)`, the rest cut the same way), each piece measured as the sum of its
  chars' `CharWidth::char_width`. Clusters within the limit are unchanged (`cluster_width`). Keep the function in
  host_blocks; don't export fe-display's private `Clusters`.
- With units at most `MAX_CLUSTER_BYTES` long, `slice`'s start unit is bounded and its existing
  `cluster.end - from > MAX_SHAPED_ROW_BYTES` check bounds the rest, so every range it returns is at most
  `MAX_SHAPED_ROW_BYTES` bytes and starts and ends on a char boundary. Update the doc comments of `clusters` and
  `slice` to say so. `widest` (host and ghost) keeps measuring through `clusters`.
- No change to `crates/fe-gpui/src/element/ghost_text.rs` is expected; the fix is in the shared slicer.
- Regression test in a new file, `crates/fe-gpui/src/view/ghost_text_tests/oversized_cluster_tests.rs`, declared
  as `mod oversized_cluster_tests;` from `crates/fe-gpui/src/view/ghost_text_tests.rs` (as
  `view/find_tests.rs` declares `find_tests/scroll_tests.rs`). It is a child of the existing test module, so it
  reuses its private helpers (`editor`, `suggest`, `draw`, `texts`, `scroll_right`) through `super::`, and the
  regression overlay (`**/*_tests.rs`) applies both files to the base, where the module then exists. Put the
  `mod` line after the helper functions near the top of `ghost_text_tests.rs`, not at the end or among the `use`
  lines: T88's lane edits that file's imports and appends at its end.
- Test `oversized_cluster_is_shaped_in_bounded_pieces`: suggest `bar(\n{row}\n)` with
  `row = format!("a{}tail!", "\u{301}".repeat(10_000))` (20,006 bytes), draw, and assert the painted block row
  (`texts(..).1[0]`) is at most `MAX_SHAPED_ROW_BYTES` bytes and starts with `a`; then `scroll_right` to the end
  and assert the painted row is still at most `MAX_SHAPED_ROW_BYTES` bytes. At base the first painted row is the
  whole 20,001-byte cluster.
