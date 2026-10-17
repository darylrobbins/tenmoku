---
task_uid: "01m55jvbzm783w53fgw53sd0zv"
task: "T181"
rev: 2
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display integration test folds_hidden_rows_after_random_edits_match_a_fresh_map (new file crates/fe-display/tests/fold_rows_across_edits.rs; cargo test -p fe-display --test fold_rows_across_edits) passes: after random edits (header typing, newlines, edits inside and between folds, multi-range and multi-batch syncs), wrapped and unwrapped, folds(), row_count(), every row(r), hidden_rows(b) and row_for_buffer_row(b) equal a fresh map of the same snapshot, width and folds"
    required: true
    manual: false
  - id: "C2"
    text: "DisplayMap::sync no longer calls FoldMap::rebuild: WrapMap::splice returns the wrap rows it replaced, and FoldMap::sync re-resolves only the runs that overlap or touch the edits, shifting the rest in one sweep; fold, fold_many, unfold, unfold_all, set_wrap_width, rewrap, wrap_pending, replace_text and a resetting sync still use rebuild"
    required: true
    manual: false
  - id: "C3"
    text: "On helium, base and candidate bench binaries run alternately for 3 rounds (--warm-up-time 1 --measurement-time 3 'sync/|fold/'), fastest sample per case: sync/keystroke, 1M lines wrapped, 10k folds at most 0.5 ms; sync/keystroke, 1M lines wrapped, 200k folds at most 5.2 ms; no other sync/ case more than 10% slower than base. The fold/ cases are compared like-for-like, because in the full run the earlier keystroke cases fit a different number of edits into the wrap tree before fold/ runs (lead decision 2026-10-17): base and candidate run alternately for 3 rounds with only 'fold/', and no fold/ case may be more than 10% slower than base there; the full run's fold/ figures are recorded too. The decision file has the tables with machine, commits, date and load, plus the 200k case's slowest sample against 8 ms"
    required: true
    manual: false
  - id: "C4"
    text: "PERF.md's Display map section has this card's base and candidate rows for the many-fold cases, with machine, commit, date and load"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-display passes with its existing fold, wrap and round-trip tests unchanged, display_map::pending_wrap_tests::fold_rows_tests::batched_fold_rows_match_per_row_lookups included; DisplayMap's public API (folds() -\u003e \u0026[Fold] included) is unchanged"
    required: true
    manual: false
---

# fe-display: a keystroke in a 1M-line wrapped file syncs in at most 0.5 ms with 10k folds and 5.2 ms with 200k, by keeping folds' wrap rows across edits and shifting them by the edit's wrap-row delta (follow-up to T159 / Mac T813)

## Problem

T159 (Mac T813, lane ez74dvmwrv) made `FoldMap::rebuild` (`crates/fe-display/src/fold_map.rs`) resolve
every fold's hidden wrap rows in one forward pass: it merges the folds' buffer-row runs, then
`WrapMap::first_rows` (`crates/fe-display/src/wrap_map.rs`) finds all their bounds with one
`Dimensions<BufferRows, WrapRows>` cursor. `DisplayMap::sync` (`crates/fe-display/src/display_map.rs`) still
ends in `rebuild_folds` on every keystroke, so each keystroke reads the wrap tree once per fold run, even when
the edit is nowhere near a fold.

Measured on helium (Ryzen 9 5900X, 2026-10-17 17:20-17:24 UTC, load 2.6-11), fastest samples, base
`4a3c74c` vs T159's candidate `1b12d1b`, alternately for 3 rounds
(`docs/decisions/ez74dvmwrv-fold-rows-one-cursor-pass.md`):

| Case | Before T159 | T159 | Budget the user set |
|---|---|---|---|
| `sync/keystroke, 1M lines wrapped, 10k folds` | 1.42 ms | 690 us | at most 0.5 ms |
| `sync/keystroke, 1M lines wrapped, 200k folds` | 20.8 ms | 11.0 ms | at least 4x faster than before T159, so at most 5.2 ms; ideally under the 8 ms p99 keystroke-to-paint target |

