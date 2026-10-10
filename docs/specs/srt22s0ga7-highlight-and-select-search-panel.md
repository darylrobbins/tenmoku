---
task_uid: "01m4gwhpxhrybsyvsrt22s0ga7"
task: "T289"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::search_panel_followups::encoding::a_match_in_a_bom_file_selects_the_word: searching 'needle' in EF BB BF + 'hello needle\\n' shows the row 'hello needle' (no U+FEFF) highlighting 6..12, and clicking it selects 'needle' at 6..12 in the editor; it fails at base (9..15)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::search_panel_followups::encoding::a_match_in_a_windows_1252_file_selects_the_word: for b'caf\\xE9 na\\xEFve needle\\n' (detection asserted windows-1252), the row shows 'café naïve needle' highlighting 'needle', and clicking it selects 'needle' in the editor; it fails at base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-text has FileEncoding::detect(reader), reading at most the 64 KiB window plus a byte, through the same function Text::from_reader_detecting uses; fe-buffer re-exports FileEncoding; crates/fe-text/tests/encoding.rs and crates/fe-buffer/tests/encoding.rs pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "UTF-16 files and UTF-8 files without a BOM map as before: tests::search_panel and the other tests::search_panel_followups tests pass unchanged, and search_panel_followups.rs changes only by the 'mod encoding;' line. tests::search_panel_cancel passes with every assertion and expected value unchanged; the only change allowed in search_panel_cancel.rs is to the batch() fixture bytes (e.g. ASCII padding past the 64 KiB detection window, as search_panel_followups.rs does) so FileEncoding::detect picks UTF-8 for its files, asserted in the test; that file's tests still run in under 5 s in debug"
    required: true
    manual: false
  - id: "C5"
    text: "No Cargo.toml or Cargo.lock changes; fe-project is unchanged; scripts/check-no-gpui.sh prints ok for fe-text and fe-buffer"
    required: true
    manual: false
---

# Highlight and select search-panel matches in the text the editor loads, in BOM-prefixed and legacy-encoded files

# Highlight and select search-panel matches in the text the editor loads, in BOM-prefixed and legacy-encoded files

## Problem

Review of T250, finding F1 (medium), at `crates/fe-workbench/src/loader.rs:30`, the search-panel
half. The fe-nav half is T287 and T288.

Since T250 the editor opens files with `Buffer::from_reader_detecting`, which drops a BOM and decodes
legacy encodings (`Text::from_reader_detecting` in `crates/fe-text/src/encoding.rs`). The search panel
still maps match offsets as if every file were lossy UTF-8. `decode_offsets`, `decode_line` and
`decode_checked` in `crates/fe-workbench/src/search_panel.rs` each count an invalid sequence as a 3-byte
U+FFFD. `SearchPanel::open` passes the first range to `Pane::open_at` as `Reveal::columns` on row
`line_number - 1`. So:

- **UTF-8 with a BOM.** fe-project's plain search keeps the BOM in line 1, in `LineMatch::line_offset`
  and the preview. From the review: for a BOM-prefixed `hello needle\n`, clicking the match supplies
  columns 9..15, although the editor's `needle` is at 6..12. `may_shift` doesn't even flag that line,
  because its preview has no U+FFFD.
- **windows-1252, Shift_JIS and other legacy encodings.** Each non-ASCII char before a match is a
  3-byte U+FFFD to the panel, but a 1–3 byte char to the editor. The selection lands off by the
  difference, and the preview shows U+FFFD where the editor shows `é`.

UTF-16 files are already right. The search transcodes them, and their ranges are in the UTF-8 of the
decoded line; `decode_offsets` leaves them alone.

## Change

**Detection, reused from fe-text rather than re-implemented:**

- In `crates/fe-text/src/encoding.rs`, factor the detection step of `Text::from_reader_detecting` (BOM,
  then UTF-8 validity of the first 64 KiB, then `chardetng`) into one private function that returns the
  `FileEncoding` and the BOM's length. `from_reader_detecting` keeps using it, so the two can't
  disagree.
- Expose it as `pub fn FileEncoding::detect(reader: impl Read) -> io::Result<FileEncoding>`. It reads at
  most the 64 KiB window plus one byte.
- Re-export `FileEncoding` from `crates/fe-buffer/src/lib.rs` (`pub use fe_text::{…, FileEncoding}`), so
  fe-workbench can name it through its existing fe-buffer dependency. No `Cargo.toml` or `Cargo.lock`
  changes.

**`crates/fe-workbench/src/search_panel.rs`:**

- `may_shift` also flags line 1 when its preview starts with U+FEFF.
- `decode_offsets` detects the file's encoding with `FileEncoding::detect` on the already-opened
  `source`, once per file, and only when some line may shift. Then:
  - **UTF-16:** unchanged; return as today.
  - **UTF-8:** today's lossy mapping. With a BOM, the line at `line_offset` 0 also moves its
    `preview_start` and ranges back by 3, and its preview drops the leading U+FEFF.
  - **Any other encoding:** `decode_line` keeps today's unchanged-file check: the lossy UTF-8 of the
    bytes must start with the search's preview. Then it decodes the line's bytes with the encoding's
    `new_decoder_without_bom_handling()` and maps each offset as `decode_checked` does today: an offset
    inside a char goes to its start, and a range's end rounds up. It also replaces `preview` with the
    decoded text of the same window, so `line_label`'s highlights line up.
- Cancellation stays as it is: `cancelled` is checked before each line and every `CHECK_BYTES` read or
  decoded.
- Don't change fe-project. T288 relies on its previews and line offsets keeping the BOM.

**Tests:**

- New file `crates/fe-workbench/src/tests/search_panel_followups/encoding.rs`, declared by one
  `mod encoding;` line in `crates/fe-workbench/src/tests/search_panel_followups.rs`. That avoids
  `src/tests/mod.rs`, which many ready tasks hold. As a child module it uses that file's helpers
  (`project`, `searching`, `run`, `shown`, `result_center`, `editor`, `line_row`, `find`) through
  `super::`.
- Fixtures:
  - BOM: `EF BB BF` + `hello needle\n`.
  - windows-1252: `caf\xE9 na\xEFve needle\n`, after T250's `loader_encoding_tests.rs` fixture. The test
    first asserts that `Buffer::from_reader_detecting` reports windows-1252.
