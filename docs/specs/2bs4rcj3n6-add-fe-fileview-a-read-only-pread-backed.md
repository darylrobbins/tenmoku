---
task_uid: "01m4f644r9s0x2r92bs4rcj3n6"
task: "T241"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-fileview test read::reads_match_the_file: on a generated 8 MiB file with multi-byte chars and invalid bytes straddling chunk edges, read() of 1000 random ranges equals String::from_utf8_lossy of the same range with both ends snapped back to char starts"
    required: true
    manual: false
  - id: "C2"
    text: "fe-fileview test read::first_rows_without_an_index: rows_from(0, 60) returns the first 60 lines of an LF file and of a CRLF file without terminators, plus the offset after the last line"
    required: true
    manual: false
  - id: "C3"
    text: "fe-fileview test read::cache_stays_bounded: reading every chunk of a 64 MiB file in order and then at random never takes cached_bytes() above 16 MiB; and cargo tree -p fe-fileview -e normal lists no memmap2"
    required: true
    manual: false
  - id: "C4"
    text: "fe-fileview test changed::truncation_and_rewrites_never_panic: over 1000 iterations that truncate, extend and rewrite a 4 MiB file under an open view while reading random ranges, every read returns Ok or ReadError::Changed, and check() reports Changed after each truncation"
    required: true
    manual: false
  - id: "C5"
    text: "PERF.md gains a row from 'cargo run --release -p fe-fileview --example first_screen' on a generated 1 GiB file: open plus the first 60 rows under 100 ms, with RSS growth under 100 MB"
    required: true
    manual: false
---

# Add fe-fileview: a read-only pread-backed view of very large files with a bounded chunk cache, lossy UTF-8 reads and change detection

## Problem

Large files are on the plan's read path. `docs/decisions/mmap-read-only.md` (T6, decided 2026-10-03) says:

- **Build.** Files of 512 MiB or more open as a read-only `pread`-backed view.
- `memmap2` isn't shipped for mutable files, because truncation raises SIGBUS (confirmed: exit 138).
- The view must be "`pread` with owned buffers", lossy UTF-8 per chunk, with a size/mtime/inode check for
  staleness as a UX signal only.

Its "Follow-up card (draft)" was never filed, and no code exists (`rg -i pread crates` finds only fe-project's
search). Today a 4 GB file costs 6.5 s and 5.8 GB of RAM before the first screen (the decision's table).

That draft is xl, because it touches the rope-typed signatures in `fe-display`, `fe-syntax` and `fe-editor`. This
card is its first, self-contained slice: the file-backed source as a new GPUI-free crate. Nothing else depends on
it yet.

## Change

Add the new crate `crates/fe-fileview` (workspace member through `crates/*`). Add its path dependency to the
workspace `Cargo.toml`, appending lines only. It has no GPUI and no `memmap2`, and it depends only on `std`,
`thiserror` and `libc` if needed.

- `FileView::open(path) -> io::Result<FileView>`. It opens the file read-only and records its length, mtime and
  (dev, inode). `FileView` is `Send + Sync`, with the cache behind a `Mutex`.
- Reads use `std::os::unix::fs::FileExt::read_at` (`cfg(unix)`) or `std::os::windows::fs::FileExt::seek_read`
  (`cfg(windows)`) into owned buffers. There is never a mapping.
- Chunk cache: 256 KiB chunks aligned to 256 KiB, LRU, capped at 64 chunks (16 MiB). `cached_bytes()` reports its
  size.
- `read(range: Range<u64>) -> Result<String, ReadError>` returns the bytes in `range`, decoded lossily like
  `String::from_utf8_lossy`. Both ends are first snapped back to a char start: at most 3 bytes back, past UTF-8
  continuation bytes. A multi-byte char that straddles a chunk edge decodes whole.
- `rows_from(offset: u64, count: usize) -> Result<Rows, ReadError>` reads forward from `offset`, which must be a
  line start, and returns up to `count` lines without their terminators, `\n` or `\r\n`, plus the byte offset after
  the last one. This is how the first screen is shown before any line index exists.
- A read that comes back short of what the recorded length promised is `ReadError::Changed`. So is an EOF inside
  the recorded length. Other I/O failures are `ReadError::Io`. Nothing panics, whatever another process does to the
  file.
- `check(&self) -> Freshness::{Unchanged, Changed}` re-stats the path and compares length, mtime and (dev, inode).
  Document it as a UX signal, not a safety mechanism, citing the decision.
- Example `crates/fe-fileview/examples/first_screen.rs`. It generates a 1 GiB file in `$TMPDIR` (or takes a path),
  times `open` plus `rows_from(0, 60)`, prints that time and the RSS growth (`getrusage`), and deletes the generated
  file. Run it in release and append one row to `PERF.md`. The decision's acceptance is 4 GB; 1 GiB is used for disk
  headroom (T158). Don't create larger files.

Not in this card, so the lead can file them next:

- the background sparse line index and point↔offset;
- the read-only text-source trait in fe-text, with fe-display and fe-editor taking it;
- the open-time threshold, the "file changed, reload" banner and "Edit this file" in the workbench.

## Tests

Tests go in the new files `crates/fe-fileview/tests/read.rs` and `crates/fe-fileview/tests/changed.rs`. Files are
generated in `tempfile` directories. Keep each test under a few seconds.
