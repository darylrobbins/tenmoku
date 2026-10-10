---
task_uid: "01m4fksrwz1sbq6nqmzq9ejt4j"
task: "T272"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display test read_only::lays_out_a_file_text: DisplayMap::read_only over a FileText of a generated 8 MiB CRLF file with invalid bytes lays out frames at the top, middle and end whose rows' text equals the FileText's rows at file byte offsets, and offset_at at each caret_bounds returns the same offset"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display test read_only::long_row_is_sliced: in a 32 MiB single-row file read through FileText, frames scrolled to the row's start, middle and end shape at most MAX_SHAPED_ROW_BYTES of it"
    required: true
    manual: false
  - id: "C3"
    text: "fe-display test read_only::replace_text_adds_rows: after replace_text with a later build_stepped snapshot, the new rows lay out and earlier rows lay out unchanged; on a read-only map set_wrap_width(Some(_)) leaves wrap_width() None and fold returns false"
    required: true
    manual: false
  - id: "C4"
    text: "cargo bench -p fe-display --bench display stays within 10% of the base commit on every case, with both runs' numbers in the decision file"
    required: true
    manual: false
  - id: "C5"
    text: "Existing fe-display tests change only at call sites, with no assertion changed"
    required: true
    manual: false
---

# Lay out any TextSource in fe-display: wrap, long-row and layout code read through the trait, and DisplayMap gets a read-only source

## Problem

`fe-display` is rope-typed. `DisplayMap` holds a `BufferSnapshot` and returns `&Text`, and `wrap_map.rs`,
`long_row.rs` (`RowIndex` keeps a `Text` clone) and `layout.rs` (`layout_frame`) take `&Text`. `LongRows` keys its
cache by `BufferId` and `Version`. So a `FileText` (<T-filetext>) can't be laid out.

`docs/decisions/mmap-read-only.md` (T6) says that "wrap_map over 50 M lines must be lazy (viewport only) or disabled
in this mode. Soft wrap off by default is acceptable", and that a file view is never parsed (every language's parse
limit is at most 10 MiB).

## Change

fe-display reads all text through `&dyn fe_text::TextSource` (<T-source>). The buffer path must not change
behaviour.

- `wrap_map.rs`, `long_row.rs`, `layout.rs` and `display_map.rs` take `&dyn TextSource` wherever they take `&Text`
  today. `RowIndex` keeps `Arc<dyn TextSource>` from `TextSource::share()`. `RowIndex::new` still takes a reference,
  so existing call sites compile unchanged. Use `row_start`/`row_len` and never `+ 1` for a line break (the trait's
  contract allows two-byte breaks).
- `DisplayMap` holds a private source enum: a buffer (`BufferSnapshot`, as today) or read-only (`Arc<dyn
  TextSource>` plus a `SourceId`, a process-unique `u64`).
  - `DisplayMap::read_only(text: Arc<dyn TextSource>, tab_size) -> DisplayMap`.
  - `DisplayMap::replace_text(&mut self, text: Arc<dyn TextSource>)` takes a later snapshot of the same read-only
    source with more rows. Rows already shown keep their layout.
  - `text()` returns `&dyn TextSource`. `snapshot()` returns `Option<&BufferSnapshot>`, `None` when read-only.
    Nothing outside fe-display calls either today (`rg 'display\(\)\.(text|snapshot)' crates` finds no hits).
  - On a read-only map, `set_wrap_width(Some(_))` keeps soft wrap off, `fold`/`fold_many` return `false`/do
    nothing, and `sync` with a buffer snapshot panics in debug (it's a caller bug).
- `LongRows::index` keys its cache by a source key (buffer id and version, or `SourceId` and row count), not
  `BufferSnapshot`. `layout_frame` passes the map's key.
- `FrameInput` keeps `selections: &[ResolvedSelection]`. A read-only caller builds them from plain offsets.

Add `fe-fileview` as a **dev-dependency** of `crates/fe-display/Cargo.toml` for the new test file. Don't touch
`block_map.rs`, which is T254's.

## Tests

In the new file `crates/fe-display/tests/read_only.rs`, through the public API (`layout_frame` with a fake
`TextMeasure`, as `src/layout/tests.rs` does). Existing tests may change only at call sites, mechanically.

## Implementation notes

- The source key is a public, opaque `fe_display::SourceKey`, from `DisplayMap::source_key()` or
  `From<&BufferSnapshot>`, and `LongRows::index(key, text, row, tab_size, measure)` takes it with the text. Its
  read-only form is the `SourceId`, the row count and the length, so a later snapshot with the same rows and a
  longer last row (which `replace_text` allows) drops long rows' indexes too. For `FileText` the row count alone would
  do: a snapshot taken while its index builds has an empty last row, which is never indexed.
- `layout.rs`'s `visible_spans` joined two rows' highlight queries across a line break by `last.end + 1`. It now asks
  the text whether the gap is the first row's line break (`row_start`/`row_len`), so a CRLF file still gets one query
  for consecutive rows. On a buffer the only difference is that a soft-wrapped segment cut short at
  `MAX_SHAPED_ROW_BYTES` exactly one byte before the next segment no longer joins it, as the doc comment says.
- Wrapping reads a run of rows through one `chunks_in_range` pass (`RowReader` in `wrap_map.rs`), splitting at `\n`
  and asking `row_len` when the text before a `\n` ends in `\r`; debug builds check each row against `row_len`.
  Reading each row with `row_start` plus `row_len` took three rope lookups per row instead of two and put the wrap
  bench about 18% over base. See `docs/decisions/qmzq9ejt4j-read-only-display.md` for the numbers (C4).
- Existing tests changed only at call sites (C5): `LongRows::index` calls in `long_row.rs`'s
  `long_rows_follow_the_snapshot`, a `use fe_text::Text;` in five test modules that had it through `use super::*`,
  and `layout/tests.rs`'s helper returning the buffer's `Text` instead of cloning `display.text()`.
