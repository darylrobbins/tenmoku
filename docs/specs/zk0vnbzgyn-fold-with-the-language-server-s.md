---
task_uid: "01m5n9hz8pbkw7gbzk0vnbzgyn"
task: "T212"
rev: 2
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "LspManager::folding_ranges decodes foldingRange replies to rows (Ok(None) without foldingRangeProvider, empty for null or timeout, single-row ranges dropped) and the client declares lineFoldingOnly"
    required: true
    manual: false
  - id: "C2"
    text: "EditorView::set_fold_ranges makes fold, unfold, fold all, the gutter chevrons and the chevron click use the server's ranges alone, sanitised as VS Code does (one per start row, improper overlaps dropped), following edits until the next answer; None restores tree-sitter folds"
    required: true
    manual: false
  - id: "C3"
    text: "The workbench asks on open and after edits settle and sets or clears the ranges per editor; stale answers dropped. Grammarless languages (language-server attach without a grammar, review F2) are out of scope: T221 carries them"
    required: true
    manual: false
  - id: "C4"
    text: "New tests in folding_range_tests.rs, server_folds_tests.rs and lsp_folds_tests.rs pass headless; existing fold tests pass unchanged"
    required: true
    manual: false
---

# Fold with the language server's foldingRange ranges in place of the tree-sitter fold model where a server provides them, as VS Code does (Mac T1109)

## Problem

Folding comes only from tree-sitter node kinds (`fe_syntax::fold_ranges` / `fold_range_at`, crates/fe-syntax/src/fold.rs), reached through `EditorHost::fold_ranges`/`fold_range_at` (ViewHost in crates/fe-gpui/src/commands.rs:314), the gutter chevrons (`fold_headers`, crates/fe-gpui/src/element.rs:2831) and the chevron click (crates/fe-gpui/src/view.rs:1248). No `textDocument/foldingRange` (`rg foldingRange crates` is empty). VS Code (`editor.foldingStrategy` "auto") folds with the language's folding range provider, here the language server, and falls back to indentation/syntax only where there is none. Languages without a grammar can't fold at all.

## Change

1. fe-lsp: new `crates/fe-lsp/src/folding_range.rs` (+ `pub mod folding_range` and `pub use FoldingRange` in lib.rs): `LspManager::folding_ranges(buffer, version) -> Future<Result<Option<Vec<FoldingRange>>, LspError>>` as document_highlight does (`Ok(None)` without `foldingRangeProvider`, `Some(vec![])` for null/timeout). `FoldingRange { start_row: u32, end_row: u32, kind: Option<FoldKind> }` with `FoldKind::{Comment, Imports, Region}`; rows are LSP lines (rows need no encoding), ranges with end <= start dropped. Client capability in server.rs (`folding_range: { dynamic_registration, line_folding_only: true, range_limit: 5000 }`), and `textDocument/foldingRange` joins the hover timeout group in requests.rs.
2. fe-gpui: new `crates/fe-gpui/src/server_folds.rs`: `EditorView::set_fold_ranges(Option<Vec<RangeInclusive<u32>>>)` (a host's folding ranges, `header..=last hidden row`, the shape `fe_syntax::fold_ranges` returns: LSP `startLine..=endLine`). When set, `ViewHost::fold_range_at` / `fold_ranges`, `fold_headers` and the chevron click use them alone instead of the syntax's (VS Code: a provider's ranges replace the fallback), sanitised as VS Code does: sorted by start, one range per start row (the widest), a range that starts inside another but ends after it dropped, fewer than 2 rows dropped. The rows follow edits until the next set, as inlay hints do (crates/fe-gpui/src/inlay_hints.rs `synced`). `None` restores the tree-sitter model.
3. fe-workbench: new `lsp_folds.rs` (mod in lsp.rs, `sync_folds` beside the other syncs): per open editor, ask on open and 400 ms after an edit settles; a `Some(non-empty)` answer calls `set_fold_ranges(Some(..))`; `None`, empty or an error calls `set_fold_ranges(None)`. Answers for old versions are dropped.
4. No new setting; `editor.fold`, `unfold`, `fold_all`, `unfold_all`, vim `zc/zo/za/zM/zR` and the chevrons all go through the host so need no change. Region/comment `kind`s are decoded but unused (no "Fold All Block Comments" here).

## Tests (all new files)

- `crates/fe-lsp/src/folding_range_tests.rs` (#[path] mod at the end of folding_range.rs): `ranges_decode_to_rows`, `no_provider_sends_nothing`, `null_reply_and_timeout_are_empty`, `inverted_and_single_row_ranges_are_dropped`, `kinds_decode`.
- `crates/fe-gpui/src/server_folds_tests.rs` (#[path] mod in server_folds.rs): `server_ranges_replace_the_syntax_candidates`, `overlapping_ranges_are_sanitised`, `fold_all_and_fold_at_use_the_server_ranges`, `ranges_follow_edits_above_them`, `none_restores_the_syntax`, `a_language_without_a_grammar_folds`.
- `crates/fe-workbench/src/lsp_folds_tests.rs` (#[path] mod in lsp_folds.rs, fake server): `fold_command_folds_the_servers_range`, `no_provider_keeps_tree_sitter_folds`, `edit_asks_again`.
Linux-OK (headless; no goldens). Expected to be near 1.5k lines; if it runs over, land the fe-lsp half (item 1 and its tests) first.
## Revision 2 (lead decision on review F2, high): narrow C3, split the grammarless case

A language server is attached by fe-syntax grammar name (`Document.language`, `LspManager::route_for`, `sync_lsp`), so a language with no grammar never reaches any server, for any LSP feature. Making attachment grammar-independent is cross-cutting and out of scope here; it is a separate card (grammar-independent LSP attachment). C3 no longer requires a language without a grammar to fold; every language that already attaches a server (those with a grammar) does. Scope widened to the files the exclusive-source fix (F1) needed: fe-editor commands.rs/lib.rs, fe-vim folds.rs/headless.rs/tests/folds.rs, fe-gpui vim.rs/inlay_hints.rs.

## Implementation notes

- With a host's ranges set, they are the only folds: `EditorHost::folds_exclusive` stops `editor.fold`, `editor.fold_all` and vim's `zc/zC/za/zA/zM/[z ]z` from falling back to indented blocks (`EditorModel::fold_from`, `fold_all_exact`). A selection spanning rows still folds those rows.
- Ranges and the request are retired when the document's server opening changes (a restart, a reopen), and asked afresh.
- Grammarless languages (a documented limit, per the lead's decision on review F2; C3 is narrowed and the rest split into follow-up T221): `EditorView::set_fold_ranges` folds an editor with no grammar (tested). The workbench, however, attaches a language server by fe-syntax's grammar name (`Document.language`, `ServerSpec.languages`, `LspManager::route_for`), and nothing maps a path to a language without one, so such a language never reaches a server and has no server folds yet. Grammar-independent attachment is T221.
- Widened beyond the reserved scope, with the lead's agreement: `crates/fe-editor` (`EditorHost::folds_exclusive`, `EditorModel::fold_from`, `fold_all_exact`), `crates/fe-vim/src/folds.rs` and `headless.rs`, and `crates/fe-gpui/src/vim.rs` (the `server_folds` field of its three `ViewHost` constructions).