T159's decision file says why it stops at about 2x: the pass is memory-bound. A wrapped 1M-line tree is about
1M items, a rebuild reads the summaries of every leaf the folds land in, and once those leaves are scattered
(after the bench's earlier keystrokes) the pass waits on memory. The same 200k-fold pass ran at 5.1 ms on a
quiet heap and 11 ms in the bench. The user moved the original budgets to this follow-up, which T159's card
names: keep folds' wrap rows across edits and shift them by the edit's wrap-row delta.

## Change

Against T159's versions of `fold_map.rs`, `wrap_map.rs` and `display_map.rs`:

- `WrapMap::splice` returns the wrap rows it replaced, as `WrapEdit { start, old_rows, new_rows }` in wrap
  rows (`start` in the same coordinates as the `RowEdit` it follows: rows before it are untouched). Unwrapped,
  it is the row edit itself; wrapped, `start` and the old count come from the tree before
  `replace_rows` (two seeks), the new count from the new items' rows.
- `DisplayMap::sync` collects the `WrapEdit`s of its row edits and, instead of `rebuild_folds`, calls a new
  `FoldMap::sync(&mut self, wrap_map: &WrapMap, buffer_rows: u32, edits: &[WrapEdit])` after
  `FoldMap::edit` has dropped and shifted the folds as it does now.
- `FoldMap::sync` builds the new hidden runs in one sweep over the old runs and the edits together, in
  order, whatever the number of edits (a multi-caret keystroke is many row edits in one sync):
  - a run that ends before an edit's old wrap rows start is kept, shifted by the wrap-row delta of the edits
    before it, with `hidden_before` shifted by the hidden-row delta so far;
  - runs that overlap or touch an edit's old wrap rows (adjacent on either side included) are removed, and
    the buffer rows they and the edit cover are re-resolved: the folds whose hidden rows fall in that
    window (found with `partition_point` on `folds`, which is sorted by start) are merged and resolved with
    `WrapMap::first_rows`, as `rebuild` does, and the runs that come out go in their place;
  - no other run is read from the wrap tree.
  So tree reads per keystroke are proportional to the folds the edits touch, times log rows. The remaining
  per-fold work is integer arithmetic over the `Vec`s (`FoldMap::edit`'s `retain_mut`, and copying the
  shifted runs), with no tree reads.
- `FoldMap::rebuild` stays, unchanged from T159, for everything else that rebuilds folds: `fold`,
  `fold_many`, `unfold`, `unfold_all`, `set_wrap_width`, `rewrap`, `wrap_pending`, `replace_text` and a
  `sync` that resets (another buffer or earlier version). `DisplayMap`'s public API, `folds() -> &[Fold]`
  included, doesn't change.
- No behaviour change: after any sync the hidden runs, display rows and `hidden_rows` are those a
  from-scratch `rebuild` gives, including `rebuild`'s rule that folds past the last buffer row go and runs
  clip at the wrap row count.

Correctness test, in a new integration test file `crates/fe-display/tests/fold_rows_across_edits.rs`
(public API only, so it builds against T159's base): `folds_hidden_rows_after_random_edits_match_a_fresh_map`.
A fixed-seed `StdRng`; about 4,000 rows of random short words, wrapped at a narrow width with `Monospace` and
wrapped through (`wrap_pending(0, usize::MAX, ..)`) before folding; a few hundred random folds with
`fold_many`, short and long, nested and overlapping. Then about 200 rounds, each one or more `buffer.edit`
calls followed by one `sync`: typing on a fold header (same row count, so its wrap rows can change), a
newline on a header, edits inside a fold, between folds, at the end of the file, edits that insert and
delete whole rows, an edit with several ranges in one call (several row edits in one sync), and two edits
between syncs. After each sync, compare with a fresh map of the same snapshot, width and folds: `folds()`,
`row_count()`, every `row(r)`, and `hidden_rows(b)` and `row_for_buffer_row(b)` for every buffer row. Run the
same rounds once more unwrapped (`set_wrap_width(None, ..)`). Keep it under about 10 s in a debug build.

Bench: the cases are T159's, in `crates/fe-display/benches/display.rs`; it doesn't change. Measure as T159
did on helium: build the base (this card's base commit, with T159 in it) and candidate bench binaries each
into its own `CARGO_TARGET_DIR` on the home disk (never under /tmp), run them alternately for 3 rounds with
`--bench --warm-up-time 1 --measurement-time 3 'sync/|fold/'`, base first in each round, and compare the
fastest times/iters per case over all of `new/sample.json`'s samples. Record the machine, commits, date and
1-minute load averages.

Budgets:
- `sync/keystroke, 1M lines wrapped, 10k folds` at most 0.5 ms (fastest sample).
- `sync/keystroke, 1M lines wrapped, 200k folds` at most 5.2 ms (fastest sample), a quarter of the 20.8 ms
  T159 measured before its change. Report its slowest sample too, against the 8 ms target; that one isn't
  a gate.
- No other `sync/` or `fold/` case more than 10% slower than base (`fold/one more fold among 200k, wrapped`
  still goes through `rebuild`).

PERF.md: under "Display map (`cargo bench -p fe-display`)", add this card's base and candidate rows for the
many-fold cases next to T159's, with machine, commit, date and load, as that section does. The decision file
gives the table above with this card's numbers and the gap to any budget missed.

Out of scope: `Buffer::trim_history` (T180), and making `FoldMap::edit` or the run shift sublinear in folds,
which would need folds stored relative to each other and `folds()` to stop returning a slice.

## Implementation notes

- `WrapEdit` is `{ buffer: Splice, wrap: Splice }`, each `Splice { start, old_rows, new_rows }`: the
  spec's wrap rows are its `wrap` half. The buffer rows ride along, and each hidden run records its buffer
  rows, so `FoldMap::sync` decides which runs an edit overlaps or touches, and finds the folds in the window
  with `partition_point`, without reading the tree.
- `FoldMap::sync` first composes the edits into disjoint edits of the old rows (merging any that overlap or
  touch; in-order edits cost their number), then sweeps runs and edits together to plan windows, then
  applies them in one pass over the runs: in place when no window changes the number of runs (not moving
  the runs at all where the deltas are 0), otherwise copying each run once into a new list. If a fold ends
  past the last row it falls back to `rebuild`, so that rule is `rebuild`'s.
- The test runs its wrapped and unwrapped rounds on two threads; about 13-18 s in a debug build at load
  15-20. `fold_map::tests::a_sync_of_many_edits_is_one_pass_over_the_runs` covers a batch of 50k edits.
- Measurements and the `fold/` case: `docs/decisions/fgw53sd0zv-fold-rows-across-edits.md`. Both keystroke
  budgets are met (23.2 µs and 182 µs fastest). `fold/one more fold among 200k, wrapped` run with only
  `'fold/'` (rev 2's like-for-like comparison) is level with base (3.96 vs 4.09 ms fastest). In the full run
  it reads 2x slower, because the candidate's earlier cases type 20-70x more on the wrap tree in their fixed
  time.
