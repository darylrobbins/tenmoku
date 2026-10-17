---
task_uid: "01m55bq5bbkbhss9ez74dvmwrv"
task: "T159"
rev: 3
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fe-display/benches/display.rs has the cases sync/keystroke, 1M lines unwrapped, 10k folds; sync/keystroke, 1M lines wrapped, 10k folds; sync/keystroke, 1M lines wrapped, 200k folds; and fold/one more fold among 200k, wrapped; cargo bench -p fe-display -- --test runs every case"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display unit test display_map::pending_wrap_tests::fold_rows_tests::batched_fold_rows_match_per_row_lookups (a new file declared by #[path] from pending_wrap_tests.rs): with 2,000 nested and overlapping folds over a soft-wrapped 20k-row text, after 200 random edits, every fold's hidden rows and every display row equal those computed with one WrapMap::first_row per row"
    required: true
    manual: false
  - id: "C3"
    text: "Fastest samples, base and candidate run alternately for 3 rounds: wrapped 10k folds and wrapped 200k folds each at least 1.8x faster than base; no other fe-display bench case more than 10% slower than base; all recorded in the decision file"
    required: true
    manual: false
  - id: "C4"
    text: "PERF.md's Display map section has the new cases' base and candidate rows with machine, commit, date and load; the decision file names the follow-up (keep folds' wrap rows across edits, shifted by each edit's wrap-row delta) with the measured gap to the original budgets: wrapped 10k folds at most 0.5 ms, wrapped 200k folds at least 4x faster than base"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-display passes with its existing fold, wrap and round-trip tests unchanged"
    required: true
    manual: false
---

# fe-display: a keystroke in a 1M-line file with 10k or 200k folds and soft wrap syncs about twice as fast, by resolving folds' wrap rows in one forward cursor pass (Mac T813)

## Problem

Every `DisplayMap::sync` (each keystroke) ends in `rebuild_folds` -> `FoldMap::rebuild`
(`crates/fe-display/src/fold_map.rs`), which recomputes every fold's hidden wrap rows from scratch: two
`WrapMap::first_row` calls per fold (`crates/fe-display/src/wrap_map.rs`), and with soft wrap on each one is
a fresh sum-tree seek from the root. So a keystroke costs O(folds x log rows) even when the edit is nowhere
near a fold. The display bench (`crates/fe-display/benches/display.rs`) keeps only 20 folds, so this isn't
measured.

Measured on the Mac (M3 Max, release, load ~35, 2026-10-17), a 1M-line file shaped like the display
bench's corpus, 200 keystrokes at spread offsets, `buffer.edit` + `map.sync` per keystroke:

| Folds | Unwrapped p50 / p99 | Wrapped at 80 columns p50 / p99 / max |
|---|---|---|
| 20 | 0.004 / 0.012 ms | 0.029 / 0.064 / 0.066 ms |
| 10,000 | 0.13 / 0.16 ms | **2.9 / 3.7 / 4.0 ms** |
| 200,000 (Fold All on 1M lines) | 2.1 / 5.9 ms | **48 / 246 / 1,017 ms** |

Fold All (`EditorModel::fold_all`, every indented block) on a large file gives fold counts like these. The
budget the bench header states: a keystroke's display work is far inside the 8 ms p99 keystroke-to-paint
target. 10,000 folds wrapped uses about half of it; 200,000 wrapped misses frames by a wide margin.

## Change

- In `FoldMap::rebuild`, resolve the folds' wrap rows in ordered passes with one sum-tree cursor each
  instead of a seek from the root per row: add `WrapMap::first_rows(&self, buffer_rows: impl
  Iterator<Item = usize>) -> impl Iterator<Item = usize>` for ascending rows (one
  `Dimensions<BufferRows, WrapRows>` cursor moved with `seek_forward`; the unwrapped case stays
  arithmetic). Fold starts are already ascending (`folds` is sorted by start). Ends of nested folds aren't:
  resolve them from `(end, index)` pairs sorted by end (`sort_unstable`, nearly sorted in practice) and
  write each back by index.
- Skip `set_hidden_rows`'s `sort_by_key` when the ranges are already sorted by start (they are when no
  fold nests out of order; check in the same pass).
- No behaviour change: the same hidden runs come out.

Bench: in `crates/fe-display/benches/display.rs`, after the existing `sync/keystroke` cases, add
`sync/keystroke, 1M lines unwrapped, 10k folds`, `sync/keystroke, 1M lines wrapped, 10k folds` (a 4-row fold
every 100 rows) and `sync/keystroke, 1M lines wrapped, 200k folds` (one every 5 rows), with
`sample_size(10)` for the 200k case, and `fold/one more fold among 200k, wrapped` (`DisplayMap::fold` on a
row with none). Update the header's budget lines to name them.

Measure base vs candidate as `bench-under-load` does: build both bench binaries (base: the new bench file
over the base sources), run them alternately for 3 rounds (`--warm-up-time 1 --measurement-time 3
'sync/|fold/'`), compare the fastest sample per case from `new/sample.json`.

Budgets (revised 2026-10-17 by the user after attempt 1 measured 1.42 ms -> 690 us and 20.8 -> 11.0 ms on
helium): `wrapped, 10k folds` and `wrapped, 200k folds` each at least 1.8x faster than base (fastest
sample); no other display case more than 10% slower than base. The original budgets (`wrapped, 10k folds`
at most 0.5 ms, `wrapped, 200k folds` at least 4x faster than base) move to the follow-up: keep folds' wrap
rows across edits and shift them by the edit's wrap-row delta. Name it in the decision file, with the
measured gap to those budgets; don't build it here.

PERF.md: under "Display map (`cargo bench -p fe-display`)", add the new cases' base and candidate rows to
its table, with the machine, commit, date and load, as that section does.
