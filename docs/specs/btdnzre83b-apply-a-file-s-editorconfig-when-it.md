---
task_uid: "01m4qzfdvkjtzmngbtdnzre83b"
task: "T478"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test pane::editorconfig_tests::indent_and_tab_width: a space-indented file under [*.go] indent_style = tab opens with Tabs (status Tab Size), a [*.js] indent_size = 2 file with Spaces(2), and tab_width = 8 survives a settings reload that changes editor.tab_size"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test pane::editorconfig_tests::line_ending_and_none: end_of_line = crlf on an LF file shows CRLF and saves CRLF; a file with no .editorconfig keeps its detected style and the tab_size setting"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Apply a file's .editorconfig when it opens: indent style and size over detection, tab width over the setting, and its line ending

## Problem

T467 reads a file's `.editorconfig` properties; nothing applies them. The user (2026-10-12): EditorConfig wins over
T458's indentation detection.

## Change

In `crates/fe-workbench/src/pane.rs` only:

- For each editor a pane gains for a path (opened, split from a peer, or restored), `editorconfig_for(path)` runs
  on a background task (`cx.background_spawn`) and its result is applied on the main thread, if the editor is
  still open:
  - `indent_style = tab` → `set_indent_style(Some(Tabs))`; `space` with `indent_size = n` →
    `Some(Spaces(n))`; with only `indent_size`, the style stays detected and a detected `Spaces` takes `n`.
    These replace T458's detected style.
  - `tab_width` → the editor's tab size, overriding `editor.tab_size` for that editor. The pane keeps each tab's
    EditorConfig, and `Pane::reload_settings` (:514) re-applies its `tab_width` after `reload_settings`, so a
    settings reload doesn't undo it.
  - `end_of_line = lf | crlf` → the buffer's line ending (`Buffer::set_line_ending`), so the status bar shows it and
    saving writes it; `cr` is ignored (fe-buffer has no lone-CR ending).
  - `insert_final_newline` and `trim_trailing_whitespace` are kept on the tab for the save card.
- Properties are read once per open. Editing a `.editorconfig` applies to files opened afterwards.

## Tests

New file `crates/fe-workbench/src/editorconfig_tests.rs`, declared from pane.rs with `#[path]`, on temp projects.
