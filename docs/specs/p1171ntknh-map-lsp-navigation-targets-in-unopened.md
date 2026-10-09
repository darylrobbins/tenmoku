---
task_uid: "01m4gvqnxhtf6myyp1171ntknh"
task: "T286"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::navigation::encoding::targets_in_a_bom_file_select_the_word: for an unopened file of EF BB BF + '// heading\\nfn target() {}\\n', a definition Location, a references reply and a workspace symbol at row 1, columns 3..9 each give a Location with range 14..20 and row 1, which selects 'target' in Buffer::from_reader_detecting of the file's bytes; it fails at base (17..23)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::navigation::encoding::targets_in_a_windows_1252_file_select_the_word: for an unopened windows-1252 file with the 'café naïve' comment on row 0 and 'fn target() {}' on row 1 (detection asserted to be windows-1252), the same three replies give ranges that select 'target' in the detected text; it fails at base"
    required: true
    manual: false
  - id: "C3"
    text: "load in crates/fe-lsp/src/navigation.rs decodes with Text::from_reader_detecting and keeps its O_NONBLOCK open, regular-file check and max_bytes checks; the existing tests::navigation tests pass, and tests/navigation.rs changes only by the 'mod encoding;' line"
    required: true
    manual: false
  - id: "C4"
    text: "No Cargo.toml or Cargo.lock changes; scripts/check-no-gpui.sh prints 'ok: fe-lsp'"
    required: true
    manual: false
---

# Map LSP navigation targets in unopened files to the text the editor loads

# Map LSP navigation targets in unopened files to the text the editor loads

## Problem

Review of T144 (finding F7, medium, `crates/fe-lsp/src/navigation.rs:722`). Since T249/T250 the editor
loads files with encoding detection: `crates/fe-workbench/src/loader.rs` `read` builds buffers with
`Buffer::from_reader_detecting`, which drops a BOM and decodes legacy encodings (`Text::from_reader_detecting`
in `crates/fe-text/src/encoding.rs`). `LspNavigation` still reads a target that isn't open in an editor
with `load` in `crates/fe-lsp/src/navigation.rs`, which ends in `Text::from_reader` (lossy UTF-8, BOM kept
as U+FEFF). The server's row/column is converted against that text, so the byte range it yields indexes
different text from the buffer the jump then opens.

From the review: for a BOM-prefixed `// heading\nfn target() {}\n`, an LSP target at row 1, columns 3..9
becomes 17..23 and selects `get() ` after opening, instead of `target` at 14..20. In a windows-1252 file
every non-ASCII byte before the target becomes a 3-byte U+FFFD instead of a 2-byte char, so the range
drifts by one byte per such char. This affects definitions, references and workspace symbols alike: all
three decode unopened targets through `Decoder::decode` → `load`.

## Change

- `crates/fe-lsp/src/navigation.rs` `load`: decode with `Text::from_reader_detecting(raw.as_slice())`
  and keep its `.text`, in place of `Text::from_reader`. Reuse it as is; don't add a decoder. Keep
  everything else in `load` unchanged: the `O_NONBLOCK` open, the regular-file check and the
  `max_bytes` checks before and after the read. Fix its doc comment: it now reads the file as the
  editor's loader (`loader::read`) does.
- fe-lsp already depends on fe-text, so neither `Cargo.toml` nor `Cargo.lock` changes, and
  `scripts/check-no-gpui.sh` keeps printing `ok: fe-lsp`.
- Tests go in a new module `crates/fe-lsp/src/tests/navigation/encoding.rs`, declared by one
  `mod encoding;` line in `crates/fe-lsp/src/tests/navigation.rs`. As a child module it can use that
  file's private helpers (`Nav`, `location`, `range`, `link`, `bytes_of`) through `super::`, so
  `navigation.rs` needs no other change. Each fixture is written as raw bytes into the fake's root and
  never opened in a buffer. Each test checks the selected text against
  `Buffer::from_reader_detecting` of the same bytes, which is what the editor opens.
  - BOM: `EF BB BF` + `// heading\nfn target() {}\n`.
  - windows-1252: the `café naïve` comment that T250's `crates/fe-workbench/src/loader_encoding_tests.rs`
    uses (é = `E9`, ï = `EF`) on row 0, and `fn target() {}` on row 1. The test first asserts that
    detection reports windows-1252, so a changed guess fails loudly instead of passing vacuously.

Out of scope: fe-nav's search-based extraction and references, which have their own cards, and the
search panel (T277).
