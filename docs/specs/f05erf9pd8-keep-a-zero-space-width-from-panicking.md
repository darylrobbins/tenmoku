---
task_uid: "01m4spt09zcy86h5f05erf9pd8"
task: "T615"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "set_indent_style(Some(IndentStyle::Spaces(0))) leaves indent_style() as None, and tab(), indent and outdent then use tab_size spaces without panicking; widths of 1 and more are kept as given (test in crates/fe-editor/tests/indent_style_bounds.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "detect_indent_style returns None for five rows of two spaces then U+00A0, and for rows of spaces then a form feed; real indented rows after such rows still detect as before"
    required: true
    manual: false
  - id: "C3"
    text: "Every existing fe-editor indent_style test still passes unchanged"
    required: true
    manual: false
---

# Keep a zero space width from panicking tab and treat Unicode-whitespace rows as blank in indent detection (T458 review F1, F2)

## Problem

Two findings from the codex review of T458 (approved 2026-10-12, never filed):

- **F1** `crates/fe-editor/src/lib.rs` `EditorModel::set_indent_style` stores any width. After
  `set_indent_style(Some(IndentStyle::Spaces(0)))`, `tab()` panics on `column % width` (remainder by
  zero, lib.rs `tab`), indent and space outdent (`rows.rs`) silently do nothing, and the status bar shows
  `Spaces: 0`. The workbench's editorconfig path clamps to 1 or more (`fe-workbench/src/pane.rs` `spaces`),
  but the method is public and unguarded.
- **F2** `crates/fe-editor/src/indent_style.rs` `detect_indent_style`: every byte other than space, tab,
  CR and LF marks a row non-blank, so whitespace-only rows holding a form feed or Unicode whitespace count
  as indentation evidence. Five rows of `"  \u{00a0}\n"` detect `Spaces(2)`; the spec's blank-row rule
  says `None`.

## Change

- `set_indent_style` stores `Some(IndentStyle::Spaces(0))` as `None` (the `tab_size`-spaces default).
  Widths of 1 or more are kept as given. Update the `IndentStyle::Spaces` doc: detected widths are 2 to 8,
  set ones are 1 or more.
- In `detect_indent_style`, a row is blank when it holds only whitespace (`char::is_whitespace`, so `\x0b`,
  `\x0c`, U+00A0, U+3000 and the like). Leading spaces still count only up to the first non-space
  character. Keep the chunk-based scan and `MAX_ROWS`.
- Tests go in the new `crates/fe-editor/tests/indent_style_bounds.rs`.
