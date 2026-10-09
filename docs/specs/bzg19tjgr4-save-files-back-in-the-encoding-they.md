---
task_uid: "01m4fa9yvh9fmpz1bzg19tjgr4"
task: "T249"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-text test encoding_replaced: from_reader_detecting reports replaced=false for UTF-8 with and without a BOM, UTF-16LE with a BOM and a windows-1252 file, and replaced=true for a UTF-8 file with an invalid byte after the first 64 KiB and a UTF-16LE file with an unpaired surrogate; crates/fe-text/tests/encoding.rs passes with only its call sites changed"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test encoding: Buffer::from_reader_detecting then write_to reproduces the input byte for byte for UTF-8, UTF-8 with BOM, UTF-16LE and UTF-16BE with BOM, windows-1252 and Shift_JIS, and encoding().label() is UTF-8, UTF-8 with BOM, UTF-16LE, UTF-16BE, windows-1252, Shift_JIS; Buffer::new, from_reader and from_file_contents label UTF-8 and write exactly what write_to wrote before"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test encoding: write_to of a windows-1252 buffer holding '😀' on line 2, column 3 fails with ErrorKind::InvalidData and a message containing U+1F600, 'line 2, column 3' and windows-1252"
    required: true
    manual: false
  - id: "C4"
    text: "fe-buffer test encoding: write_to of a buffer read with replaced bytes fails with InvalidData, writing nothing, while its text holds a U+FFFD, and succeeds once every U+FFFD is deleted"
    required: true
    manual: false
  - id: "C5"
    text: "fe-editor test save_encoding: EditorModel::save of an edited windows-1252 buffer writes windows-1252 bytes; a save refused for an unmappable char returns Err, leaves the file byte-identical, leaves no .fastedit-save file in the directory, and the model stays dirty"
    required: true
    manual: false
---

# Save files back in the encoding they were read in, refusing a save that would replace unmappable chars or bytes decoded as U+FFFD

## Problem

T239 (adb50ca) added `Text::from_reader_detecting` and `Text::write_encoded` to fe-text
(`crates/fe-text/src/encoding.rs`), but nothing uses them. Its spec left "wiring it into open and save, and
showing the encoding in the status bar" to a follow-up, which the user asked to be filed once T239 landed
(plan approved 2026-10-08). This card is the GPUI-free half: a `Buffer` that remembers the encoding it was
read in and saves back in it. The workbench's loader and status bar follow in a separate card.

Two gaps make that unsafe today:

- `from_reader_detecting` throws away whether decoding replaced anything: the legacy path ignores
  `decode_to_string`'s `_replaced` (`encoding.rs`, the `decode_stream` closure) and the UTF-8 path uses
  `decode_lossy` (`crates/fe-text/src/lib.rs`), which pushes U+FFFD silently. A UTF-8 file with a bad byte
  past the first 64 KiB, or a UTF-16 file with an unpaired surrogate, would be saved back with U+FFFD where
  the original bytes were: silent data loss.
- `Buffer::write_to` (`crates/fe-buffer/src/buffer.rs`) always writes UTF-8 via `Text::write_to`.

`Buffer::from_reader` must keep its exact UTF-8-lossy decoding: fe-nav (`crates/fe-nav/src/extract.rs`,
`crates/fe-nav/src/references.rs`, the `Decoded` shift table) maps raw byte offsets through it.

## Change

**fe-text.**

- `Text::from_reader_detecting(reader) -> io::Result<DecodedFile>`, where
  `pub struct DecodedFile { pub text: Text, pub line_ending: LineEnding, pub encoding: FileEncoding, pub replaced: bool }`,
  declared in `encoding.rs` and re-exported from `lib.rs`. `replaced` is true when any input was decoded to
  U+FFFD because it was malformed in the detected encoding (not when the file legitimately contains U+FFFD).
  Detection, streaming and memory stay exactly as T239 built them. Collect the flag from `decode_lossy`
  (give it a `&mut bool` it sets when it pushes U+FFFD; `Text::from_reader` passes a throwaway) and from
  the `encoding_rs` decoder's `replaced` result.
- Update the existing call sites in `crates/fe-text/tests/encoding.rs` to the struct; don't change what they
  assert.
- `FileEncoding::label(self) -> String` for the status bar: `"UTF-8 with BOM"` for UTF-8 with `bom`, else
  `name()` (`"UTF-8"`, `"UTF-16LE"`, `"windows-1252"`, `"Shift_JIS"`, …).

**fe-buffer** (`crates/fe-buffer/src/buffer.rs` only; don't touch `crates/fe-buffer/src/lib.rs`, which
T240 holds — callers reach `FileEncoding` through `fe_text` or by method).

- `Buffer` gains `encoding: FileEncoding` and `replaced_on_read: bool`. `Buffer::new`, `from_file_contents`
  and `from_reader` set `FileEncoding::UTF8` and `false`; their decoding is unchanged.
- `Buffer::from_reader_detecting(reader) -> io::Result<Self>` builds from `Text::from_reader_detecting`,
  keeping its line ending, encoding and `replaced`.
- `Buffer::encoding(&self) -> FileEncoding`.
- `Buffer::write_to(out) -> io::Result<()>` writes with `Text::write_encoded(self.line_ending,
  self.encoding, out)`. For a UTF-8 buffer without a BOM that is byte-for-byte what it writes today. It
  refuses rather than writing anything lossy, with `io::ErrorKind::InvalidData`:
  - **Bytes replaced on read.** If `replaced_on_read` and the text still contains U+FFFD, fail before writing
    a byte, with a message saying the file had bytes that aren't valid `<label>`, shown as U+FFFD, and that
    saving would replace them. Once the user has deleted every U+FFFD, the save goes through: dropping
    those bytes is then their choice.
  - **Unmappable char.** `EncodeError::Unmappable { ch, offset }` becomes a message naming the char, its
    `U+XXXX`, its 1-based line and column (column in chars, as the status bar counts) and the label, e.g.
    `'😀' (U+1F600) on line 2, column 3 can't be saved in windows-1252`. `EncodeError::Io` passes through.
  Doc comment: partial output may have been written on an unmappable char, so callers write to a temp
  file, as `EditorModel::save` already does.

**fe-editor.** No source change. `EditorModel::save` (`crates/fe-editor/src/lib.rs`) already writes through
`Buffer::write_to` into a temp file that is dropped on error and only calls `did_save` on success, so a
refused save leaves the original file and the dirty flag alone. `EditorModel::open` stays on
`Buffer::from_reader` for now: the app opens files through `fe-workbench`'s loader, which the next card
switches. Only a new integration test goes in this crate.

Large files: T241's fe-fileview is a separate read-only path; nothing here touches it.

## Tests

New files only: `crates/fe-text/tests/encoding_replaced.rs`, `crates/fe-buffer/tests/encoding.rs`,
`crates/fe-editor/tests/save_encoding.rs`. Build fixtures from string literals with
`encoding_rs::Encoding::encode` or byte literals, as T239's tests do; no binary files.

## Implementation notes

- `Buffer::write_to` reports the 1-based column of an unmappable char as chars since the row start
  (`Text::offset_to_char`), so the test puts a two-byte 'ô' before the emoji to tell chars from bytes.
- fe-editor's test builds windows-1252 fixtures by hand (Latin-1 code points), since the crate has no
  `encoding_rs` or `fe-text` dependency and `Cargo.toml` was out of scope.
