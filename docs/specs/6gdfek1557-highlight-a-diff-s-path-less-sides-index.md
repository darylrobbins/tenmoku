---
task_uid: "01m559wr244emdgd6gdfek1557"
task: "T794"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test fe-gpui diff_view::tests::language_tests::the_left_side_takes_the_files_language passes: a DiffView named a.rs over a path-less left side and a right model at a.rs highlights both sides as rust (EditorView::syntax's language name), after run_until_parked."
    required: true
    manual: false
  - id: "C2"
    text: "Test fe-gpui diff_view::tests::language_tests::both_sides_of_a_commit_diff_take_the_files_language passes: with both sides path-less (a commit's diff, HEAD ↔ Index), a DiffView named a.rs highlights both as rust, and one named regatta.toml both as toml."
    required: true
    manual: false
  - id: "C3"
    text: "Test fe-gpui diff_view::tests::language_tests::a_left_side_whose_text_lands_later_keeps_the_language passes: a path-less left side created empty, whose whole text is then replaced by a buffer edit as git_diff's set_side_text does, is still rust and its syntax tree is parsed at the buffer's new version."
    required: true
    manual: false
  - id: "C4"
    text: "Test fe-gpui diff_view::tests::language_tests::a_side_with_a_path_keeps_its_own_language passes: a right model at b.py in a DiffView named a.rs stays python."
    required: true
    manual: false
  - id: "C5"
    text: "No file outside the scope changes; the existing diff_view tests (diff_view::tests, diff_view::diff_view_inline_tests, diff_view::diff_view_compare_tests) pass unchanged."
    required: true
    manual: false
---

# Highlight a diff's path-less sides (Index, HEAD, a commit's) in the compared file's language, as VS Code's diff editor does

## Problem

The user, from the running app (2026-10-17, main 92c57c57): "In diff view the original isn't formatted."
Their screenshot shows `regatta.toml (Index ↔ Working Tree)`: the right (Working Tree) side is
highlighted as TOML, the left (Index) side is plain text. VS Code's diff editor highlights the original
side of every diff in the file's language.

Cause: an `EditorView` picks its language once, in `EditorView::new` → `detect_language`
(crates/fe-gpui/src/pane_item.rs:596), from `fe_syntax::detect_language_name(model.path(), <text start>)`.
Every git side is built path-less, `EditorModel::new(buffer, None)`: `fixed_side` in
crates/fe-workbench/src/git_diff.rs:537, git_history.rs:177 and git_blame.rs:1158, and the file compare's
left at git_diff.rs:1358. Its text also lands later (`set_side_text`, git_diff.rs:584), so at creation
it is empty and neither the path nor a first line names a language. So:

- HEAD ↔ Working Tree and Index ↔ Working Tree: the left side is plain, the right (a split of the
  file's editor, with its path) is highlighted.
- HEAD ↔ Index (a staged row) and a commit's diff from File History or blame: both sides are plain.
- Compare with file: the left (the other file) is plain.

`DiffView::new` (crates/fe-gpui/src/diff_view.rs:196) already knows the compared file's name
(`file_name`), so the fix belongs there and needs no host change.

## Change

In `DiffView::new`, after making each side's `EditorView`, give every side whose model has no path a
language with `EditorView::set_language`: the one `fe_syntax::detect_language_name` names for
`Path::new(file_name)` and that side's text start; if that names none, the right side's language
(detected the same way from the right model's path, else `file_name`, and its text start). A side with
a path keeps what `EditorView::new` detected. The language sticks when the host later replaces the
side's text (`set_language` sticks until the next call; nothing re-detects a diff side).

A file compare whose two files differ in language (a.rs against b.py) gets the right file's language on
its left until the follow-up card for `git_diff.rs` sets the left from its own file; that card depends
on this one.

Tests in a new crates/fe-gpui/src/diff_view_language_tests.rs, declared in
crates/fe-gpui/src/diff_view_tests.rs by `#[path = "diff_view_language_tests.rs"] mod language_tests;`,
using only base API (`DiffView::new`, `DiffSide`, `EditorView::syntax`), through `diff_view_of`'s
headless window: read a side's language as `view.syntax().map(|s| s.language().name())` after
`run_until_parked`.

Not in this card: the inline mode's removed-row blocks (`diff_view/inline.rs`) stay unhighlighted.
