---
task_uid: "01m77satx0mdv46nvf19jf8c83"
task: "T356"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist); USER_SNIPPET, its l10n-allow.toml entry and kind_name are gone"
    required: true
    manual: false
  - id: "C2"
    text: "en-US byte-equal; cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
  - id: "C3"
    text: "New tests in crates/fe-workbench/src/tests/lsp_fragments_locale.rs pass: under qps-ploc a description-less user snippet's detail and each of the four kinds' (source.organizeImports, source.fixAll, refactor, source) empty-answer message are whole fake-locale messages"
    required: true
    manual: false
---

# Snippet picker's user-snippet detail and the empty code-action message from the catalogue (T1561 review F1/F3 follow-up) (Mac T1728)

# Snippet detail and empty code-action messages from the catalogue (T1561 review F1/F3 follow-up)

T1561 (i18n W6) left two English strings that its review (codex F1, F3) carried as known limits:

- **F1.** `snippet_picker.rs` (`snippet_rows`) falls back to `extension_snippets::USER_SNIPPET`
  (`"User snippet"`) for a user snippet with no description. Use the existing catalogue accessor
  `extension_snippets::user_snippet()` instead. `USER_SNIPPET` then has no reader: remove it, update the
  doc comment that links it in `extension_snippets.rs`, and drop its `[[literal]]` entry from
  `crates/fe-workbench/l10n-allow.toml` (the lint rejects an entry that matches nothing).
- **F3.** `lsp_code_actions.rs` builds "No { $kind } actions available" from `kind_name()`, which makes an
  English phrase out of the kind id ("organize imports", "fix all"). Replace `kind_name` with a choice keyed
  on the kind id: `workbench-lsp-code-actions-none-of-kind` in `l10n/en-US/lsp.ftl` becomes a selector whose
  variants are whole messages for the kinds the commands ask for (`source.organizeImports`, `source.fixAll`,
  `refactor`, `source`; `workbench_commands.rs` `code_actions_of` callers). Fluent variant keys can't hold a
  dot, so map each kind id to a key in code (e.g. `organize-imports`, `fix-all`, `refactor`, `source`); any
  other kind takes `workbench-lsp-code-actions-none`. Add a translator comment listing the variants.

en-US stays byte-equal: `snippet_picker_tests.rs` ("User snippet") and `lsp_code_action_kinds_tests.rs`
("No organize imports actions available") pass unchanged.

A new test module, `crates/fe-workbench/src/tests/lsp_fragments_locale.rs` (declared from
`src/tests/mod.rs`, the pattern of `settings_editor_locale_tests.rs`: a `qps-ploc` `Resources` submitted with
`inventory::submit!`, then `kiln_platform::locale::set_locale`), checks under the fake locale that a
description-less user snippet's detail is the fake word and that each of the four kinds' empty answer is
its whole fake message with no English fragment.

Platform: **Linux-OK**.
