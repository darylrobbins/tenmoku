---
task_uid: "01m559x1f2p5bk5q6ejvfvn6xe"
task: "T795"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test fe-workbench workbench::git_commit::git_commit_tests::commit_box_tests::an_empty_box_is_three_rows_tall passes: the Changes panel's empty commit box is drawn 3 rows tall (box_height in 3.0..3.1), and 2 rows of text keep it 3 rows."
    required: true
    manual: false
  - id: "C2"
    text: "Test fe-workbench workbench::git_commit::git_commit_tests::commit_box_tests::the_box_wraps_at_its_own_width_with_no_gutter passes: the box's wrap width (model().display().wrap_width()) is at least the drawn commit-box width less (HINT_ROOM + 3) ems, the em measured as crates/fe-gpui/src/element.rs measures it."
    required: true
    manual: false
  - id: "C3"
    text: "Test fe-workbench workbench::git_commit::git_commit_tests::commits passes with its one-row assertion changed to 3 rows (3.0..3.1) and its 30-row assertion still 10 rows (10.0..10.1)."
    required: true
    manual: false
  - id: "C4"
    text: "EditorView::set_gutter(false) lays out through a new gutter-less entry point in crates/fe-display/src/layout.rs: no existing pub fn signature in layout.rs and no FrameInput field changes, and no file under crates/fe-display other than layout.rs changes."
    required: true
    manual: false
  - id: "C5"
    text: "Every editor other than the commit box keeps its gutter: the existing fe-display, fe-gpui and fe-workbench test suites pass."
    required: true
    manual: false
---

# Show the Changes panel's commit message box with no gutter, wrapped at its width and 3 to 10 rows tall, as VS Code's SCM input

## Problem

The user, from the running app (2026-10-17, main 92c57c57): "The message text is way too short to be
used." The Changes panel's commit message box is an `EditorView` with the full editor gutter: line number
"1", fold column and change lane (`fe_display::layout::gutter_width`, crates/fe-display/src/layout.rs:525,
9.75 em at least) eat the left of a box only a sidebar wide, so the placeholder
`Message (⌘⏎ to commit)` is cut off. And an empty or one-row box is 1.5 rows tall
(`box_rows`, crates/fe-workbench/src/git_panel.rs:1744; `render_commit_box` at :1634), with no room to
write a message. The box is made in `Workbench::attach_commit_box` (crates/fe-workbench/src/git_commit.rs:166)
over a path-less, soft-wrapped model; there is no way today to lay an editor out without its gutter:
`layout_frame_with_inlay` (layout.rs:593) always starts the text at `gutter_width`, and `wrap_width`
(layout.rs:533) always subtracts it.

VS Code's SCM input has no line numbers, no folding and no change decorations, soft-wraps at the panel's
width, and grows with its text between `scm.inputMinLineCount` (default 1) and `scm.inputMaxLineCount`
(default 10).

## Change

Decision (the lead's, on the user's report that one row is too short to use): the box is at least 3 rows
tall and at most 10, not VS Code's default minimum of 1.

1. fe-display (crates/fe-display/src/layout.rs): a way to lay out a frame with no gutter: the text starts
   half an em in from the left, and the frame has no line numbers, fold toggles, fold column or change
   lane (zero-width rects). Add it as a new entry point beside `layout_frame_with_inlay` and a
   gutter-aware wrap width beside `wrap_width`. Keep every existing signature and `FrameInput`'s fields as
   they are: fe-display's tests build `FrameInput` literally, and the regression run builds the changed
   test files against the base.
2. fe-gpui: `EditorView::set_gutter(&mut self, shown: bool, cx)` (crates/fe-gpui/src/view.rs), default
   shown; crates/fe-gpui/src/element.rs lays out, wraps and hit-tests through the gutter-less path when
   it's off, and paints no line numbers, chevrons, change bars or diagnostic marks.
3. fe-workbench: `attach_commit_box` turns the box's gutter off. `box_rows` floors at 3 rows (it keeps
   its growth rule above that and its cap at `BOX_ROWS`, 10). Update the module docs in git_panel.rs
   ("grows from 1 to 10 rows") and the `commits` test's one-row height assertion
   (crates/fe-workbench/src/git_commit_tests.rs, `(1.5..1.6)`) to 3 rows.

The placeholder and length hints stay after-line decorations; with the gutter gone the placeholder gets
its 9.75 em back. Making a placeholder longer than the box's width wrap is not part of this card.

Tests in a new crates/fe-workbench/src/git_commit_box_tests.rs, declared in
crates/fe-workbench/src/git_commit_tests.rs by `#[path = "git_commit_box_tests.rs"] mod commit_box_tests;`,
using only base API and the parent's helpers (`fixture`, `open`, `box_height`, `set_text`, `draw`):
- the empty box's drawn height (`box_height`) is 3 rows;
- the box's wrap width (`model().display().wrap_width()`) is at least the drawn box's width
  (`cx.debug_bounds("commit-box")`) less `(HINT_ROOM + 3)` ems, the em measured as element.rs measures it
  (`text_system.em_advance` of the theme's font and size). The gutter alone is 9.75 em, so this fails
  with it.

A screenshot golden isn't needed: the wrap width proves the gutter's room is gone, and the height test
proves the rows.

## Implementation notes

- fe-display: `layout_frame_without_gutter` (the same arguments as `layout_frame_with_inlay`, less
  `fold_candidates`) and `wrap_width_without_gutter(width, em)`, both over a private `lay_out_frame`
  that takes the gutter flag. Without a gutter the text starts `NO_GUTTER_LEFT_EMS` (0.5) in, and the
  wrap width leaves that padding and one em for the caret, not the gutter's two: the box is
  `HINT_ROOM` ems narrower already, and with two the C2 bound (`HINT_ROOM + 3` ems) would hold only
  when the box's width happens to round well.
- fe-gpui: with the gutter off, the element also drops the frame's gutter decorations (change bars and
  deletion wedges, so their hitboxes and clicks go too) and the diagnostics' gutter marks.
