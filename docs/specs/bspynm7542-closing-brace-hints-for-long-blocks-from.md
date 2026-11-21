---
task_uid: "01m6w0adzqpnzbm2bspynm7542"
task: "T1593"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::closing_brace_hints (cargo nextest run -p fe-workbench) pass: a_long_function_gets_fn_name_after_its_last_row, a_block_under_the_minimum_gets_none, impl_trait_and_mod_labels, a_server_hint_with_the_same_text_at_that_offset_wins, the_setting_off_shows_none, the_caret_skips_the_hint"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui --test inlay_closing_kind passes (the Closing kind draws faint text without a pill)"
    required: true
    manual: false
  - id: "C3"
    text: "Test defaults_are_on_and_25 (cargo nextest run -p fe-config --test closing_brace_setting) passes"
    required: true
    manual: false
  - id: "C4"
    text: "UPDATE_GOLDEN=1 regenerates the settings schema and the generated settings.ftl and settings-template.ftl; cargo nextest run -p fe-config passes with them"
    required: true
    manual: false
---

# Closing-brace hints for long blocks from the outline, in every language: } fn render

# Closing-brace hints: `} fn render` after a long block

Spec: `docs/design/design-system.md#editor` ("Closing-brace hints in faint
italic after a block's last brace: } fn render"). rust-analyzer can send these
as inlay hints; for every other language, and without a server, nothing shows
them.

## Change

- `editor.closing_brace_hints` (bool, default true) and
  `editor.closing_brace_hints_min_rows` (default 25): `crates/fe-config/src/settings.rs`,
  schema, template, generated catalogue.
- `crates/fe-workbench/src/closing_brace_hints.rs` (new): from the editor's
  outline (`fe_syntax::outline`) every Function, Impl, Trait, Struct, Enum,
  Class, Module or Interface item spanning at least the minimum rows gets a
  hint at the end of its last row: `fn render`, `impl Foo`, `mod tests` (the kind's
  keyword and the name). Drawn through the editor's inlay-hint path
  (`EditorView::set_inlay_hints`, merged with the language server's in
  `lsp_inlay_hints.rs`), with a new `InlayHintKind::Closing` that draws plain
  faint text without a pill (`crates/fe-gpui/src/inlay_hints.rs`). A server hint at
  the same offset with the same text wins (no duplicates).
- Updates with the outline, not per keystroke; nothing for a file with no
  outline query. The hint is not part of the buffer and the caret skips it.
- The label is the item kind's keyword (`fn`, `impl`, `mod`...) and its name, both verbatim.

## Not in this card

Italic (a one-line change once italic faces land: the font-style cards), clicking a hint,
a hint for closing tags in HTML.

Platform: **Linux-OK**. No catalogue text: the labels are code words and names, shown verbatim (`verbatim!`).
