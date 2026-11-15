---
task_uid: "01m6w0a3mmdvafb8x1bcrqd76v"
task: "T1587"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::problems_detail (cargo nextest run -p fe-workbench) pass: shows_code_source_message_and_a_three_line_snippet, the_preferred_quickfix_shows_as_a_celadon_line_with_a_plus_glyph, apply_fix_applies_the_fixs_edit, no_fix_means_no_button_and_alt_enter_does_nothing"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-lsp diagnostics and -p fe-editor diagnostics pass with Problem.code; cargo nextest run -p fe-keymap --test problems_detail_keys passes (alt-enter in the problems_panel context)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac, the audit scene named in the body renders in both themes and its probes pass (recorded in the decision file) (audit/problems_detail.rs)"
    required: true
    manual: false
---

# Problems detail pane: code and source, a three-line snippet, the suggested fix as a green line, Apply fix on alt-enter

# Problems detail pane: snippet, suggested fix, Apply fix

Spec: `docs/design/design-system.md#problems` ("The selected problem opens a
detail pane: code and source, a three-line snippet with the suggested fix as a
green line, then Apply fix ⌥↵, Ask an agent and Explain"). The Ask and Explain
buttons are the agent-actions cards; this one leaves their place.

## Change

- `Problem` (`crates/fe-editor/src/diagnostics.rs`) gains `code: Option<String>`
  from the diagnostic's `code`; `crates/fe-lsp/src/diagnostics.rs` fills it.
- `crates/fe-workbench/src/problems_detail.rs` (new): below the list, for the
  selected problem: `code · source` in code small (the code links to
  `code_href` when present), the message, a snippet of the problem's row and
  one row either side in the editor font (the underlined range in the
  severity colour; read from the open buffer, else the file off the main
  thread), then, when the language server has a preferred quick fix for it
  (`lsp_code_actions.rs`: `quickfix` kind, `isPreferred` first), the fixed row
  as a celadon-soft line with a + glyph and the fix's title.
- `Apply fix` (⌥↵ in the panel's key context, `crates/fe-keymap/keymaps/fastedit.toml`) applies that
  action's edit through the existing code-action path; with no fix the button
  is absent and ⌥↵ does nothing.
- An audit scene `audit/problems_detail.rs`, both themes.

## Not in this card

Ask an agent and Explain (a later card), fixes needing a command or a choice (the list
shows "N fixes" and opens the code-action picker), Project-wide fixes.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-problems-…` in
`crates/fe-workbench/l10n/en-US/problems.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**; the audit scene is recorded on the Mac.
