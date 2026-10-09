---
task_uid: "01m4f643a31y4vq8rrjm5rmegs"
task: "T239"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-text test encoding::boms_select_the_encoding: UTF-8, UTF-16LE and UTF-16BE files with a BOM decode to the same text without the BOM, and FileEncoding reports the encoding with bom = true"
    required: true
    manual: false
  - id: "C2"
    text: "fe-text test encoding::unmarked_files_are_detected: a windows-1252 file containing 'café naïve' and a Shift_JIS file decode correctly and report those encodings; valid UTF-8 with a multi-byte char straddling the 64 KiB boundary reports UTF-8; a file whose first invalid UTF-8 byte comes after 64 KiB decodes as UTF-8 with U+FFFD, as from_reader does"
    required: true
    manual: false
  - id: "C3"
    text: "fe-text test encoding::round_trip_is_byte_identical: for UTF-8 with and without a BOM, UTF-16LE and BE with a BOM, windows-1252, Shift_JIS and a CRLF file, from_reader_detecting then write_encoded with the reported encoding and line ending gives back the original bytes"
    required: true
    manual: false
  - id: "C4"
    text: "fe-text test encoding::unmappable_chars_are_errors: write_encoded of 'a → b' to windows-1252 returns EncodeError::Unmappable naming '→' and its byte offset, and the output never contains a '?' or '\u0026#' substitute"
    required: true
    manual: false
---

# Detect a file's encoding in fe-text (BOM, UTF-8, else a chardetng guess) and encode text back to it without loss

## Problem

The plan's M6 ("Editing polish: multi-cursor, IME, undo, save/encodings") and its reuse map (`encoding_rs`,
`chardetng`: fuzzy, diff, files, encoding) call for encoding support. None exists. Every file is read by
`fe_text::Text::from_reader` (`crates/fe-text/src/lib.rs:110`), which decodes as UTF-8. Invalid bytes become
U+FFFD, so:

- a Windows-1252 or Shift_JIS file opens as replacement characters;
- a UTF-16 file opens as NUL-riddled garbage;
- saving any of these writes the U+FFFDs back as UTF-8, which loses data.

For a browse-first product, opening such files readably is a read-path gap.

The open and save paths live in `fe-editor/src/lib.rs` and `fe-buffer`'s callers. `fe-editor/src/lib.rs` is in
T145's and T147's scope, so this card adds the GPUI-free decode and encode half in fe-text only. Wiring it into
open and save, and showing the encoding in the status bar, is a follow-up.

## Change

Add the new module `crates/fe-text/src/encoding.rs`, declared and re-exported from `crates/fe-text/src/lib.rs`.
Add `encoding_rs` (already in `Cargo.lock` at 0.8.35; keep it there, as the workspace `Cargo.toml` comment says)
and `chardetng` (Apache-2.0 OR MIT) as workspace dependencies, appending lines only, and to
`crates/fe-text/Cargo.toml`.

- `FileEncoding { encoding: &'static encoding_rs::Encoding, bom: bool }`, with `FileEncoding::UTF8` as the default
  and a `name()` for display (`"UTF-8"`, `"UTF-16LE"`, `"windows-1252"`, …).
- `Text::from_reader_detecting(reader) -> io::Result<(Text, LineEnding, FileEncoding)>`. It streams like
  `from_reader`, normalises line endings the same way, and doesn't hold a second full copy of the file. Detection
  runs in this order:
  1. A UTF-8, UTF-16LE or UTF-16BE BOM picks that encoding and is stripped from the text.
  2. Otherwise, if the first 64 KiB are valid UTF-8 (an incomplete sequence at the cut doesn't count against it),
     the file is UTF-8. Invalid bytes later in the file stay lossy U+FFFD, exactly as `from_reader` does today.
  3. Otherwise, `chardetng::EncodingDetector` fed those 64 KiB (TLD `None`, `allow_utf8 = false`) picks the
     encoding, and an `encoding_rs` streaming decoder decodes the whole file with it.
- `Text::write_encoded(&self, line_ending, encoding: FileEncoding, out) -> Result<(), EncodeError>` writes the BOM
  if `bom` is set, then the text with `line_ending`, as `Text::write_to` does. A char the encoding can't represent
  is `EncodeError::Unmappable { ch, offset }`, where `offset` is the char's byte offset in the text. It never
  substitutes `?` or an `&#…;` reference. Callers write to a temporary file, so partial output is acceptable.
  I/O errors are `EncodeError::Io`.
- `Text::from_reader` and `Text::write_to` keep their signatures and behaviour.

Fixtures are generated in the test from string literals with `encoding_rs::Encoding::encode` (or written as byte
literals). No binary files.

## Tests

All tests go in the new file `crates/fe-text/tests/encoding.rs`.

## Implementation notes

- `from_reader` and `from_reader_detecting` share one streaming loop (`Text::decode_stream` in `lib.rs`).
  UTF-8 files, with or without a BOM, go through the same `decode_lossy` as `from_reader`; every other
  encoding goes through an `encoding_rs` decoder without BOM handling, since the BOM is already stripped.
- Detection reads 64 KiB plus one byte, so a sequence cut at 64 KiB is forgiven only when the file goes
  on past it. A file that ends mid-sequence within the window isn't valid UTF-8 and goes to chardetng.
- `encoding_rs` encoders write UTF-16 as UTF-8, as the web does, so `write_encoded` encodes UTF-8 and
  UTF-16 itself and uses `encoding_rs` only for the other encodings. The line ending goes through the
  same encoder as the text, so ISO-2022-JP returns to ASCII before it. `bom` is written only for UTF-8 and
  UTF-16; the other encodings have no BOM and ignore it.
- `fe_text` re-exports `encoding_rs::Encoding`, so callers can name `FileEncoding::encoding`'s type.
- As with UTF-8 today, malformed bytes in a detected legacy or UTF-16 file decode to U+FFFD, so such a
  file doesn't round-trip. Surfacing that on save belongs with the open/save wiring follow-up.
- Some chars encode without an `encoding_rs` error but decode as another char. Shift_JIS and EUC-JP write '¥'
  as `\` and '‾' as `~`, and ISO-2022-JP widens half-width katakana. So before encoding,
  `write_encoded` checks that each char encodes on its own and decodes back to itself, caching the answer
  per char, and treats a char that fails as `Unmappable`. ASCII is skipped for ASCII-compatible encodings.
  If the encoder still reports `Unmappable`, the char and offset come from the input it consumed: for
  SO, SI and ESC, ISO-2022-JP reports U+FFFD.
- Text survives a round trip, but bytes may not when an encoding has two byte forms for one char.
  Shift_JIS `ED 40` (an NEC-selected IBM extension) decodes to U+7E8A, which `encoding_rs` writes as
  `FA 5C`. C3's fixtures avoid such duplicates. Keeping the original bytes would need per-span
  bookkeeping that the save path doesn't have.
