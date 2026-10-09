---
task_uid: "01m4fksrcpbyenhxpz0fsftasb"
task: "T269"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-fileview test index::rows_match_a_full_scan: on generated 8 MiB LF, CRLF and mixed files with empty rows, multi-byte chars and invalid bytes, with and without a final \\n, once Complete, row_start of every row and row_for_offset of 1000 random offsets equal a naive scan, and progress().rows + 1 equals the naive row count"
    required: true
    manual: false
  - id: "C2"
    text: "fe-fileview test index::answers_while_building: under build_stepped, progress().rows never decreases from step to step, every row_start that is Some equals the naive scan, rows past progress().rows are None, and the state is Building until the last step and Complete after it"
    required: true
    manual: false
  - id: "C3"
    text: "fe-fileview test index::stops_on_truncation_and_drop: truncating a 16 MiB file during a stepped build ends it as IndexState::Changed with no panic, and dropping a LineIndex mid-build stops its thread within 1 s"
    required: true
    manual: false
  - id: "C4"
    text: "fe-fileview test index::checkpoints_are_sparse: a file of 1,000,000 short lines holds at most 1,000,000 / 64 + 1 checkpoints, and a full build leaves the view's cached_bytes() unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "PERF.md gains a row from 'cargo run --release -p fe-fileview --example line_index' on a generated 1 GiB file: the full index completes in under 2 s with peak RSS growth under 50 MB"
    required: true
    manual: false
---

# Add a background sparse line index to fe-fileview: row starts and offset-to-row answered while the scan runs, without loading the file

## Problem

T241 (commit 7c6f926) added `crates/fe-fileview`: a `pread`-backed `FileView` that shows a very large file's first
screen through `FileView::rows_from` without an index. Its spec lists the next slice it left out: "the background
sparse line index and point↔offset". Without it nothing can scroll past the first screen, jump to a row, or map an
offset to a row.

`docs/decisions/mmap-read-only.md` (T6) sets the shape. "Build the line index lazily on a background thread; show the
first screen before it exists." Its measurement shows an eager full scan costs as much as the rope (4 GB: 7.8 s,
4.3 GB resident through a mapping): "A mapped view only wins if the index is built lazily or in the background ...
and the view is allowed to drop pages." The prototype recorded every 64th line start.

## Change

Add `crates/fe-fileview/src/index.rs` with `LineIndex`, exported from `crates/fe-fileview/src/lib.rs`. No new
dependencies: `std` only.

- `LineIndex::build(view: Arc<FileView>) -> LineIndex` starts a named thread (`"line index"`) that scans the file
  from offset 0 to the view's recorded length. `LineIndex` is `Send + Sync`. Its shared state sits behind an `Arc`
  and a `Mutex` or atomics. Dropping the `LineIndex` sets a cancel flag that the thread checks between chunks, so the
  thread exits within one chunk.
- The scan reads `CHUNK_BYTES` at a time into one owned buffer, through a new crate-private uncached read on
  `FileView` in `lib.rs` (`read_at` / `seek_read` like the cache's). It doesn't use the chunk cache, so indexing a
  4 GB file never evicts the chunks the screen is reading. Its memory is one buffer plus the checkpoints.
- Rows follow `fe_text::Text`: the row count is the number of `\n` plus one. `\r\n` is a two-byte line break. A lone
  `\r` is not a break (unlike `Text`, which normalises it; document the difference). Rows are `u32`, and counting
  stops at `u32::MAX` rows with state `TooManyRows`.
- Sparse: the index stores the start offset (`u64`) of every 64th row (`STRIDE = 64`), plus the total row count
  and the offset of the last `\n` found. `row_start(row)` finds the checkpoint at or before `row` and walks forward
  over at most 63 line breaks through `FileView`'s cached reads. `row_for_offset(offset)` binary-searches the
  checkpoints, then walks the same way.
- Queries never wait for the scan. `row_start(row) -> Result<Option<u64>, ReadError>` is `None` for a row whose
  start the scan hasn't reached. `row_for_offset(offset) -> Result<Option<u32>, ReadError>` is `None` for an
  offset after the last `\n` found while the scan is still running.
- `progress() -> Progress { rows: u32, scanned: u64, state: IndexState }`, where `rows` counts the line breaks found
  so far and `state` is `Building | Complete | Changed | TooManyRows | Failed`. A short read or an EOF inside the
  recorded length ends the scan as `Changed`. Other I/O errors end it as `Failed`. Nothing panics. Callers poll
  `progress()`: there's no callback.
- For tests only, a `#[doc(hidden)] pub fn build_stepped(view) -> (LineIndex, Step)` scans one chunk each time
  `Step::next()` is called, so tests can see an index that is still building, deterministically.
- Example `crates/fe-fileview/examples/line_index.rs`. Like `first_screen`, it generates a 1 GiB file in `$TMPDIR`
  (or takes a path). It times `build` until `Complete`, prints the time, the row count and the `getrusage` peak RSS
  growth, then deletes the generated file. Run it in release and add a row to PERF.md's "Large-file view" section.
  Don't create a larger file (T158).

Not in this card: the `TextSource` trait and its file-backed implementation (the next cards), watcher events, and
any UI.

## Tests

In the new file `crates/fe-fileview/tests/index.rs`. Generate files in `tempfile` directories, reusing nothing
from `read.rs`/`changed.rs` beyond copying a small generator if needed. Each test should take under a few seconds.
