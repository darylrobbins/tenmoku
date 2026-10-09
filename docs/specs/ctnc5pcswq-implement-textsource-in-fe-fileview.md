---
task_uid: "01m4fksrqbrtrb7kctnc5pcswq"
task: "T271"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-fileview test source::matches_text_on_valid_lf_files: on generated 8 MiB valid UTF-8 LF files (multi-byte chars, empty rows, with and without a final \\n), a FileText of the completed index answers like Text::new of the same contents for every row's row_start, row_len and row_to_string and for 1000 random offsets, points and ranges"
    required: true
    manual: false
  - id: "C2"
    text: "fe-fileview test source::crlf_and_invalid_bytes_keep_file_offsets: on a CRLF file with invalid bytes, row_start and row_len are file byte offsets and lengths without the \\r\\n, each invalid byte reads as one '?', slice_to_string(r).len() == r.len() for 1000 random in-row ranges, and a range across rows reads each break as \\n"
    required: true
    manual: false
  - id: "C3"
    text: "fe-fileview test source::snapshots_while_building: under build_stepped, a FileText taken after each step equals Text over the file's prefix through the last \\n found, and each later snapshot extends the earlier one"
    required: true
    manual: false
  - id: "C4"
    text: "fe-fileview test source::changed_file_never_panics: over 1000 random TextSource calls on a FileText whose 4 MiB file is truncated and rewritten underneath, every call returns, and error() reports Changed after a truncation"
    required: true
    manual: false
  - id: "C5"
    text: "cargo tree -p fe-fileview -e normal lists no gpui and no memmap2"
    required: true
    manual: false
---

# Implement TextSource in fe-fileview: FileText, a snapshot of a FileView and its line index, with file byte offsets and length-preserving lossy decoding

## Problem

The line index (<T-index>) and the `TextSource` trait (<T-source>) exist, but nothing joins them. A file view can't
be handed to fe-display until `fe-fileview` implements `TextSource` over a `FileView` and its `LineIndex`.

Offsets are the hard part. `Text` normalises `\r\n` to `\n` and decodes invalid UTF-8 as U+FFFD (3 bytes). Either
change shifts every later offset, and a source that reads a byte range with `pread` on demand can't know that shift
without decoding everything before the range. So the file-backed source must keep **file byte offsets**, and decode
in a way that preserves length.

## Change

Add `crates/fe-fileview/src/source.rs` with `FileText`, exported from `crates/fe-fileview/src/lib.rs`. Add
`fe-text.workspace = true` to `crates/fe-fileview/Cargo.toml`. fe-text has no GPUI, so the crate stays GPUI-free.

- `FileText::new(view: Arc<FileView>, index: Arc<LineIndex>) -> FileText` takes a **snapshot** of the index's
  progress. It's immutable, `Clone` (Arcs only), and `TextSource`. It reads as the file's prefix through the last
  `\n` the index had found, so its last row is empty while the index is building. Once the index is `Complete`, it
  reads as the whole file. Callers take a new snapshot to see more rows.
- Offsets are file byte offsets. `row_start`/`row_len` come from the `LineIndex`. A row excludes its `\n` and a `\r`
  directly before it.
- Decoding preserves length, so the trait's per-row contract holds. Valid UTF-8 reads as itself. **Each byte of an
  invalid sequence reads as one `?`** (U+003F). A `\r` that isn't before a `\n` reads as itself. Across rows,
  `chunks_in_range` and `slice_to_string` give each line break as `\n`. So copying a CRLF file's rows gives `\n`
  breaks, as the rope does.
- Long rows: `chunks_in_range` reads only the bytes in its range, through `FileView`'s chunk cache, at most one
  chunk at a time, so a row as long as the file is never read whole. `clip_offset`, `is_char_boundary` and
  `char_at` look at no more than the 4 bytes around the offset.
- Errors: when a read returns `ReadError`, `FileText` records the first one (`FileText::error() ->
  Option<ReadErrorKind>`, where `ReadErrorKind` is `Changed | Io`) and answers with empty text for what it couldn't
  read. Every method still returns, and none panics.

## Tests

In the new file `crates/fe-fileview/tests/source.rs`. Build mid-index snapshots with <T-index>'s
`LineIndex::build_stepped`.

## Implementation notes

- `FileText` decodes a window with 3 bytes of context either side of what it returns. UTF-8 is
  self-synchronising, so that classifies every returned byte as a decode from the file's start
  would. `chunks_in_range` yields one owned piece per `CHUNK_BYTES` of the file; a valid char that
  straddles a chunk edge goes whole into the earlier piece. Part of a valid char cut by the range's
  own ends reads as a `?` per byte, so `slice_to_string(r).len() == r.len()` holds for any in-row
  range, char boundaries or not.
- Between a `\r` and its `\n` is not a char boundary: `clip_offset` moves to the `\r` (`Left`) or
  past the `\n` (`Right`). A line break reads as `\n` in a range that holds its `\n` byte, and as
  nothing in one that ends between the two. `char_at` gives `\n` at either byte of a break, `?` at an
  invalid byte, and `None` inside a valid char (where `Text` panics).
- `row_to_string` is overridden to walk the index once per row end rather than three times.
- `ReadError::kind()` and `ReadErrorKind` live in `lib.rs`. A snapshot of an index that ended
  `Changed` or `Failed` starts with that error recorded. Clones share the recorded error.
- The tests run in a debug build, so the generator draws rows of 0 to 80 pieces: about 55K rows per
  8 MiB file rather than 180K, which keeps the every-row check under ~10 s of CPU.
- Decoding context is optional: a window reads the requested bytes, then up to 3 bytes either side.
  Context that can't be read is left out, so the requested text still comes back. A chunk-edge
  extension never runs past the range's end, and `slice_to_string` clamps before it allocates.
