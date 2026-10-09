---
task_uid: "01m4gw6m5hb9r8fz8n1wdy09ns"
task: "T288"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-nav test references_in_a_bom_file_select_the_word: references to target in EF BB BF + '// heading\\nfn target() {}\\nfn main() { target(); }\\n' give two References whose location.range selects 'target' in Buffer::from_reader_detecting of the file's bytes, with location.row 1 and 2; it fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-nav test references_in_a_windows_1252_file_select_the_word: in a windows-1252 file with \\r\\n endings, the 'café naïve' comment on row 0 and 'fn main() { let café = target(); }' on row 2 (detection asserted to be windows-1252), each reference's range selects 'target' in the detected text, on the right row; it fails at base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-nav test references_in_a_utf16_file_select_the_word: in a UTF-16LE file with a BOM holding the BOM fixture's text, each reference is verified and its range selects 'target' in the detected text; it fails at base"
    required: true
    manual: false
  - id: "C4"
    text: "In all three fixtures each Reference's preview_ranges select 'target' in its preview; the crates/fe-nav/tests/references.rs tests pass, with utf16_hits_are_unverified replaced by utf16_hits_are_located (3 hits, each selecting x in the decoded text), and tests::decoded_offsets passes"
    required: true
    manual: false
  - id: "C5"
    text: "references.rs load builds the buffer with Buffer::from_reader_detecting and keeps its cancel checks and limit; Loaded::utf16 and utf16_bom are gone; no Cargo.toml or Cargo.lock changes, and scripts/check-no-gpui.sh prints 'ok: fe-nav'"
    required: true
    manual: false
---

# Place search-based references in the text the editor loads, in BOM-prefixed, legacy-encoded and UTF-16 files

# Place search-based references in the text the editor loads, in BOM-prefixed, legacy-encoded and UTF-16 files

## Problem

Split from T286 (review of T144, finding F7, instances `crates/fe-nav/src/references.rs:740` and `:845`;
also T277 F1, review of T250). Since T249/T250 the editor opens files with `Buffer::from_reader_detecting`,
which drops a BOM and decodes legacy encodings (`crates/fe-workbench/src/loader.rs` `read`;
`Text::from_reader_detecting` in `crates/fe-text/src/encoding.rs`).

fe-nav's references still assume lossy UTF-8:

- `load` (`references.rs:845`) builds `Loaded::buffer` with `Buffer::from_reader`.
- `Decoded::new` (`references.rs:740`) maps the search's file offsets with `utf8_chunks` shifts for
  `\r\n` and invalid sequences.

So a `Reference::location.range` is in different text from the buffer a jump opens:

- **UTF-8 with a BOM:** fe-project's plain search keeps the BOM in line 1 (`LineMatch::line_offset`).
  The range comes out 3 bytes too far.
- **windows-1252 and other legacy encodings:** each non-ASCII byte before a hit maps to a 3-byte U+FFFD
  instead of its 1- or 2-byte char, so the range drifts.
- **UTF-16:** `Loaded::locate` gives up (`utf16`). The hit falls back to the search's raw offsets: a
  file byte offset plus a range in the transcoded line, which are not text offsets at all.

The language-aware verification (`Finder::check`) runs on the same wrong ranges.

## Change

All in `crates/fe-nav/src/references.rs`:

- **`load`:** build the buffer with `Buffer::from_reader_detecting(raw.as_slice())` in place of
  `Buffer::from_reader`. Reuse it as is; don't add a decoder. The cancel checks, the `limit` read and
  the `limit` check stay as they are.
- **`Decoded`:** build it from the raw bytes and the detected encoding (`loaded.buffer.encoding()`).
  It maps a search hit, given as the line's file offset (`LineMatch::line_offset`) and the hit's range in
  the line, to a range in the buffer's text:
  - **UTF-8:** today's `\r\n` and invalid-sequence shifts, with the dropped BOM's 3 bytes subtracted
    when there was one.
  - **Any other encoding:** build a table of line starts, in the file and in the text. Walk the file's
    lines, which end at the encoding's `\n`: byte `0A`, or the 16-bit unit `000A` for UTF-16. Decode
    each line with the encoding's `new_decoder_without_bom_handling()`, applying the same `\r\n`
    normalisation the text has. Then place a hit in its line:
    - UTF-16: the search's in-line ranges are already in the line as UTF-8 (fe-project
      `LineMatch::ranges`), so use them as is.
    - Otherwise: decode the line's bytes before the hit and take that UTF-8 length.
  - Encoding methods are inherent on the `&'static Encoding` that `FileEncoding` holds, so no new
    dependency is needed.
- Remove `Loaded::utf16` and `utf16_bom`. A UTF-16 hit is now located and verified like any other.
- **Previews:** `preview_ranges` stay computed in the preview's own decoding. `LineMatch::preview` is
  lossy UTF-8 of the line as the search read it, BOM included on line 1. They select the word in
  `Reference::preview` as they do today.
- The in-file unit test `tests::decoded_offsets` changes only as far as `Decoded::new`'s new signature
  needs.
- **`crates/fe-nav/tests/references.rs`:** `utf16_hits_are_unverified` asserts the old behaviour. Turn
  it into `utf16_hits_are_located`: the same 3 hits, each with a range that selects `x` in the decoded
  text. Make no other change there.
- Neither `Cargo.toml` nor `Cargo.lock` changes, and `scripts/check-no-gpui.sh` keeps printing
  `ok: fe-nav`.
- **New tests:** a new integration test file, `crates/fe-nav/tests/references_encoding.rs`. It copies
  the few helpers it needs (search, request, collecting batches) from `tests/references.rs`, and asks
  for references to `target` from a request file. Each test asserts that every reference's
  `location.range` selects `target` in `Buffer::from_reader_detecting` of the file's bytes, and that
  `location.row` is that text's row. The request is a separate `req.rs` that mentions `target`. Each
  fixture is raw bytes in a tempdir, with the definition on row 1 and a call on row 2:
  - BOM: `EF BB BF` + `// heading\nfn target() {}\nfn main() { target(); }\n`.
  - windows-1252: the `café naïve` comment that T250's `loader_encoding_tests.rs` uses, on row 0, with
    `\r\n` line endings. Row 2 is `fn main() { let café = target(); }`, with a non-ASCII char before
    the hit on its own line. The test first asserts that detection reports windows-1252.
  - UTF-16LE with a BOM: the same text as the BOM fixture.

## Implementation notes

- `Decoded::new` takes the raw bytes and the `&Buffer` they were loaded into (fe-nav can't name `FileEncoding`/`Encoding`, which fe-buffer doesn't re-export and Cargo.toml may not change); a boxed closure holds the encoding's decode.
- The search puts a UTF-16 file's first line at file offset 2 (after the BOM), so the line table keys that line by 2.
- Two tests in `tests/references.rs` beyond `utf16_hits_are_located` had to change: their invalid-UTF-8 fixtures are now detected as legacy encodings. `loaded()` uses `from_reader_detecting`, and `verified_only_when_parsed_as_loaded` uses byte 0x80 (windows-1252, 3-byte `€`) instead of 0xFF so the text still outgrows the parse limit.
