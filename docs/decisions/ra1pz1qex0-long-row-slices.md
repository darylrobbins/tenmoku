# Long rows: shaping only the visible slice

Status: **measured**. Rows over 8 KB with soft wrap off are shaped only
around the view, and are reachable to their last byte. Measured 2026-10-03.
Source card T29 (Paddock T-17, part 2).

## Context

Rows longer than `MAX_SHAPED_ROW_BYTES` were cut off at 8 KB: the rest of a
minified file's line couldn't be seen, clicked or scrolled to. Now
`layout_frame` shapes a slice from the row's `RowIndex`, the view plus
32 em either side, and places carets, selections, clicks and IME bounds
outside the slice with the index. Scrolling spans the index's full width.

## Method

Release build in a private target dir, M3 Max, the window at its default
size, `fastedit --perf` with a temporary extra phase (not committed): after
the scroll phases, press cmd-right (move to line end), wait 30 frames, click
20 px left of the caret (from `bounds_for_range`), wait 30 frames, then
scroll sideways 40 px a frame for 300 frames.

Two files of the same 10 MB of minified JS: `long.js` is one line;
`short.js` breaks it every 100 bytes (104,859 rows). Times are prepaint per
painted frame.

## Results

| Phase                  | 10 MB on one line           | 100-byte lines              |
|------------------------|-----------------------------|-----------------------------|
| First prepaint         | 7.4 ms                      | 5.1 ms                      |
| Line end (cmd-right)   | 1.39 ms (caret at 10485760) | 1.26 ms (caret at 100)      |
| Click by the caret     | 0.07 ms (caret 10485757)    | 0.24 ms (caret 97)          |
| Scroll sideways, p50   | 0.54 ms                     | 0.26 ms                     |
| Scroll sideways, p99   | 1.52 ms                     | 2.23 ms                     |

Both the line end and the click land on the right byte. Moving to the line
end, clicking and scrolling sideways all prepaint within 1 ms of the
short-line file. The first prepaint doesn't: it's 2.3 ms slower, because it
measures the whole row once for its width (the printable-ASCII fast path
scans 10 MB). That's once per buffer version.

## Known limitation: additive metrics

Accepted 2026-10-03 (review finding F1, option A), then narrowed by T41;
T60 bounded the work it allowed.
The row index adds up widths measured out of context, but GPUI shapes the
slice in context. Where shaping changes widths, the two disagree, and the
error grows along the row.

- **What's exact:** monospace ASCII, the minified-file and data-dump case,
  and, since T41, emoji sequences and combining marks. The index walks
  extended grapheme clusters and measures each multi-char cluster as a
  unit (`CharWidth::cluster_width`, which GPUI shapes in the editor font
  and caches), so a family emoji counts as its one 17 px glyph, not 68 px
  of parts. Checkpoints sit on cluster boundaries, and offsets inside a
  cluster clamp to its start. A cluster that crosses a 4 KB mark gets a
  checkpoint at its start too, so it's measured once, not on every query.
  GPUI shapes each cluster whole, caching widths by a hash of the
  cluster's text.
- **Clusters past the size limit:** a cluster longer than
  `MAX_CLUSTER_BYTES` (256) is pathological (thousands of stacked
  combining marks, a ZWJ chain of a quarter of a million emoji), not text
  anyone reads. The index cuts it into pieces of at most 256 bytes at char
  boundaries, measures each as the sum of its chars' widths, and segments
  the text after each cut afresh. `GlyphWidths::cluster_width` also refuses
  to shape anything that long and sums char widths instead. The index
  never segments or copies more than a window plus 260 bytes of lookahead
  at a time. Inside such a cluster, positions come from summed char widths
  while the shaper draws the cluster its own way, so carets, clicks and
  slice edges in and after it can land off, and a slice can start or end
  inside it, drawing a cut-off cluster. That's the accepted rendering
  degradation.
- **The slice cap:** no slice is longer than `MAX_SHAPED_ROW_BYTES`
  (8 KB of buffer text; tabs expand when shaped). `RowIndex::slice_limit`
  is the last unit boundary that far from the slice's start, and the
  slice, each widening and the last-resort shape all stop there. Where a
  slice from view start minus margin would pass the cap (only kilobytes of
  text measured into a few hundred pixels), it starts at most 4 KB before
  the view, so the view gets the rest.
- **The safeguard:** if a shaped slice still ends short of the view's
  right edge plus the slice margin, and the slice cap allows it to grow,
  `layout_frame`
  widens it and shapes it again, at most twice per row per frame. The
  first widening adds twice the shortfall times the ratio of indexed to
  shaped width over the slice. If the slice is drawn at r times the width
  the index measures, that covers the view and margin whenever the text
  it adds is drawn at r/2 or more, so narrowing that's even along the row,
  at any ratio, needs no more. Nothing already shaped bounds how narrow
  the text after it is drawn, though. So if the text still ends short of
  the view's right edge, the second widening takes as much as a slice may
  hold, up to the slice cap or the row's end (as the first does if the
  slice shaped to nothing). Each of the three shapes is at most 8 KB,
  whatever the shaper does.
- **Bounded work over exact coverage:** T41 let that last shape take the
  rest of the row, so the slice's text always covered the view, and let a
  slice snap out to a whole cluster. A review of T41 showed either can
  shape megabytes in one frame: a 2 MB cluster was shaped whole, and a
  shaper drawing ASCII at half its indexed width and families at a tenth
  made the third shape 10 MB. T60 reverses that: bounded work wins over exact
  coverage for pathological rows. With a shaper that draws text so much
  narrower than the index measures it that 8 KB from the slice's start
  ends short of the view's right edge, the rest of the view is left blank
  for that frame. Scrolling right shows it, from a slice starting further
  along. With real fonts that takes strong contextual narrowing or a
  fallback mismatch, or text dense in bytes, narrowed, in a view thousands
  of pixels wide: 8 KB of 25-byte family emoji at 17 px is about 5,600 px,
  so such a row covers a 4,000 px view only if drawn at over about 0.8 of
  its indexed width. Other text keeps T41's coverage.
- **What still drifts:** Arabic and Indic scripts, slightly. Their joined
  and conjunct forms depend on neighbouring clusters, so measuring each
  cluster alone still misses. Carets, clicks and autoscroll outside the
  slice can land a little off far along such a row. The safeguard keeps a
  slice's text over the view unless the slice runs to the row's end or is
  capped. At the row's end, text drawn narrower than indexed ends before
  the index's end of the row, so scrolling can pass the text, and
  autoscroll to the row's end leaves the caret left of where it aimed (off
  screen only for text drawn far narrower than indexed). A click past the
  text lands on the row's end. Past a capped slice's text, a click lands
  where the index puts it.
- **Shared with soft wrap:** T-20's wrap points still sum char widths. There,
  a mismatch only moves a wrap point; it doesn't build up along the row.

## Notes

- Scrolling sideways shapes a new slice every frame, so GPUI's line cache
  misses: about 0.5 ms a frame. If that matters, snapping the slice to a
  grid of the margin would let consecutive frames reuse it.
- An edit makes a new buffer version, which drops the row's index, so the
  next frame measures the row again (a few ms for 10 MB of ASCII, more for
  non-ASCII). Not measured here. The caret movement's whole-row copy
  (`EditorModel::row_at`) is the known follow-up.
- Far along a row, x is an `f64`. Scroll positions stay `f32` (GPUI's
  `Pixels`), so 40M px along they're multiples of 4 px. Positions in the
  frame are taken relative to the scroll position, so they stay exact.
