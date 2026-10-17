# Folds' wrap rows kept across edits (T181, follow-up to T159 / Mac T813)

Status: **built; meets the budgets on helium.** A keystroke with many folds
and soft wrap no longer reads every fold's rows from the wrap tree:
`sync/keystroke, 1M lines wrapped, 10k folds` is 23.2 µs (budget 0.5 ms)
and `200k folds` 182 µs (budget 5.2 ms; slowest sample 404 µs against the
8 ms target). `fold/one more fold among 200k, wrapped`, which still goes
through `FoldMap::rebuild` as the card requires, is level with base
compared like-for-like (3.96 against 4.09 ms; budget: at most 10% slower),
as card rev 2 has it. In the full run it reads 2x slower, because the
bench's earlier cases, now 30-54x faster, fit 20-70x more keystrokes into
their measurement time, and
each keystroke leaves the wrap tree a little more fragmented and scattered
in memory before the `fold/` case runs. Details below. Measured
2026-10-17.

## What changed

- `WrapMap::splice` (`crates/fe-display/src/wrap_map.rs`) returns a
  `WrapEdit`: the buffer rows it replaced, and the wrap rows they had and
  have (`Splice { start, old_rows, new_rows }` each). Unwrapped, the wrap
  rows are the buffer rows. Wrapped, the start and old count are two seeks
  on the tree before `replace_rows`, the new count the sum of the new
  items' rows. The spec's `WrapEdit { start, old_rows, new_rows }` is its
  `wrap` half; the buffer rows ride along so the fold map can find the
  folds near an edit without reading the tree.
- `DisplayMap::sync` (`crates/fe-display/src/display_map.rs`) collects the
  `WrapEdit`s and calls `FoldMap::sync` instead of rebuilding the folds.
  Everything else that rebuilt still does.
- Each hidden run now also records its buffer rows. `FoldMap::sync`
  (`crates/fe-display/src/fold_map.rs`):
  1. composes the edits (each in the coordinates the ones before it left)
     into disjoint, sorted edits of the old rows, merging any that overlap
     or touch, so a multi-caret keystroke is one sweep.
     In-order edits, as a batch's are, only ever merge with the last few
     composed, and a running row delta stands in for summing those before,
     so this costs the number of edits;
  2. sweeps the old runs and the edits together to plan windows. Runs that
     overlap or touch an edit (in buffer rows), and edits that touch those
     runs in turn, form a window; the folds whose hidden rows start in it
     (two `partition_point`s on `folds`) are merged and resolved with
     `WrapMap::first_rows` as `rebuild` does. Each window records the
     buffer-, wrap- and hidden-row deltas the runs after it move by;
  3. applies the windows in one pass over the runs: in place when no window
     changes the number of runs (the usual keystroke), shifting the runs
     between windows, and not at all where the deltas are 0, as when typing
     doesn't change a row's wrap count; otherwise into a new list, copying
     each run once. Review round 1 caught a first version that spliced each
     window into the list, moving the tail once per window that changed the
     count, and summed the earlier edits for each edit in `compose`;
     `fold_map::tests::a_sync_of_many_edits_is_one_pass_over_the_runs` (50k
     edits each dropping one of 200k runs, checked against `rebuild`) took
     19.7 s in a debug build with that version and 0.16 s with this one.
  4. falls back to `rebuild` if any fold now ends past the last row, so
     that rule stays exactly `rebuild`'s.
- `rebuild` resolves the same runs as T159's, through the same helpers
  (`buffer_runs`, then `first_rows` in one cursor pass), now keeping each
  run's buffer rows too.

Tree reads per sync are proportional to the folds the edits touch; the
rest is `FoldMap::edit`'s `retain_mut`, the shift, and at most one copy of
the runs.

Correctness: `crates/fe-display/tests/fold_rows_across_edits.rs`
(`folds_hidden_rows_after_random_edits_match_a_fresh_map`) runs 200 rounds
of edits on 4,000 rows with 300 folds, wrapped at 8 columns and unwrapped,
and after each sync compares `folds()`, `row_count()`, every `row(r)`,
`hidden_rows(b)` and `row_for_buffer_row(b)` with a fresh map. Breaking the
hidden-row shift, the window's upper bound, or `compose`'s running delta
fails it (tried).
`wrap_map::tests::splices_return_the_wrap_rows_replaced` checks the
`WrapEdit`s against `first_row` before and after.

## How it was measured

