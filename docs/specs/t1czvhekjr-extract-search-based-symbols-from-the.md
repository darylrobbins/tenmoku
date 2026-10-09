---
task_uid: "01m4gw6jcqwtxp56t1czvhekjr"
task: "T287"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-nav test extracted_symbols_in_a_bom_file_select_their_names: fe_nav::extract of EF BB BF + '// heading\\nfn target() {}\\n' gives the symbol target with name_range 14..20 and row 1, selecting 'target' in Buffer::from_reader_detecting of the same bytes; it fails at base (17..23)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-nav test extracted_symbols_in_a_windows_1252_file_select_their_names: for a windows-1252 file with \\r\\n endings, the 'café naïve' comment on row 0 and 'fn target() {}' on row 1 (detection asserted to be windows-1252), target's name_range selects 'target' in the detected text and its row is 1; it fails at base"
    required: true
    manual: false
  - id: "C3"
    text: "extract.rs read builds the buffer with Buffer::from_reader_detecting and keeps the NUL sniff and byte limit, so a UTF-16 file is still Skipped::Binary; crates/fe-nav/tests/extract.rs passes, and its only change is skips_what_it_cannot_index's lossy.rs fixture: more than 64 KiB of ASCII filler before the invalid bytes (as T277 did), an assertion that Buffer::from_reader_detecting of it reports UTF-8, and after_lossy's expected name_range and row shifted only by the filler, still selecting after_lossy in the lossily decoded text"
    required: true
    manual: false
  - id: "C4"
    text: "No Cargo.toml or Cargo.lock changes; scripts/check-no-gpui.sh prints 'ok: fe-nav'"
    required: true
    manual: false
---

# Extract search-based symbols from the text the editor loads, in BOM-prefixed and legacy-encoded files

# Extract search-based symbols from the text the editor loads, in BOM-prefixed and legacy-encoded files

## Problem

Split from T286 (review of T144, finding F7; also T277 F1, review of T250). Since T249/T250 the editor
opens files with `Buffer::from_reader_detecting`, which drops a BOM and decodes legacy encodings
(`crates/fe-workbench/src/loader.rs` `read`; `Text::from_reader_detecting` in
`crates/fe-text/src/encoding.rs`). fe-nav's extraction still reads each file with `Buffer::from_reader`
(`crates/fe-nav/src/extract.rs:186`, in `read`). That is lossy UTF-8 and keeps a UTF-8 BOM as U+FEFF.
So every `Symbol::name_range` and `range` in the symbol index is in different text from the editor's:

- in a BOM-prefixed file the ranges are 3 bytes too far;
- in a windows-1252 file each non-ASCII byte before a symbol adds 1 byte (a 3-byte U+FFFD for a 2-byte
  char).

Search-based go-to-definition and workspace symbols both answer from this index, so a jump selects the
wrong text.

## Change

- `crates/fe-nav/src/extract.rs` `read`: build the buffer with
  `Buffer::from_reader_detecting(&mut reader)` in place of `Buffer::from_reader`. Reuse it as is; don't
  add a decoder. The `BINARY_SNIFF` NUL check, the `Counted` byte limit and `Skipped::TooLarge` stay as
  they are, so a UTF-16 file is still `Skipped::Binary`. Update the doc comments that cite
  `Buffer::from_reader` (the `read` comment and the one near line 42).
- fe-nav already depends on fe-buffer, which has `from_reader_detecting`, so neither `Cargo.toml` nor
  `Cargo.lock` changes, and `scripts/check-no-gpui.sh` keeps printing `ok: fe-nav`.
- Tests go in a new integration test file, `crates/fe-nav/tests/extract_encoding.rs`, which calls
  `fe_nav::extract`. Fixtures are raw bytes written to a tempdir:
  - BOM: `EF BB BF` + `// heading\nfn target() {}\n`.
  - windows-1252: the `café naïve` comment that T250's `loader_encoding_tests.rs` uses, on row 0, with
    `\r\n` line endings, then `fn target() {}`. The test first asserts that
    `Buffer::from_reader_detecting` reports windows-1252, so a changed guess fails loudly.
  - Each test asserts that the symbol `target`'s `name_range` selects `target` in
    `Buffer::from_reader_detecting` of the same bytes, and that its `row` is that text's row.

## Rev 2: the lossy fixture in tests/extract.rs

`crates/fe-nav/tests/extract.rs` `skips_what_it_cannot_index` writes `lossy.rs` as
`b"// \xff\xfe\nfn after_lossy() {}\n"`. With detection, that short file is guessed as windows-1252, so
the test no longer exercises lossy UTF-8 and its asserted `name_range` (13..24) moves. Fix the fixture,
not the assertion's intent, the way T277 did for the search panel (594bbcc): put more than the 64 KiB
detection looks at of plain ASCII filler (for example `"// plain ascii filler\n".repeat(...)`) before
the invalid bytes, assert that `Buffer::from_reader_detecting` of the fixture reports UTF-8, and keep
asserting that `after_lossy` is extracted with a `name_range` selecting `after_lossy` in the lossily
decoded text (the expected range and row shift only by the filler). Nothing else in that file changes.
