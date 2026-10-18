---
task_uid: "01m55v197510tw1gz8g0cc0nvm"
task: "T172"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-config integration test lsp_editor_settings_round_trip (crates/fe-config/tests/lsp_editor_settings.rs, top level) passes: defaults true, true, true, true and []; a user file setting all five loads with no diagnostics and those values; code_actions_on_save given a string is a diagnostic and leaves the default; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "EditorSettings has inlay_hints, semantic_highlighting, lightbulb, code_lens and code_actions_on_save, each with a doc line; settings.schema.json is regenerated with UPDATE_GOLDEN=1 and its golden test passes; settings-template.toml lists the five commented out at their defaults"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-config passes with no existing test edited beyond what the spec names; cargo clippy -p fe-config --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Add the editor settings the LSP track's features read: editor.inlay_hints, semantic_highlighting, lightbulb, code_lens and code_actions_on_save (Mac T922)

## Problem

The LSP completeness track (user-approved 2026-10-17, from `~/.local/share/fastedit-lead/lsp-gap-audit.md`
§3 items 1-10) adds five editor features that VS Code lets the user turn off or configure: inlay hints,
semantic highlighting, the code-action lightbulb, code lenses and code actions on save. Each would add its
own key to `EditorSettings` (`crates/fe-config/src/settings.rs:178-211`), its schema golden and the template.
Those three files are already held by eight open tasks (T821, T842, T873, T877, T894, T911, T912, T918), so five
more cards on them would queue behind each other and every one of those.

This card adds the five keys once, so the feature cards only read them. Each key's doc line says what it
does; the feature cards named below make it do so. Until then a key is read by nothing.

## Change

1. `crates/fe-config/src/settings.rs` `EditorSettings`, with a doc line each (VS Code's setting in brackets):
   - `inlay_hints: bool`, default **true**: show language servers' inlay hints (type and parameter names)
     inside the text (`editor.inlayHints.enabled`). Wired by the inlay-hints workbench card.
   - `semantic_highlighting: bool`, default **true**: colour code with language servers' semantic tokens over
     the syntax colours (`editor.semanticHighlighting.enabled`). Wired by the semantic-tokens workbench card.
   - `lightbulb: bool`, default **true**: show a lightbulb by the caret's row when a language server has
     quick fixes or refactorings there (`editor.lightbulb.enabled`). Wired by the lightbulb card.
   - `code_lens: bool`, default **true**: show language servers' code lenses ("3 references", "Run") above
     rows (`editor.codeLens`). Wired by the code-lens workbench card.
   - `code_actions_on_save: Vec<String>`, default **empty**: code action kinds to run, in order, when a file
     is saved by the user, before `format_on_save` (`editor.codeActionsOnSave` with "explicit"), such as
     `["source.organizeImports", "source.fixAll"]`. Wired by the code-actions-on-save card.
2. `Default for EditorSettings`: the defaults above.
3. `crates/fe-config/settings-template.toml`: the five keys, commented out at their defaults, in the
   `[editor]` section beside `format_on_save`.
4. `crates/fe-config/settings.schema.json`: regenerated (`UPDATE_GOLDEN=1`, as its test says).
5. Test: new integration test `crates/fe-config/tests/lsp_editor_settings.rs`, built like
   `tests/indent_guides_setting.rs`: `lsp_editor_settings_round_trip`. Defaults are true, true, true, true and
   `[]`; a user file setting all five (the list to `["source.organizeImports"]`) loads through
   `SettingsStore::load` with no diagnostics and those values; and `code_actions_on_save = "source.fixAll"` (a
   string, not a list) gives a non-empty `store.diagnostics()`, leaving the default.

Linux-OK.

## Implementation notes

`crates/fe-config/tests/template.rs` `template_is_all_defaults` hard-codes the editor key list, so it needed the five new keys added (outside the named scope, unavoidable).
