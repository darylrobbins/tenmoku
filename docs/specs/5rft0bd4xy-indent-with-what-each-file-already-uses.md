---
task_uid: "01m4pz6zzzd16k6b5rft0bd4xy"
task: "T458"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test indent_style::tests::detects: a tab-indented Go sample gives Tabs, a 2-space JavaScript sample Spaces(2), a 4-space Python sample Spaces(4), a file with 3 indented rows None, and 10,000 2-space-indented rows followed by 50,000 tab-indented rows Spaces(2)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test indent_style::tests::editing_follows_it: with Tabs, tab inserts a tab character and outdent removes one; with Spaces(2), tab at column 1 inserts one space and indent_rows inserts two; with None, tab_size spaces as before"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test pane_item::indent_status_tests::shows_the_detected_style: a tab-indented file shows Tab Size: 4, a 2-space file Spaces: 2, and after editor.tab_size changes to 8 the 2-space file still shows Spaces: 2"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-editor -p fe-gpui -p fe-workbench and cargo clippy on those crates --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Indent with what each file already uses: detect tabs or a space width when it opens, and use it for tab, indent, outdent and the status bar

## Problem

Every file is edited as if it were indented with `tab_size` spaces. `EditorModel::tab`
(`crates/fe-editor/src/lib.rs` :900) always inserts spaces to the next `tab_size` stop, and the status bar's
indentation item always says `Spaces: <tab_size>` (`crates/fe-gpui/src/pane_item.rs` :373). So pressing tab in a
tab-indented Go file or Makefile inserts spaces, and a 2-space JavaScript file gets 4-space indents unless the user
changes the global `editor.tab_size` setting. The plan's M6 is "Editing polish" (plan line 110). No card on the
board detects indentation (2026-10-11 scoping pass).

## Change

New module `crates/fe-editor/src/indent_style.rs` (`mod indent_style;` in lib.rs, types exported):

- `pub enum IndentStyle { Tabs, Spaces(u8) }` and `pub fn detect_indent_style(text: &Text) -> Option<IndentStyle>`.
  It reads at most the first 10,000 rows, through the rope's chunks (no String of the whole text). Rows starting
  with a tab count for tabs. For rows indented with spaces, it takes the increase in leading spaces between each
  non-blank row and the non-blank row before it; the width is the most common increase from 2 to 8 (ties go to the
  smaller). Tabs win when more rows start with a tab than with two or more spaces. Fewer than 5 indented rows: `None`.
- `EditorModel` detects it when made (`EditorModel::new` and `open`, lib.rs :118 / :145) and keeps it in a field.
  `indent_style() -> Option<IndentStyle>` reads it, `set_indent_style(Option<IndentStyle>)` overrides it.
  `indent_unit() -> String` is `"\t"` for `Tabs`, `n` spaces for `Spaces(n)`, and `tab_size` spaces for `None`.
- `tab()`: with `Tabs` it inserts `"\t"` at each caret; with `Spaces(n)` it inserts spaces to the next multiple of
  `n`; with `None`, as today. `tab_size` stays the display width of a tab in every case.
- In rows.rs (the previous card): `indent_rows` inserts `indent_unit()`; `outdent_rows` removes a leading tab, else
  up to the unit's width in leading spaces (`tab_size` for `Tabs` and `None`).

`crates/fe-gpui/src/pane_item.rs` :373: the indentation item says `Tab Size: <tab_size>` for `Tabs`,
`Spaces: <n>` for `Spaces(n)`, and `Spaces: <tab_size>` for `None`.

No new setting: detection is always on, and `editor.tab_size` keeps its meaning (tab width, and the unit for files
with too little indentation to tell).

## Tests

New files `crates/fe-editor/src/indent_style_tests.rs` (declared from indent_style.rs with `#[path]`) and
`crates/fe-gpui/src/indent_status_tests.rs` (declared from pane_item.rs with `#[path]`, next to its existing tests
module).

## Implementation notes

- Blank (whitespace-only) rows count for nothing. "Indented rows" are non-blank rows starting with a tab or a
  space; rows starting with a tab don't move the baseline the next spaced row's increase is measured from.
- Once a row has a non-blank byte, the detector skips to its line break, so a long row costs a byte search.
- Splits keep the model's style. A reload or edit doesn't re-detect; `set_indent_style` is the override.
