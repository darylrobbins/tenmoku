# Folds' wrap rows in one forward cursor pass (T159, Mac T813)

Status: **built; meets the revised budgets on helium.** A keystroke with many
folds and soft wrap costs about half what it did: `wrapped, 10k folds` is
2.05x faster than base and `wrapped, 200k folds` 1.90x, against the revised
budget of at least 1.8x each (card rev 2, set by the user on 2026-10-17 after
attempt 1's measurements). The original budgets, at most 0.5 ms for 10k and
at least 4x for 200k, are not met; they move to the follow-up below.
Measured 2026-10-17.

## What changed

`FoldMap::rebuild` (`crates/fe-display/src/fold_map.rs`) no longer makes two
`WrapMap::first_row` seeks from the root per fold:

1. It merges the buffer rows the folds hide into runs first, in one pass over
   `folds` (sorted by start). Every buffer row has at least one wrap row and
   wrap rows keep the buffer rows' order, so `first_row` is strictly
   increasing: buffer-row runs that overlap or touch are exactly those whose
   wrap rows would, and the same hidden runs come out.
2. The runs' bounds then ascend, so `WrapMap::first_rows`
   (`crates/fe-display/src/wrap_map.rs`) resolves all of them with one
   `Dimensions<BufferRows, WrapRows>` cursor moved by `seek_forward`; the
   unwrapped case stays arithmetic. It doesn't read the item it lands on: a
   wrapped item, or the end, starts at the row sought, so `first + row -
   start` is right for both kinds.
3. The runs go straight into the hidden-run table. Nested folds need no sort
   by end, and folds inside others cost no lookups.

This differs from the spec's sketch (starts in one pass, ends from `(end,
index)` pairs sorted by end, then `set_hidden_rows`). That version, and a
lockstep pair of cursors, were built and measured first (commits `557da3f`,
`9b7c521`); merging the runs first was faster and simpler. `set_hidden_rows`
keeps its sort and is now only the proptest's way in.

The bench (`crates/fe-display/benches/display.rs`) gained the four cases the
card names. Keystrokes in the many-fold cases go on a header or between
folds, so they don't drop folds as they go. The buffer's undo history limit
is lifted for the whole bench: see "Undo history trim" below.

## How it was measured

Machine: helium (Linux x86_64, AMD Ryzen 9 5900X, 12 cores / 24 threads).
Base `4a3c74c` with the new bench file over its sources; candidate `1b12d1b`.
Each built with `cargo bench -p fe-display --no-run` into its own
`CARGO_TARGET_DIR`, then run alternately for 3 rounds with
`--bench --warm-up-time 1 --measurement-time 3 'sync/|fold/'`, base first in
each round, 2026-10-17 17:20–17:24 UTC. 1-minute load averages: 2.64 at the
start, 11.20 at the end (4.58, 7.93, 8.03, 7.82, 10.55 between runs). The
quietest window of the day; earlier runs at load 23–36 gave the same picture
with slower numbers.

"Fastest" is the smallest times/iters over every sample of the case in
`new/sample.json`, over all 3 rounds.

| Case | Base fastest | Candidate fastest | Base / candidate | Budget | Verdict |
|---|---|---|---|---|---|
| `sync/keystroke, 1M lines unwrapped` | 3.57 µs | 3.40 µs | 1.05 | not over 10% slower | meets |
| `sync/keystroke, 1M lines wrapped` | 13.9 µs | 14.2 µs | 0.98 | not over 10% slower | meets (2% slower) |
| `sync/keystroke, 1M lines unwrapped, 10k folds` | 72.8 µs | 73.4 µs | 0.99 | not over 10% slower | meets (1% slower) |
| `sync/keystroke, 1M lines wrapped, 10k folds` | 1.42 ms | 690 µs | 2.05 | at least 1.8x faster | meets |
| `sync/keystroke, 1M lines wrapped, 200k folds` | 20.8 ms | 11.0 ms | 1.90 | at least 1.8x faster | meets |
| `fold/one more fold among 200k, wrapped` | 20.9 ms | 12.7 ms | 1.64 | not over 10% slower | meets |

Per round, candidate `wrapped, 10k folds`: 839, 788, 690 µs; base 1.42,
1.51, 1.73 ms. Candidate `wrapped, 200k folds`: 13.1, 11.0, 13.9 ms; base
20.8, 23.1, 23.5 ms. The `wrap/` cases were filtered out, as the procedure
says; they run before any fold exists, so only an empty `rebuild` changed
for them.

## Why it stops at about 2x

Timing the parts of one sync inside a single process (min of 40, same tree,
load ~10–18):

- Lookups alone, 10k folds (20k rows, 100 rows apart): per-row seeks
  1.20 ms, two lockstep cursors 0.57 ms, one merged cursor 0.41 ms.
- 200k folds (400k rows, every few rows): per-row 16.9 ms, one cursor
  2.66 ms.
- Whole rebuild, merged runs: 0.45 ms (10k), 3.5–3.8 ms (200k).
  `folds.edit`, `wrap.splice` and the edit itself add about 0.14 ms at
  200k folds (p50), 25 µs at 10k.

So the pass is about 3x (10k) and 6x (200k) faster than base in isolation.
In the bench it is half that, because the tree is in worse shape there. The
200k case alone, with its tree built on a quiet heap, ran at 5.1 ms (median).
After the earlier cases' few hundred thousand keystrokes, the same code ran
at 11 ms. A wrapped 1M-line tree is about 1M items in leaves of up to 12, and
a rebuild reads the summaries of every leaf the folds land in. Once those
leaves are scattered in memory, the pass waits on memory, and how long
depends on what other lanes are doing to the shared cache (the same
lookups ranged from 0.45 to 5 ms between runs at load 15–35).

Halving the summaries (`u32` counts, so a node's summaries take half the
cache lines) was tried. At load ~30 it showed no clear gain, so it isn't
included.

## Follow-up: keep folds' wrap rows across edits

**Keep folds' wrap rows across edits, shifted by each edit's wrap-row
delta.** A keystroke then re-resolves only the folds it touches, and every
other fold moves by one addition, with no tree reads. That removes the
memory-bound pass above. Not built here; the lead is filing it as its own
card, with the original budgets. The measured gap to them:

| Case | Original budget | Candidate fastest | Gap |
|---|---|---|---|
| `sync/keystroke, 1M lines wrapped, 10k folds` | at most 0.5 ms | 690 µs | 190 µs over, 1.38x the budget |
| `sync/keystroke, 1M lines wrapped, 200k folds` | at least 4x faster than base (base 20.8 ms, so at most 5.2 ms) | 11.0 ms, 1.90x faster | 5.8 ms over, needs another 2.1x |

`wrapped, 200k folds` is also over the 8 ms keystroke-to-paint budget on its
own.

## Undo history trim (found while measuring, not fixed)

The first bench runs showed every keystroke case at about 0.4 ms, even with
20 folds (PERF.md has 3.7 µs). The cause is in `fe-buffer`: once an origin's
undo history passes `DEFAULT_HISTORY_LIMIT` (16 MiB), `Buffer::trim_history`
(`crates/fe-buffer/src/buffer.rs`) walks the whole history on every edit to
collect droppable entries. After ~50k single-character edits, `buffer.edit`
went from 3 µs to 150–275 µs and kept growing. A long typing session reaches
that limit, after which every keystroke pays O(history). The bench now calls
`buffer.set_history_limit(usize::MAX)` so it measures the display map. The
trim itself is being filed as its own `fe-buffer` card: drop from the
front of the history as it grows rather than rescanning it, or keep a
per-origin queue.
