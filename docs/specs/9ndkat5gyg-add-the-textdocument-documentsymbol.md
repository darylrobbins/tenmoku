---
task_uid: "01m5n9hz404jckbz9ndkat5gyg"
task: "T210"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "LspManager::document_symbols(buffer, version) returns Ok(None) and sends nothing without documentSymbolProvider, Ok(Some([])) for a null reply or a timeout, and otherwise a pre-order Vec\u003cDocumentSymbol\u003e with depth and parent, ranges in bytes of the request version (UTF-8 and UTF-16 sessions)"
    required: true
    manual: false
  - id: "C2"
    text: "Both DocumentSymbol[] and SymbolInformation[] replies decode (flat ones nested by range containment, other documents' symbols dropped), symbol kinds map to OutlineKind, the list is capped at MAX_SYMBOLS"
    required: true
    manual: false
  - id: "C3"
    text: "The client declares hierarchicalDocumentSymbolSupport and all symbol kinds; textDocument/documentSymbol uses the hover timeout; navigation.rs is untouched"
    required: true
    manual: false
  - id: "C4"
    text: "New tests in crates/fe-lsp/src/document_symbol_tests.rs pass with cargo nextest run -p fe-lsp and clippy -p fe-lsp --all-targets -D warnings is clean; no Cargo change"
    required: true
    manual: false
---

# Add the textDocument/documentSymbol request to fe-lsp: hierarchical and flat replies decoded to a pre-order symbol list in bytes (Mac T1107)

## Problem

LSP completeness track (user-approved 2026-10-23: "LSP features not yet supported"). fe-lsp has no `textDocument/documentSymbol`: `rg documentSymbol crates` finds nothing. The Outline panel and the breadcrumb symbols come only from tree-sitter (`fe_syntax::outline_job`, crates/fe-workbench/src/outline_panel.rs), so a language with a server but no outline query shows "no outline". VS Code's Outline and breadcrumbs are built from the document symbol provider. This card is the fe-lsp half (the request and its decoding); the next card feeds it to the Outline panel and breadcrumbs.

## Change (copy the shape of `crates/fe-lsp/src/document_highlight.rs`)

1. New `crates/fe-lsp/src/document_symbol.rs`, `pub mod document_symbol` in lib.rs with `pub use document_symbol::DocumentSymbol`:
   - `DocumentSymbol { name: String, detail: String, kind: fe_nav::OutlineKind, range: Range<usize>, selection_range: Range<usize>, depth: u32, parent: Option<u32>, deprecated: bool }`, `#[non_exhaustive]` with a `new`.
   - `LspManager::document_symbols(&self, buffer, version) -> impl Future<Output = Result<Option<Vec<DocumentSymbol>>, LspError>> + Send + 'static`, through `document_request_if_provided::<DocumentSymbolRequest>`: `Ok(None)` and nothing sent if the server lacks `documentSymbolProvider`; `null`/empty reply or a timeout is `Ok(Some(vec![]))`.
   - Both reply shapes: `DocumentSymbol[]` (hierarchical) and `SymbolInformation[]` (flat; items whose `location.uri` is another document are dropped). Output is the flat pre-order list the tree-sitter `Outline` uses: sorted by start, a parent before its children, `depth` 0 at the top and `parent` the index of the nearest earlier item whose `range` contains this one's (for the hierarchical shape, the server's own nesting; for the flat shape, found by containment). Ranges decoded against the version the request was made on with `admitted.offsets` (bytes), `selection_range` clipped into `range`, an item with an inverted range dropped. At most `MAX_SYMBOLS` (a pub const, 20_000, as large as the tree-sitter outline allows) kept, the rest dropped.
   - `pub(crate) fn outline_kind(SymbolKind) -> OutlineKind`: the same mapping as the private one in navigation.rs (copy it; do not touch navigation.rs, which T949 holds; a later chore can dedupe).
2. `crates/fe-lsp/src/server.rs` client capabilities (next to `document_highlight`, :2354): `document_symbol: { dynamic_registration, symbol_kind: all kinds, hierarchical_document_symbol_support: true, tag_support: [Deprecated] }`.
3. `crates/fe-lsp/src/requests.rs` `timeout`: `textDocument/documentSymbol` joins the `lsp.timeouts.hover` group (3 s default, no new setting).

## Tests

New `crates/fe-lsp/src/document_symbol_tests.rs`, declared at the end of document_symbol.rs as `#[cfg(test)] #[path = "document_symbol_tests.rs"] mod tests;`, over the fake server (as document_highlight_tests.rs): `hierarchical_symbols_are_pre_order_with_depth_and_parent` (UTF-8 session), `utf16_ranges_decode_to_bytes` (emoji before a symbol), `flat_symbol_information_nests_by_containment`, `symbols_of_other_documents_are_dropped`, `no_provider_sends_nothing` (Ok(None), no request recorded), `null_reply_and_timeout_are_empty`, `stale_version_is_unavailable`, `kinds_map_to_outline_kinds`, `deprecated_tag_is_kept`, `more_than_the_cap_are_truncated`. Linux-OK (headless, no GPUI).