Machine: helium (Linux x86_64, AMD Ryzen 9 5900X, 12 cores / 24 threads).
Base `cd378d8` (the card's base, T159 in it); candidate `42ccce6`. Each
built with `cargo bench -p fe-display --no-run` into its own
`CARGO_TARGET_DIR` under `~/bench-T181`, then run alternately for 3 rounds
with `--bench --warm-up-time 1 --measurement-time 3 'sync/|fold/'`, base
first in each round, 2026-10-17 22:20:40–22:24:17 UTC. 1-minute load
averages: 10.84 at the start, 11.04 at the end (11.62, 14.50, 14.97,
19.20, 15.15 between runs), from other lanes.

"Fastest" is the smallest times/iters over every sample of the case in
`new/sample.json`, over all 3 rounds; "median" is the lowest of the 3
criterion medians.

| Case | Base fastest | Candidate fastest | Base / candidate | Budget | Verdict |
|---|---|---|---|---|---|
| `sync/keystroke, 1M lines unwrapped` | 3.9 µs | 3.6 µs | 1.10 | not over 10% slower | meets |
| `sync/keystroke, 1M lines wrapped` | 14.9 µs | 14.6 µs | 1.02 | not over 10% slower | meets |
| `sync/keystroke, 1M lines unwrapped, 10k folds` | 78.6 µs | 11.6 µs | 6.8 | not over 10% slower | meets |
| `sync/keystroke, 1M lines wrapped, 10k folds` | 797 µs | 23.2 µs | 34 | at most 0.5 ms | meets |
| `sync/keystroke, 1M lines wrapped, 200k folds` | 9.81 ms | 182 µs | 54 | at most 5.2 ms | meets |
| `fold/one more fold among 200k, wrapped` | 9.81 ms | 20.1 ms | 0.49 | recorded; gated like-for-like below | 2x slower here, see below |

Medians, base / candidate: 4.4 / 3.7 µs, 16.9 / 14.9 µs, 81.9 / 11.8 µs,
903 / 25.1 µs, 10.0 ms / 185 µs, 10.0 / 22.2 ms.

Per round, candidate `wrapped, 10k folds`: 25.3, 29.7, 23.2 µs (base 862,
1,070, 797 µs). Candidate `wrapped, 200k folds`: 211, 331, 182 µs (base
14.6, 15.2, 9.81 ms); **its slowest sample is 404 µs**, far inside the
8 ms keystroke target. `fold/`: candidate 25.3, 20.1, 21.5 ms; base 13.2,
13.8, 9.81 ms.

An earlier run of the first candidate (`e35ed33`, which spliced each
window into the list), 21:45–21:49 UTC at load 11.3–18.1, gave the same
picture: 26.6 µs and 222 µs (slowest 547 µs) for the two budgets, `fold/`
16.3 ms against base's 13.9 ms in the full run and 4.15 against 3.90 ms
like-for-like.

## `fold/` like-for-like

Card rev 2 (lead decision, 2026-10-17) compares the `fold/` cases with only
`'fold/'` run, so both binaries fold on a freshly wrapped tree: the same
binaries, alternately for 3 rounds, base first, `--bench --warm-up-time 1
--measurement-time 3 'fold/'`, 2026-10-17 22:24:17–22:25:08 UTC. 1-minute
load averages: 11.04 at the start, 10.34 at the end (10.35, 9.91, 9.68,
9.19, 10.60 between runs).

| Case | Base fastest | Candidate fastest | Base / candidate | Budget | Verdict |
|---|---|---|---|---|---|
| `fold/one more fold among 200k, wrapped` | 4.09 ms | 3.96 ms | 1.03 | not over 10% slower | meets |

Medians, base / candidate: 4.17 / 4.25 ms. Per round, base 4.10, 4.09,
9.31 ms; candidate 4.16, 4.23, 3.96 ms. The rebuild resolves the same runs
with the same one cursor pass as T159's; its runs carry their buffer rows
too (32 bytes rather than 24).

## Why the full run's `fold/` reads slower

`fold` still rebuilds, as the card says, and the rebuild reads every
fold's rows from the wrap tree: a pass over most of the tree's leaves at
200k folds (T159's decision file has it as memory-bound). Its cost depends
on the shape the tree is in, and the bench runs it last:

- **Run alone it isn't slower**: the table above, with the wrapped tree
  freshly built and no keystroke cases before it.
- **The bench's earlier cases type far more in the candidate.** Criterion
  runs each case for a fixed time, and the wrapped tree is built afresh
  before the `10k folds, wrapped` case. Measured iterations (warm-up not
  counted) on that tree before `fold/`: base 1,565–5,380 keystrokes per
  round, candidate 84,660–152,465.
- **Keystrokes degrade the tree, the same per keystroke in both.** Each
  `splice` slices the tree and appends the suffix, allocating new nodes
  along the path and splitting a leaf at the seam (`sum_tree` has no
  in-place update by position). In one process, candidate build, 200k
  folds, fastest of 30 `fold` calls: 4.06 ms on a fresh tree, 7.34 ms
  after 5,000 bench-style keystrokes, 15.97 ms after 170,000.

So the candidate's `fold/` is measured on a tree that has taken ~20-70x
more edits than base's; on the same tree it's level. The lead chose
the like-for-like comparison for the gate (card rev 2) and keeps `fold`
on `rebuild`. The fragmentation itself goes to its own card: keep the
wrap tree compact under edits (replace an item in place when the row
count doesn't change, or compact now and then), which would help every
rebuild after a long editing session, not only the bench's.
