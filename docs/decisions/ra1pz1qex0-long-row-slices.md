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

Accepted 2026-10-03 (review finding F1, option A). The row index adds up
each char's width measured alone, but GPUI shapes the slice in context.
Where shaping changes widths, the two disagree, and the error grows along
the row:

- **What drifts:** ZWJ emoji sequences (a family emoji is one 17 px glyph
  shaped, but 68 px summed in Menlo at 13 px), combining marks, and
  Arabic and Indic scripts, whose joined and conjunct forms differ from
  the chars measured alone.
- **What you see:** the slice is drawn at the index's x but comes out
  narrower or wider than the index says. Far along such a row, the text
  can fall short of the view or miss it entirely, leaving it blank, and
  after moving to the row's end, autoscroll can leave the caret off
  screen. Clicks and carets near the slice's ends land a little off.
- **What's exact:** monospace ASCII, the minified-file and data-dump case,
  where shaped and summed widths are the same.
- **Shared with soft wrap:** T-20's wrap points use the same additive
  metrics. There, a mismatch only moves a wrap point; it doesn't build up
  along the row.

A cluster-aware index is the fix, carried by T41. It measures grapheme clusters
as units, shaped and cached, and widens a slice whose shaped width falls
short of the view. It fixes emoji sequences and combining marks; Arabic
and Indic would still drift slightly.

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
