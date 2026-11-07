---
task_uid: "01m6rmd9tnc115wr3h9aftjte3"
task: "T476"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test (tests/format_on_type_settings.rs) both_default_off_and_parse: editor.format_on_type and editor.format_on_paste default false, parse true, and appear in the template and schema"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test on_type_formatting_tests::dynamic_registration_is_accepted_with_trigger_characters: registering textDocument/onTypeFormatting is now accepted and its trigger chars are recorded; the existing unsupported-method case in tests/registration.rs still refuses another method"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test on_type_formatting_tests::request_carries_position_char_and_options: a fake server receives textDocument/onTypeFormatting with the typed char and the buffer's tab size and insert_spaces"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test lsp_format_on_type_tests::typing_a_trigger_char_applies_the_edits_as_one_undo_step: with the setting on and a fake server returning an edit for '}', typing '}' formats and one undo reverts both; with the setting off nothing is requested"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test lsp_format_on_type_tests::stale_answers_and_pastes_are_ignored: an answer arriving after another edit is dropped; a pasted '}' and an IME composition send no on-type request; two carets send none"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test lsp_format_on_type_tests::paste_formats_the_pasted_range_only: with format_on_paste on and a server with rangeFormatting, pasting 3 rows requests range formatting of exactly the pasted range and applies the edits in one undo step with the paste; a one-row paste and a server without the capability request nothing"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Add editor.format_on_type (LSP textDocument/onTypeFormatting) and editor.format_on_paste (range formatting of the pasted range), both off by default (Mac T1399)

Lent from Mac T1399 (lend y42, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

## Problem

VS Code's `editor.formatOnType` and `editor.formatOnPaste` are missing. Only format on save and Format Selection exist (`crates/fe-workbench/src/lsp_format.rs`). `fe-lsp` refuses a dynamic `textDocument/onTypeFormatting` registration today (`crates/fe-lsp/src/tests/registration.rs` `ON_TYPE_FORMATTING` is in the unsupported list), and the client does not advertise the capability.

Linux-OK: fake-server tests only.

## Change

- Settings `editor.format_on_type` and `editor.format_on_paste`, both bool, default false: `crates/fe-config/src/settings.rs`, `settings-template.toml`, `settings.schema.json`.
- `crates/fe-lsp/src/formatting.rs`: `on_type_formatting(doc, position, ch, options)` request; advertise `documentOnTypeFormattingProvider` support in the client capabilities and read the server's `firstTriggerCharacter`/`moreTriggerCharacter` (static) and accept the dynamic registration (change `registration.rs`: move the method out of the unsupported list; the existing registration test file is not extended, add the new test file below and update the old test's expectation in place).
- `crates/fe-workbench/src/lsp_format_on_type.rs` (new): after a typed character that is a trigger char (typing path only, not paste, not IME composition, not vim ex/insert-replay macros), ask the server, apply the returned edits through the same overlap-safe path `lsp_format.rs` uses (`lsp_format_overlap_tests.rs` is the model), as one undo step merged with the typing that caused it. Stale responses (buffer changed since) are dropped. Multi-cursor: only for a single caret.
- Format on paste: after a paste (and vim `p`/`P` when vim is on) of a range, if the server supports `rangeFormatting` and the pasted text spans more than one row, request range formatting of the pasted range and apply edits, one undo step with the paste; same stale/overlap rules. External formatters (`external_format_wiring_tests.rs`) are not used for these two; only the language server.

## Out of scope

Plugin formatters (T1356), tree-sitter indent-based on-type fixups (auto-indent already exists).

## Conflicts

Settings triple (with Start page and word-wrap-column cards), `lsp.rs`/`lsp_format.rs` (running work: none found in `rgt status`). Not related to batch-1 cards.


## Helium notes
- A setting goes live on helium only through a line in crates/fe-workbench/src/workbench_root.rs `reload_settings` (Pane.settings is private; GlobalSettings misses per-root settings). If the new keys must change behaviour without a restart, respec workbench_root.rs into scope before the first finish (`rgt task respec --scope`, which replaces the list).

## Implementation notes

- **Scope widened (respec).** The workbench can't tell typing from a paste,
  a composition or a vim replay from outside fe-gpui, so fe-gpui gained
  `EditorInputEvent` (`view/host.rs`): `Typed` is emitted only from the input
  handler's text with nothing composing (`view/ime.rs`), never from a vim
  replay or map (those type through `type_replayed_key`); `Pasted` from
  `editor.paste` (`commands.rs`) and `vim.put`/`vim.visual_put`
  (`vim.rs` `run_vim_once`, skipped while `vim_replaying`), user actor only,
  with the span of every edit the command made. fe-lsp's `manager.rs`
  (`on_type_formatting_triggers`, each document's URI and language id for
  selector matching), `server.rs` (client capability, dynamic registration
  on) and `requests.rs` (the 3 s format timeout) were added, and
  `workbench_root.rs`/`workbench.rs` so the settings go live on reload.
- **Undo.** The edits join the typing's or paste's transaction through
  `Buffer::edit_in` (the newest applied `Origin::User` transaction when the
  event arrives, which the version check guarantees is still it), so one
  undo takes back both.
- **Registration.** `textDocument/onTypeFormatting` registrations need
  parseable options; a `documentSelector` is matched by language id, scheme
  and glob pattern, a null one takes every document of the server.
- **Review rounds.** Enter (`editor.newline` from its key, vim off or in
  insert mode) is a typed `\n`. A vim put's range and row count come from
  fe-vim itself (`VimState::put_landed`, set by `vim.put` and
  `vim.visual_put`): the register's text as landed, without the separator
  line breaks a put adds below the last row or over characters, its rows
  counting a linewise register's empty last line. The workbench formats a
  paste when it takes up two rows or more.
- **Paste ranges.** An `editor.paste` is its text from the primary
  selection's start to the caret it leaves, so linked-range and snippet
  mirrors aren't in it; a Linux middle-click paste reports the same. A
  visual put of characters over a block spans every row it went on, and a
  block register's rows count an empty last one.
