---
task_uid: "01m4r7w31qe61g9n78hbd0g3ch"
task: "T81"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-lsp test formatting::tests::document_and_range: format with None sends textDocument/formatting and with a range sends rangeFormatting with that range in the server's encoding, both with the given tab size and insertSpaces; the reply's edits come back as sorted byte edits of the version"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test formatting::tests::missing_and_slow: a server without the provider gives Ok(None) with no request; an empty reply gives Some(empty); no reply within 3 s gives Err(Timeout); overlapping edits are an error"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-lsp and cargo clippy -p fe-lsp --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Ask language servers to format a document or a range in fe-lsp, as byte edits of the version formatted (Mac T470)

## Problem

The user (2026-10-12) asked for LSP formatting of a document or a selection, and format on save. T465 declares the
capabilities; nothing sends `textDocument/formatting` or `textDocument/rangeFormatting`.

## Change

In `crates/fe-lsp/src/formatting.rs` (the stub module T465 adds), an `impl LspManager` block:

- `format(buffer, version, range: Option<Range<usize>>, options: FormatOptions) -> impl Future<Output =
  Result<Option<Vec<(Range<usize>, String)>>, LspError>>`: `rangeFormatting` for `Some(range)`, `formatting` for
  `None`. `FormatOptions { tab_size: u32, insert_spaces: bool }` become the request's `FormattingOptions`
  (`trimTrailingWhitespace`, `insertFinalNewline` and `trimFinalNewlines` left unset). The reply's edits are
  decoded against the text the server has for `version`, sorted by start; overlapping edits are an error. `Ok(None)`
  for a server without that provider (so a host can say "no formatter"), `Ok(Some(vec![]))` for nothing to change.
- Timeout 3 s by default; a timeout is `Err(LspError::Timeout)`, not an empty result, so format on save can tell
  "nothing to change" from "gave up".

## Tests

New file `crates/fe-lsp/src/formatting_tests.rs`, declared from formatting.rs with `#[path]`, on the fake server.
