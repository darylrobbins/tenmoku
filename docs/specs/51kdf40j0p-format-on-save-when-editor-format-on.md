---
task_uid: "01m50ew2jgswe79j51kdf40j0p"
task: "T115"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test pane_item::save_hook_tests::hook_runs_before_the_write: EditorView::save awaits the hook's edit and writes the edited text; a hook that never finishes delays the save by 1 s and the file is still written"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test lsp_format::save_tests::formats_when_on: with format_on_save on, cmd-s writes the fake server's formatted text and cmd-z undoes the format; off (the default), or with no server for the language, the file saves unformatted with no request"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-gpui -p fe-workbench and cargo clippy on both --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Format on save: when editor.format_on_save is on, saving formats through the language server first, waiting at most a second (Mac T479)

## Problem

The user (2026-10-12) asked for format on save. T466 adds the `editor.format_on_save` setting, T475 adds
`Workbench::format_buffer`, and T432 moves saving into `EditorView::save` (`crates/fe-gpui/src/pane_item.rs`), which
cmd-s, vim's `:w` and closing all go through. Nothing formats before a save.

## Change

- fe-gpui (`crates/fe-gpui/src/pane_item.rs`): `pub trait SaveHook { fn before_save(&self, editor:
  &Entity<EditorView>, cx: &mut App) -> Task<()>; }` and `EditorView::set_save_hook(Option<Rc<dyn SaveHook>>)`.
  `EditorView::save` awaits the hook's task before `prepare_save`, at most 1 s (then it saves anyway), so the text
  saved includes what the hook changed. A save started while a hook runs waits for the same save, as T432's
  one-save-at-a-time rule says.
- fe-workbench (`crates/fe-workbench/src/lsp_format.rs`): `FormatOnSave`, a `SaveHook` that, when
  `editor.format_on_save` is on (read from the current settings on each save, so a reload applies at once), runs
  `format_buffer` for the editor and logs (debug) a timeout, error or `NoFormatter` without prompting. It never
  formats a buffer whose language has no server, and never blocks a save for more than the 1 s limit.
- `crates/fe-workbench/src/lsp.rs`: set the hook on every editor where it sets the hover and completion providers
  (:681).

## Tests

New files `crates/fe-gpui/src/save_hook_tests.rs` (declared from pane_item.rs with `#[path]`) and
`crates/fe-workbench/src/lsp_format_save_tests.rs` (declared from lsp_format.rs with `#[path]`), on fe-lsp's fake
server and a temp project.
