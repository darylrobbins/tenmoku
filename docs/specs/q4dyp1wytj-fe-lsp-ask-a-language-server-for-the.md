---
task_uid: "01m55ef6y81vgrjgq4dyp1wytj"
task: "T172"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-lsp unit test rename::tests::document_highlight::highlights_decode_in_bytes_of_the_version passes for UTF-8 and UTF-16 sessions: kinds 1, 2, 3 and none decode to Text, Read, Write, Text over byte ranges of the version, sorted; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test rename::tests::document_highlight::no_provider_sends_nothing passes: without documentHighlightProvider, LspManager::document_highlights answers Ok(None) and the fake records no textDocument/documentHighlight; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp unit test rename::tests::document_highlight::null_reply_and_timeout_are_nothing_to_highlight passes: a null reply is Ok(Some([])); one delayed past the hover timeout is Ok(Some([])) and the fake receives $/cancelRequest for it; it fails at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited (rename_tests.rs only gains the #[path] mod line); cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: ask a language server for the document highlights at an offset, as read, write and text ranges in bytes of the buffer's version (Mac T853)

## Problem

VS Code highlights the other uses of the symbol at the caret with the language server's
`textDocument/documentHighlight` (read and write uses told apart), falling back to the word's textual occurrences
only when no server provides it. fe-lsp sends no `documentHighlight` (gap audit 2026-10-17, user-approved). This is
the headless half; the editor half (the card that depends on this one) paints them.

## Change

New module `crates/fe-lsp/src/document_highlight.rs`, `pub mod document_highlight;` in `crates/fe-lsp/src/lib.rs`
with `pub use document_highlight::{DocumentHighlight, HighlightKind};`, in the style of `hover.rs`:

- `#[non_exhaustive] pub struct DocumentHighlight { pub range: Range<usize>, pub kind: HighlightKind }` and
  `pub enum HighlightKind { Text, Read, Write }` (a highlight with no `kind` is `Text`, as the protocol says).
- `LspManager::document_highlights(&self, buffer, version, offset) -> impl Future<Output = Result<Option<Vec<
  DocumentHighlight>>, LspError>> + Send + 'static + use<>`:
  - the buffer's server not providing `documentHighlightProvider` (checked with `provider_capabilities` from the
    seam card): `Ok(None)` with no request sent: the caller falls back to the word's occurrences;
  - otherwise one `lsp_types::request::DocumentHighlightRequest` through `document_request` with `at.position
    (offset)`; a `null` or empty reply is `Ok(Some(vec![]))` (the server says nothing to highlight: no fallback);
    each range decoded with `at.offsets(..)`, in bytes of `version`, sorted by start, duplicates dropped;
  - a timeout (`lsp.timeouts.hover`, set by the seam card) is `Ok(Some(vec![]))`, as hover's timeout is no hover;
    a buffer that moved on is `Err(LspError::Unavailable(..))` as from `document_request`; dropping the future
    cancels the request.

New unit-test file `crates/fe-lsp/src/document_highlight_tests.rs`, declared by
`#[path = "document_highlight_tests.rs"] mod document_highlight;` at the end of the existing
`crates/fe-lsp/src/rename_tests.rs` (rename's tests are the other caret-position request; Regatta's base overlay
copies only `*_tests.rs`), using the fake server as `rename_tests.rs` does:

- `highlights_decode_in_bytes_of_the_version` (UTF-8 and UTF-16 sessions, a line with a multi-byte char before
  the symbol: a reply of three highlights with kinds 1, 2, 3 and one with none decodes to byte ranges of the
  buffer's version, sorted, with kinds Text, Read, Write, Text; the request carried the caret's position);
- `no_provider_sends_nothing` (capabilities without `documentHighlightProvider`: `Ok(None)`, and the fake
  records no `textDocument/documentHighlight`);
- `null_reply_and_timeout_are_nothing_to_highlight` (a `null` reply is `Ok(Some([]))`; a reply delayed past the
  manager's clock's hover timeout is `Ok(Some([]))` and the fake gets a `$/cancelRequest` for it).
