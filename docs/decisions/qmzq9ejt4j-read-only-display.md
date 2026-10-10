# fe-display over any TextSource: read-only maps and the wrap bench

Status: **measured** (same-session ratios, loaded machine). fe-display reads
all text through `&dyn fe_text::TextSource`, and `DisplayMap::read_only` lays
out a `FileText`. The display bench stays within 10% of the base commit on
every case. Measured 2026-10-09. Source card T272.

## What changed

- `wrap_map.rs`, `long_row.rs`, `layout.rs` and `display_map.rs` take
  `&dyn TextSource` where they took `&Text`. `RowIndex` keeps the
  `Arc<dyn TextSource>` from `TextSource::share()`.
- `DisplayMap` holds a buffer snapshot or a read-only source with a
  process-unique `SourceId`. A read-only map never soft wraps or folds, as
  `docs/decisions/mmap-read-only.md` allows, and `replace_text` moves it on
  to a later snapshot of the same source.
- `LongRows::index` takes a `SourceKey` and the text, not a
  `BufferSnapshot`. A read-only key is the `SourceId`, the row count and the
  length. The spec asked for the id and row count; the length also changes
  the key when a later snapshot has the same rows and a longer last row,
  which `replace_text` allows. A `FileText` snapshot taken while its index
  builds has an empty last row, which is never long enough to be indexed,
  so for `FileText` the row count alone would do.
- `visible_spans` joined two rows' highlight queries when the second started
  one byte after the first ended. It now asks the text whether the gap is the
  first row's line break, which is two bytes in a CRLF file. On a buffer this
  is the same test, except that a soft-wrapped segment cut short at
  `MAX_SHAPED_ROW_BYTES` exactly one byte before the next segment no longer
  joins it, as the function's doc ("a row cut short ends the query") says.

## The wrap bench

Reading through the trait made wrapping slower at first. Wrapping a row asked
for `row_start` and `row_len`, and `Text::row_len` looks up two rows itself,
so each row took three rope lookups where the old code took two
(`row_start(row)` and `row_start(row + 1) - 1`). Each `chunks_in_range` call
also boxes its iterator. By the fastest sample, `wrap/one slice` went from
about 590 µs to 700 µs (+18%). Going back to two lookups alone gave 630 µs.

Wrapping now reads a run of rows through one `chunks_in_range` pass from the
first row's start and splits it at `\n` (`RowReader` in `wrap_map.rs`). A run
costs two row lookups and one box however many rows it has. A line break
reads as `\n` from both `Text` and `FileText`; if the text before a `\n`
ends in `\r`, `row_len` decides where the row ends, so a source that reads a
`\r\n` break some other way still wraps correctly. Debug builds check every
row's length against `row_len`. Re-wrapping edited rows reads them as one run
too; only wrapping rows backwards from the viewport reads one row at a time,
through the same reader. A read-only map never wraps, so none of this reads a
`FileText` in practice.

## Method

`cargo bench -p fe-display --bench display`, run as the binary cargo builds
for it, on the base commit (`6eccb8f`, `crates/fe-display` and `Cargo.lock`
checked out from it) and on this branch at `314d4f6` (the code as committed;
later commits change only tests and docs), alternating base and candidate
runs.
Other lanes kept the machine (14 cores) at a load average of 35 to 150,
which moves criterion's means by several times from run to run, so the
comparison is on each case's fastest sample (the per-iteration time of the
quickest of criterion's samples). The means are listed too.

Two sets of runs:

1. The whole bench, three rounds of base then candidate, 20:15–20:24, load
   average 38 rising to 144.
2. The three cases whose ratio round 1 left unclear (`all pending rows`, both
   `sync/keystroke`), eight rounds of base then candidate with
   `--warm-up-time 1 --measurement-time 3`, 20:24–20:31, load average about
   85.

## Results

Fastest sample over all rounds, candidate over base:

| Case | Base | Candidate | Ratio | Rounds |
|---|---|---|---|---|
| wrap/set width + one slice, 1M lines | 605.98 µs | 337.38 µs | 0.557 | 3 |
| wrap/one slice, 1M lines | 596.10 µs | 330.93 µs | 0.555 | 3 |
| wrap/all pending rows in slices, 1M lines at 80 columns | 560.6 ms | 435.4 ms | 0.777 | 8 |
| sync/keystroke, 1M lines unwrapped | 4.031 µs | 4.191 µs | 1.040 | 8 |
| sync/keystroke, 1M lines wrapped | 13.90 µs | 14.51 µs | 1.044 | 8 |

Every case is within 10% of base. Wrapping a slice forwards from the viewport
is about 45% faster than base, since a run of rows no longer looks up each
row. The keystroke cases change by under 5%: an unwrapped keystroke runs no
wrapping code, and a wrapped one re-wraps a single row, so that's within what
the fastest sample still moves at this load.

Criterion's mean estimate per round (round 1 of set 1 at load 38):

| Case | Base, rounds 1–3 | Candidate, rounds 1–3 |
|---|---|---|
| wrap/set width + one slice | 1.980 ms, 2.451 ms, 2.600 ms | 969.79 µs, 6.935 ms, 877.86 µs |
| wrap/one slice | 2.015 ms, 1.702 ms, 965.65 µs | 722.11 µs, 9.137 ms, 2.069 ms |
| wrap/all pending rows | 1.359 s, 1.188 s, 663.10 ms | 820.78 ms, 5.089 s, 3.570 s |
| sync/keystroke, unwrapped | 8.25 µs, 34.38 µs, 6.28 µs | 9.42 µs, 31.10 µs, 10.38 µs |
| sync/keystroke, wrapped | 31.16 µs, 59.91 µs, 38.58 µs | 120.47 µs, 107.81 µs, 34.07 µs |

The median of the eight means in set 2: `all pending rows` 1.089 s base,
667.0 ms candidate; `sync/keystroke` unwrapped 11.50 µs base, 7.351 µs
candidate; wrapped 38.59 µs base, 27.24 µs candidate.

Before the one-pass reader, with each row read by `row_start` and `row_len`,
the fastest samples were 583–604 µs base and 691–705 µs candidate on
`wrap/one slice` (+18%), and reading each row through to the next row's start
(two lookups, still one box per row) gave 632–659 µs (+8% to +12%).
