---
task_uid: "01m4qzfc8qpv9ch1phceqqcs6y"
task: "T476"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test signature_help::tests::trigger_retrigger_and_hide: typing ( asks with Character('('), typing , while shown asks as a retrigger and moves the bold parameter, typing ) past the call or escape hides it, and an answer for an older version is dropped"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test signature_help::tests::popover_placement: the popover sits above the caret row, below it on the first visible row, shows n/m for several signatures and stays above an open completion menu; editor.show_signature_help asks with Invoked"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test signature_help::tests::vim_insert_only: with vim on, typing ( in insert mode asks; in normal mode nothing is asked"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-gpui -p fe-workbench and cargo clippy on both --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Show signature help while typing a call's arguments: a popover above the caret with the active parameter in bold, from the language server

## Problem

The user (2026-10-12) asked for LSP signature help: the function's parameters shown while typing its arguments,
the current one highlighted. T471 adds `LspManager::signature_help` and its trigger characters; the editor has no
place to show them.

## Change

- fe-gpui, new module `crates/fe-gpui/src/signature_help.rs` (`mod` and exports in `crates/fe-gpui/src/lib.rs`):
  `pub trait SignatureHelpProvider` with `triggers(&self, buffer) -> Option<(Vec<char>, Vec<char>)>` and
  `signature_help(&self, buffer, version, offset, context) -> SignatureHelpFuture`, its own GPUI-free types
  mirroring fe-lsp's (fe-gpui doesn't depend on fe-lsp), set with `EditorView::set_signature_help_provider`, as
  `HoverProvider` is (`crates/fe-gpui/src/hover.rs` :58).
- `crates/fe-gpui/src/view.rs`: typing one of the triggers asks with `Character`; while it shows, typing a
  retrigger character asks with `Character` and `is_retrigger`, and any other edit or caret move asks with
  `ContentChange` and `is_retrigger`, the shown help as `active`. An answer for an older version or caret is
  dropped. `None` hides it, as do escape, focus loss, the caret leaving the rows it was asked on, and a selection.
  New editor command `editor.show_signature_help` asks with `Invoked`. Vim: only in insert mode.
- `crates/fe-gpui/src/element.rs`: a popover above the caret's row (below it when there's no room), in the hover
  popover's style: the active signature's label in the editor font with the active parameter's range in bold,
  `n/m` when there are several signatures, and the active parameter's documentation, then the signature's, as text,
  at most 10 rows. It doesn't take focus or keys, and sits above an open completion menu rather than over it.
- `crates/fe-workbench/src/lsp.rs`: `LspHover` implements the provider from `signature_help` and
  `signature_help_triggers`, and is set on every editor beside the hover and completion providers (:681).

Not in this card: keys for cycling signatures, and the `editor.show_signature_help` binding (the LSP key card).

## Tests

New file `crates/fe-gpui/src/signature_help_tests.rs`, declared from signature_help.rs with `#[path]`, with a fake
provider, as `view/hover_tests.rs` does for hover.
