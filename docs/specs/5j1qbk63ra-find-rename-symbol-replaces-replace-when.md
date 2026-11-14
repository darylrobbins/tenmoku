---
task_uid: "01m7ac1vqexwansg5j1qbk63ra"
task: "T345"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::find_rename (cargo nextest run -p fe-workbench, the fake language server) pass: a_symbol_match_shows_rename_symbol, a_non_symbol_or_partial_word_match_shows_replace, rename_applies_the_replace_text_through_the_server, an_empty_replace_field_does_nothing, no_server_shows_replace, replace_all_stays_textual"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench lsp_rename find passes (the rename picker and find bar behave as before)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Find: Rename symbol replaces Replace when the match is a whole symbol the server can rename (Mac T1604)

# Find: Rename symbol instead of Replace when the match is a symbol

Spec: `docs/design/design-system.md#editor` (Find and replace: "Replace adds
Replace, Replace all, and Rename symbol instead when the match is a symbol").
The find bar is T1545's; `crates/fe-workbench/src/lsp_rename.rs` renames the
symbol at the caret through the server (`LspManager::prepare_rename`).

## Change

- When the current match is a whole identifier, ask the server
  (`prepare_rename`, cached per buffer version and match) whether renaming at
  the match's start renames exactly the match's range; if so the replace row
  shows **Rename symbol** in place of Replace. Nothing is asked for a
  match that is not a whole word, for regex searches with more than one match
  group, in a read-only buffer or when no server runs.
- Rename symbol applies the replace field's text as the new name through the
  same code as `lsp.rename`, skipping its picker (`lsp_rename.rs`: a function
  that takes the name), with the same preview rule for edits beyond the file.
  An empty replace field does nothing.
- Replace all stays textual.
- `crates/fe-workbench/src/find_bar.rs`, `lsp_rename.rs`.

## Not in this card

Renaming from the search panel, previewing renames inside the find bar.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-find-…` in
`crates/fe-workbench/l10n/en-US/find.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**
