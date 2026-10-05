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

Accepted 2026-10-03 (review finding F1, option A), then narrowed by T41.
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
  checkpoint at its start too, so even one longer than a chunk (thousands
  of stacked combining marks) is measured once, not on every query. GPUI
  shapes every cluster whole, caching widths by a hash of the cluster's
  text. A slice always ends on cluster boundaries, so one with such a
  cluster in view shapes all of it; GPUI's line cache keeps that shaping
  from frame to frame while the slice stays put, but each frame still
  copies the cluster's text.
- **The safeguard:** if a shaped slice still ends short of the view's
  right edge plus the slice margin, and the row goes on, `layout_frame`
  widens it and shapes it again, at most twice per row per frame. The
  first widening adds twice the shortfall times the ratio of indexed to
  shaped width over the slice. If the slice is drawn at r times the width
  the index measures, that covers the view and margin whenever the text
  it adds is drawn at r/2 or more, so narrowing that's even along the row,
  at any ratio, needs no more. Nothing already shaped bounds how narrow
  the text after it is drawn, though. So if the text still ends short of
  the view's right edge, the second widening takes the rest of the row (as
  the first does if the slice shaped to nothing), and the slice's text
  covers the view or reaches the row's end, whatever the shaper does. That
  third shape costs as much as shaping the rest of the row; only text
  past the slice drawn at under r/2 needs it.
- **What still drifts:** Arabic and Indic scripts, slightly. Their joined
  and conjunct forms depend on neighbouring clusters, so measuring each
  cluster alone still misses. Carets, clicks and autoscroll outside the
  slice can land a little off far along such a row. The safeguard keeps a
  slice's text over the view unless the slice runs to the row's end.
  There, text drawn narrower than indexed ends before the index's end of
  the row, so scrolling can pass the text, and autoscroll to the row's end
  leaves the caret left of where it aimed (off screen only for text drawn
  far narrower than indexed). A click past the text lands on the row's
  end.
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
