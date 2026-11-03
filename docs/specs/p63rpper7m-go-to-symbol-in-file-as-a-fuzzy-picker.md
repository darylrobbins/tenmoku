---
task_uid: "01m6enymdhx64zshp63rpper7m"
task: "T1315"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "symbol_picker::tests::lists_the_outline_and_filters_fuzzily: the picker's rows are the active editor's shown outline and typing narrows them by subsequence"
    required: true
    manual: false
  - id: "C2"
    text: "symbol_picker::tests::served_outline_is_listed: rows come from the LSP-served outline when one is set, tree-sitter otherwise"
    required: true
    manual: false
  - id: "C3"
    text: "symbol_picker::tests::pick_selects_name_range_and_escape_restores_caret: selection moves preview the symbol, escape restores caret and scroll, enter selects the name range"
    required: true
    manual: false
  - id: "C4"
    text: "symbol_picker::tests::stale_outline_refuses_the_pick: an outline that changed since listing rebuilds the list and refuses the pick"
    required: true
    manual: false
  - id: "C5"
    text: "quick_open_at_prefix_tests::at_prefix_opens_symbols: an @ query in quick open lists the file's symbols; : line queries still work"
    required: true
    manual: false
  - id: "C6"
    text: "document_symbol_keys: secondary-shift-o resolves to nav.document_symbols, outline.show stays bound on its new key and via g shift-o in vim normal mode"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy -p fe-workbench -p fe-keymap --all-targets is clean"
    required: true
    manual: false
---

# Go to Symbol in File as a fuzzy picker on cmd-shift-O, with quick open's @ prefix, keeping the Outline panel reachable

## Problem

cmd-shift-O (`secondary-shift-o` -> `outline.show`, crates/fe-keymap/keymaps/fastedit.toml:948) opens the Outline panel, where VS Code opens Go to Symbol in File, a fuzzy list over the file's symbols that previews and jumps. `nav.workspace_symbols` (secondary-t, nav_picker.rs, `Workbench::workspace_symbols`) is a picker for the workspace but nothing lists one file. Approved 2026-11-02.

The symbol source already exists: the Outline panel is fed by `fe_syntax::outline_job` (tree-sitter) and, since T211 (Mac T1108, crates/fe-workbench/src/lsp_symbols.rs, `OutlinePanel::set_served`), by the language server's documentSymbol with tree-sitter as fallback. The Mac tasks T1107/T1108 on the board are abandoned only because that work landed from another host as T211; the lsp_symbols.rs and fe-lsp/src/document_symbol.rs code is on main. Do NOT add LSP requests, a new fe-lsp API or a second symbol source: read `OutlinePanel::shown_for(editor)` / `outline()` (outline_panel.rs:278, 524), the one `Outline` the panel and breadcrumbs already show, so the picker agrees with them. toolbar_picker.rs already lists an outline's symbols for a crumb and checks a pick against the outline it was listed from; mirror that staleness rule.

## Change

1. New `crates/fe-workbench/src/symbol_picker.rs`: a `Picker` delegate (picker.rs, as nav_picker.rs and toolbar_picker.rs host theirs) over the active editor's current `Outline`: rows show the kind icon (`outline_panel::symbol_icon`), name, detail and parent chain, fuzzy-matched with `HighlightedLabel`; moving the selection scrolls the editor to the symbol and dismissing with escape restores caret and scroll; enter selects the symbol's name range and focuses the editor. If the outline changed since the list was made, rebuild and refuse the pick (toolbar_picker's rule). No outline: a status row ("No symbols"), not an error.
2. New command `nav.document_symbols` ("Go to Symbol in File…") in workbench_commands.rs beside `nav.workspace_symbols`.
3. Bindings (decision, final): `secondary-shift-o` moves to `nav.document_symbols` in `workbench && !terminal`, as VS Code. Outline stays reachable by the View > Outline menu item, the palette, vim's existing `g shift-o`, and a new default: Outline gets `secondary-alt-o` (free today). Update crates/fe-keymap/menus/fastedit.toml with a Go to Symbol in File item.
4. Quick open (quick_open.rs) gains an `@` prefix: a query starting with `@` in Cmd-P hands over to the symbol picker with the rest as its query, as VS Code's quick open does; `:` line queries are unchanged (split_location, quick_open.rs:405).

## Tests

New files only: crates/fe-workbench/src/symbol_picker_tests.rs (via `#[path]` at the end of symbol_picker.rs), crates/fe-keymap/tests/document_symbol_keys.rs, and a quick_open_at_prefix_tests.rs declared from quick_open.rs. Existing tests that assert `secondary-shift-o` resolves to `outline.show` change in the same commit; find them with `rg "secondary-shift-o|outline.show" crates --glob '*tests*'`.
