# Re-wrap soft-wrapped rows in viewport-first slices

Status: **built** in `fe-display` (T10, Paddock T-24 part 1). The editor still
wraps every pending row at once until T-24b drives slices from the frame loop.
Decided 2026-10-03.

## Context

Turning soft wrap on, or resizing by an em while it's on, re-wrapped every row
in one call on the UI thread (`WrapMap::rebuild`). On a 1M-line file that was
about half a second inside the editor element's prepaint. The source card
offered three options: wrap in the background against a snapshot, wrap around
the viewport first, or an ASCII fast path.

## Decision

Viewport-first slices on the UI thread, plus the ASCII fast path:

- A new width, tab size or font marks every row pending and wraps nothing.
  Pending rows show their previous wrap (unwrapped if wrap was off). A run of
  unwrapped rows is one tree item, so turning wrap on doesn't build 1M items.
- `DisplayMap::wrap_pending(around, budget_bytes, widths)` wraps pending rows
  from `around` onward, then those before it, nearest first, until it has
  wrapped `budget_bytes` of rows (at least one). The caller decides when to
  run a slice. `WRAP_SLICE_BYTES` is 128 KiB.
- Edits re-wrap the rows they touch at the new width and take them out of the
  pending set. A sync to another buffer, or to history the map can't follow,
  marks everything pending.
- The ASCII width table is built once per width/font change instead of on
  every keystroke. An ASCII row without tabs that can't reach the width even
  if every char were the widest skips measuring.

This avoids sending `CharWidth` (GPUI's text system) to another thread and
doesn't depend on T-05's spawner seam. The cost: a full wrap of 1M lines
finishes over about 680 slices, roughly 6 s at 120 fps, instead of in one
background pass. The rows on screen are wrapped in the first slice.

## Measurements

`cargo bench -p fe-display`, 1M lines of code-like text (about 75 MB, most
rows wider than 80 columns), wrap at 80 columns, `Monospace`, release, Apple
M3 Max. Criterion medians.

| Case | Before (d0b84da) | After |
|---|---|---|
| Set width (wrap on) + one slice | 474 ms (whole file) | **0.58 ms** |
| One slice (128 KiB) | — | 0.58 ms |
| Every pending row, in slices | 474 ms (one call) | 394 ms |
| `sync/keystroke`, 1M lines wrapped | 14.2 µs | 13.0 µs |
| `sync/keystroke`, 1M lines unwrapped | 4.0 µs | 3.7 µs |

Wrapping everything in slices is faster than the old single call, mostly
because each row now does two line lookups in the rope instead of three. 128 KiB keeps a
slice near 0.6 ms, well under the 2 ms target, leaving headroom for rows that
GPUI has to measure (non-ASCII) and for slower machines. 256 KiB would take
about 1.2 ms and halve the time to finish.
