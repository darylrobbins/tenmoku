---
task_uid: "01m5n9hzdzn4csf2f0ndnrh94z"
task: "T214"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "LspManager::linked_editing_ranges decodes ranges to bytes (UTF-8 and UTF-16), Ok(None) without the provider, empty for null or timeout; editor.linked_editing (default false) is in settings, template and schema"
    required: true
    manual: false
  - id: "C2"
    text: "With the setting on and the caret rested 250 ms in a linked range, the active editor's linked ranges are set from the server's answer and typing in the open tag renames the close tag; moving away, switching editors or the setting off clears them; with it off no request is sent"
    required: true
    manual: false
  - id: "C3"
    text: "New tests in linked_editing_tests.rs, lsp_linked_editing_tests.rs and linked_editing_setting.rs pass headless"
    required: true
    manual: false
---

# Add linked editing: the textDocument/linkedEditingRange request and the editor.linked_editing setting, renaming an HTML tag's pair as you type, as VS Code does (Mac T1111)

## Problem

Follows the linked ranges model card (`EditorModel::set_linked_ranges`). fastedit never sends `textDocument/linkedEditingRange` (`rg linkedEditing crates` is empty) and has no `editor.linkedEditing`. In VS Code, with `editor.linkedEditing` on and a server that provides it (HTML, Vue, Svelte, JSX servers), renaming `<div>` also renames `</div>`.

## Change

1. fe-lsp: new `crates/fe-lsp/src/linked_editing.rs` (+ `pub mod linked_editing`, `pub use LinkedEditing` in lib.rs): `LspManager::linked_editing_ranges(buffer, version, offset) -> Future<Result<Option<LinkedEditing>, LspError>>` with `LinkedEditing { ranges: Vec<Range<usize>>, word_pattern: Option<String> }` (bytes of the request version, sorted, duplicates dropped); `Ok(None)` and nothing sent without `linkedEditingRangeProvider`; null reply or timeout is `Ok(Some(LinkedEditing::default()))` (no ranges). Client capability in server.rs (`linked_editing_range: { dynamic_registration }`), method added to the hover timeout group in requests.rs.
2. Setting `editor.linked_editing` (bool, default false, as VS Code): crates/fe-config/src/settings.rs (`EditorSettings`, next to `code_lens`), settings-template.toml, settings.schema.json; propagated like `code_lens_on` (workbench.rs :312/:606, workbench_root.rs :223-225) into a `Cell` on the workbench's lsp state.
3. New `crates/fe-workbench/src/lsp_linked_editing.rs` (mod in lsp.rs; `sync_linked_editing` beside `sync_highlights`): with the setting on, for the active editor, once the caret has rested 250 ms and is not in an already linked range, ask at the caret; if the answer has 2 or more ranges and one contains the caret, call `set_linked_ranges`; otherwise `clear_linked_ranges`. Answers for an old caret or version are dropped. Turning the setting off, switching editors or a stopped server clears. `word_pattern` is ignored (the model's fixed rule applies).

## Tests

- `crates/fe-lsp/src/linked_editing_tests.rs` (#[path] mod in linked_editing.rs): `ranges_decode_to_bytes_utf8_and_utf16`, `no_provider_sends_nothing`, `null_reply_and_timeout_are_empty`, `word_pattern_is_returned`.
- `crates/fe-workbench/src/lsp_linked_editing_tests.rs` (#[path] mod in lsp_linked_editing.rs; fake server for an `html`-like file with `<div></div>`): `typing_in_the_open_tag_renames_the_close_tag`, `setting_off_sends_no_request`, `caret_outside_the_ranges_links_nothing`, `old_caret_answer_is_dropped`, `moving_away_clears_the_link`.
- `crates/fe-config/tests/linked_editing_setting.rs`: default false, parses `true`, appears in the schema.
Linux-OK (headless). Depends on the linked ranges model card. Shares settings.rs/template/schema with T1031 and the other settings cards of this batch.
