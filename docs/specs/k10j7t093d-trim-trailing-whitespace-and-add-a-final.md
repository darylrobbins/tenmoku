---
task_uid: "01m4qzfysbzg5knmk10j7t093d"
task: "T481"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test pane_item::save_fixes_tests::trim_and_final_newline: with both fixes on, saving \"a  \\nb\\t\" writes \"a\\nb\\n\" (CRLF for a CRLF buffer) as one transaction cmd-z undoes; a text already clean makes no transaction; with both off it saves unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test pane::editorconfig_save_tests::from_the_editorconfig: a file under trim_trailing_whitespace = true and insert_final_newline = true saves trimmed with a final newline; one under false for both, or with no .editorconfig, saves as typed; after format on save, the fixes apply to the formatted text"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-gpui -p fe-workbench and cargo clippy on both --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Trim trailing whitespace and add a final newline on save where a file's .editorconfig asks for them

## Problem

T478 applies a file's EditorConfig indentation, tab width and line ending when it opens, and keeps its
`insert_final_newline` and `trim_trailing_whitespace` on the tab; nothing acts on them when saving. `EditorView::save`
(`crates/fe-gpui/src/pane_item.rs`, T432) is the one save path for cmd-s, vim's `:w` and closing, and T479 makes it
await a save hook (format on save) first.

## Change

- fe-gpui (`crates/fe-gpui/src/pane_item.rs`): `pub struct SaveFixes { pub trim_trailing_whitespace: bool, pub
  insert_final_newline: bool }` and `EditorView::set_save_fixes(SaveFixes)` (default both false). `EditorView::save`
  applies them after the save hook and before `prepare_save`: trimming removes spaces and tabs at the end of every
  row; the final newline adds the buffer's line ending when the text is non-empty and doesn't end with one. Both
  together are one transaction of the user, so cmd-z undoes them; with nothing to change there's no transaction.
- fe-workbench (`crates/fe-workbench/src/pane.rs`): when T478 applies a tab's EditorConfig, it sets the editor's
  `SaveFixes` from `trim_trailing_whitespace = true` and `insert_final_newline = true`. `false` and unset do nothing.
- Files without these properties save exactly as before.

## Tests

New files `crates/fe-gpui/src/save_fixes_tests.rs` (declared from pane_item.rs with `#[path]`) and
`crates/fe-workbench/src/editorconfig_save_tests.rs` (declared from pane.rs with `#[path]`), on temp projects.
