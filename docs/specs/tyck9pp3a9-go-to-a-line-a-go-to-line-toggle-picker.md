---
task_uid: "01m4pzqn4emnrpyctyck9pp3a9"
task: "T32"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test go_to_line::tests::moves_the_caret: in a 50-row file, confirming 12 puts a single caret at (11,0) with row 11 visible; 3:5 puts it at row 2, char column 4; 999 goes to the last row; 3:999 to the end of row 3"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test go_to_line::tests::status_and_invalid_queries: an empty query shows status Line R of 50 and no rows; abc and 0 show Type a line number between 1 and 50 and no rows, and enter leaves the picker open and the caret unmoved"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test go_to_line::tests::back_and_escape: after going from line 1 to line 40, nav.back returns the caret to line 1; escape dismisses without moving and refocuses the editor; with no active editor the command shows nothing"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Go to a line: a go_to_line.toggle picker that takes line or line:column and moves the active editor's caret there, recorded for back (Mac T451)

Filed on argon from Mac T451 (card ~/coordination/argon-cards/T451.json rev 1, Mac lead message ~/coordination/from-mac-lead-2026-10-12-argon-h.md). Mac task uid 01m4pz58hpg32jzg7tggk005ab. Title the host commit "... (Mac T451)".

## Problem

There is no way to jump to a line by number outside vim's `:N`. Browsing comes first in the plan ("Users will browse
code more than edit it", plan line 4; M3 Browse adds Cmd-P and the palettes, line 108), and a line number is what a
stack trace, a log, a review comment or a colleague gives you. The workbench's palettes (`Palette`,
`crates/fe-workbench/src/workbench.rs` :96) are quick open, language, toolbar, definitions and symbols; none takes
a line. No card on the board adds one (2026-10-11 scoping pass).

## Change

New module `crates/fe-workbench/src/go_to_line.rs` (`mod go_to_line;` in lib.rs): `GoToLine`, a `PickerDelegate`
shown as a new `Palette::GoToLine` (id `go_to_line`), the way `select_language` (workbench.rs :1089) shows the
language picker for the active editor.

- Command `go_to_line.toggle` ("Go to Line/Column…"), registered where `quick_open.toggle` is
  (`crates/fe-workbench/src/picker.rs` :35), in the workbench context. With no active editor it does nothing; with
  the picker showing it dismisses it.
- The query is `N` or `N:C` (1-based line, 1-based column in chars), with surrounding spaces and one leading `:`
  ignored. A valid query shows one row: `Go to line N` or `Go to line N, column C`. The status line is
  `Line R of T` (the caret's line and the line count) while the query is empty, and
  `Type a line number between 1 and T` for anything else that isn't valid; such a query shows no rows.
- Confirming moves the active editor to a single caret at that line and column, clamped: a line past the end goes
  to the last line, a column past the row's end to the row's end. The caret is scrolled into view
  (`EditorView::select_and_reveal` with an empty range), focus returns to the editor, and the place it left is
  recorded for `nav.back` first (`Workbench::push_history`, nav.rs :520), as go-to-definition does.
- Escape dismisses it and returns focus without moving the caret.

Not in this card: a key (a later card binds it on macOS and adds it to the Go menu); the read-only large-file viewer.

## Tests

New file `crates/fe-workbench/src/go_to_line_tests.rs`, declared from go_to_line.rs with `#[path]`, using the
workbench test helpers in `crate::tests`.
