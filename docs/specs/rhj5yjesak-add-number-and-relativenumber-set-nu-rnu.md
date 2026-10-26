---
task_uid: "01m5qphzt6s8s3hfrhj5yjesak"
task: "T235"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display test layout::line_number_modes_tests::the_four_modes (crates/fe-display/src/layout/line_number_modes_tests.rs): number, relativenumber, both and neither give the expected gutter strings for a mid-buffer caret, with the caret row 0 in relative-only and absolute in hybrid; gutter width is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "layout::line_number_modes_tests::relative_counts_rows_like_nvim: a closed fold counts as one row, a wrapped row once, several carets count from the primary, and the first and last row carets are right"
    required: true
    manual: false
  - id: "C3"
    text: "Integration test ex_set_number_forms (top level, crates/fe-vim/tests/ex_set_number.rs): :set nu rnu, nonu, rnu!, invnu, nu? and rnu? set the model's flags and print nvim's messages, and :set number=3 is an error"
    required: true
    manual: false
  - id: "C4"
    text: "Integration test editor_line_numbers_setting (top level, crates/fe-config/tests/editor_line_numbers_setting.rs): default on, the four values, a bad value is a located schema error, schema golden passes"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui test line_numbers_tests::reload_applies_and_set_survives: a settings reload applies editor.line_numbers to open editors; a per-editor :set nu survives a reload that changed something else"
    required: true
    manual: false
---

# Add 'number' and 'relativenumber': :set nu rnu and editor.line_numbers (on, off, relative, hybrid) change the gutter's numbers, the caret's row absolute in hybrid (Mac T1165)

## Problem

The user asked on 2026-10-24 for the 'number' and 'relativenumber' options, settable with `:set` and in settings (gutter shows relative numbers, the current line absolute when both are on, as `set nu rnu`). Relative numbers are how vim users read `5j`/`12k` counts, and every LazyVim, Zed and IdeaVim setup turns them on.

## Current state

- The gutter always shows absolute 1-based numbers: `layout.rs:1534-1546` shapes `(row.buffer_row + 1).to_string()` per buffer row, styled `active_line_number` for rows with a caret; its width fits the digits of the row count (`gutter_width`, `crates/fe-display/src/layout.rs:599`). `FrameInput` literals are built in about ten test files, so a new field there forces edits in all of them: carry the mode on `DisplayMap` instead (like `wrap_width`, `crates/fe-display/src/display_map.rs:185`).
- `:set` accepts `wrap ts sw et clipboard ic scs is` (`crates/fe-vim/src/ex/set.rs:41-62`, a bool is applied by `apply`, e.g. `target.model.set_soft_wrap(on)` at :208); the model keeps `soft_wrap` (`crates/fe-editor/src/lib.rs:578`). Editor settings reach views in `crates/fe-gpui/src/view.rs:660-672` (`apply_settings`, `reload_settings`, which keeps a per-editor toggle on reload of something else) and `crates/fe-gpui/src/pane_item.rs:933`.

## Change

- Setting `editor.line_numbers`: `"on"` (default, today), `"off"`, `"relative"`, `"hybrid"` (settings.rs, schema, template).
- `EditorModel` holds `number` and `relative_number` bools (fe-editor lib.rs; `on` = number only, `relative` = relativenumber only, `hybrid` = both), `set_line_numbers`/`line_numbers`; pushed to `DisplayMap`. Layout: number only: as today; relativenumber only: the distance in rows from the primary caret's row, the caret's row showing 0 (nvim `nonu rnu`); both: relative, but the caret's row shows its absolute number (nvim `nu rnu`); neither: no numbers (the gutter's marks, fold column and change lane stay). Distance counts a closed fold as one row and a wrapped row once, as nvim does. Gutter width is unchanged.
- `:set number`/`nu`, `relativenumber`/`rnu` (`no`, `inv`, `!` and `?` forms, messages `  number` / `nonumber` as nvim prints), a per-editor override like `:set wrap`: a reload of other settings keeps it.
- Out of scope: 'numberwidth', 'signcolumn', `cursorlineopt=number`, the file viewer and diff views (they keep absolute numbers).

## Tests

New `crates/fe-display/src/layout/line_number_modes_tests.rs` (one `#[path]` line in layout.rs): the four modes on a 12-row text with the caret mid-buffer, a closed fold, a wrapped row, a caret on the first and last rows, several carets (relative to the primary). New `crates/fe-vim/tests/ex_set_number.rs`: `:set nu rnu` forms and messages through HeadlessVim. New `crates/fe-config/tests/editor_line_numbers_setting.rs`. fe-gpui `crates/fe-gpui/src/line_numbers_tests.rs`: a settings reload applies, a per-editor `:set` survives an unrelated reload.

## Implementation notes

- The flags live on `DisplayMap` as `fe_display::LineNumbers { number, relative }`; `EditorModel` forwards `line_numbers`/`set_line_numbers` and the per-flag `number`/`relative_number` setters, and `split` copies them (a split keeps its window's options, as vim's do).
- Distance counts visible lines: `DisplayMap::visible_line` is the buffer row less the buffer rows folds hide through it (a hidden row lands on its fold's header), computed once per frame for the primary caret and the frame's first row; the rows starting a buffer row in a frame are consecutive lines after that.
- With 'relativenumber', only the primary caret's row shows 0 (or its number with 'number'); other carets' rows count from it but keep the active style.
- `editor.line_numbers` reaches new editors in `Pane::new_editor` and restored ones in `EditorView::restore`; `EditorView::reload_settings` applies it only when the reload changed it. `fe_gpui::line_numbers` maps the setting to the flags.
- The diff view resets its sides to absolute numbers, since they're split from an open editor (one line in `crates/fe-gpui/src/diff_view.rs`, outside the listed scope).
- nvim 0.12 reports `:set number=3` as `E474: Invalid argument`; `:set` already reports a bool given a value as `E475: Invalid argument: …` (as `ex_set_search` pins for `ic=1`), so `number=3` gets that too and the test checks the `Invalid argument: number=3` part.
