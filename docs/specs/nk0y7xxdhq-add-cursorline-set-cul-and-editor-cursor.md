---
task_uid: "01m5qphzzhnmfgyenk0y7xxdhq"
task: "T237"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Integration test ex_set_cursorline_forms (top level, crates/fe-vim/tests/ex_set_cursorline.rs): :set cul, nocul, invcul, cul! and cul? set the model flag and print '  cursorline' or 'nocursorline'; cul=1 is an error"
    required: true
    manual: false
  - id: "C2"
    text: "Integration test editor_cursor_line_setting (top level, crates/fe-config/tests/editor_cursor_line_setting.rs): default true, a bad value is a located schema error, schema golden passes"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test cursor_line_tests::band_follows_the_option: painted by default, absent after :set nocul, back after :set cul, and applied from a settings reload"
    required: true
    manual: false
  - id: "C4"
    text: "cursor_line_tests::set_survives_an_unrelated_reload: a per-editor :set nocul stays through a reload that changed another setting, as :set wrap does"
    required: true
    manual: false
---

# Add 'cursorline': :set cul and editor.cursor_line turn the caret row's band off or on (default on, as now) (Mac T1167)

## Problem

The user asked on 2026-10-24 for the 'cursorline' option, settable by `:set` and in settings. Vim users toggle it (`:set cursorline`, LazyVim turns it on) and expect `:set nocursorline` to remove the band across the caret's row.

## Current state

The editor always paints the active-line band: `frame.active_lines` (`crates/fe-display/src/layout.rs:1512`, one rect per row with a caret) is filled with `theme.active_line` at `crates/fe-gpui/src/element.rs:1455` (VS Code's `renderLineHighlight: line`, with no way to turn it off); the file viewer paints it too (`crates/fe-gpui/src/file_viewer/element.rs:322`). `:set` (`crates/fe-vim/src/ex/set.rs:41-62`) has no such option; model flags like `soft_wrap` are in `crates/fe-editor/src/lib.rs:578`; views get settings at `crates/fe-gpui/src/view.rs:660-672`.

## Change

- Setting `editor.cursor_line` (bool, default true = today; a comment says nvim's default is off and VS Code's on, and that fastedit keeps the editor default) in settings.rs, schema, template.
- `EditorModel::cursor_line`/`set_cursor_line` (fe-editor lib.rs). `element.rs:1455` skips the band when it is off (the number's `active_line_number` colour stays: that is 'cursorlineopt' number, a different option).
- `:set cursorline`/`cul`, `nocul`, `invcul`, `cul!`, `cul?` (messages `  cursorline` / `nocursorline`), per editor, surviving an unrelated settings reload as `:set wrap` does.
- Not here: 'cursorcolumn', 'cursorlineopt'.

## Tests

New `crates/fe-vim/tests/ex_set_cursorline.rs`; new `crates/fe-config/tests/editor_cursor_line_setting.rs`; new `crates/fe-gpui/src/cursor_line_tests.rs` (TestAppContext): the band is painted by default, absent after `:set nocul`, back after `:set cul`, follows a settings reload, a per-editor `:set` survives an unrelated reload; the paint check reads the frame's `active_lines` use through a small `EditorView::paints_cursor_line()` accessor.
