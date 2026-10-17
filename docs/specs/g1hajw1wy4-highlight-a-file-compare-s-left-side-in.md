---
task_uid: "01m559xpzysa7bdpg1hajw1wy4"
task: "T797"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test fe-workbench workbench::git_diff::git_diff_tests::language_tests::a_compared_file_is_highlighted_in_its_own_language passes: in a repository with a.rs and b.py, a.rs open, :diffsplit b.py gives a diff view whose left side is python and right side rust (EditorView::syntax's language name), after the read lands."
    required: true
    manual: false
  - id: "C2"
    text: "Test fe-workbench workbench::git_diff::git_diff_tests::language_tests::git_sides_take_the_files_language passes: with a.toml changed and staged then changed again, git.diff_index and git.diff_head each show both sides as toml."
    required: true
    manual: false
  - id: "C3"
    text: "The compare's left model stays path-less, and the existing git_diff tests (workbench::git_diff::git_diff_tests) pass."
    required: true
    manual: false
---

# Highlight a file compare's left side in its own file's language, not the right file's

## Problem

Follow-up to the card that highlights a diff's path-less sides in the compared file's language
(the user, 2026-10-17: "In diff view the original isn't formatted."). A file compare
(`diff.compare_file`, `:diffsplit {file}`) shows the active file on the right and another file on the
left. Its left model is built path-less in `Workbench`'s compare (crates/fe-workbench/src/git_diff.rs:1358,
`EditorModel::new(buffer, None)`), so at 92c57c57 the left is plain text; once the depended-on card lands
it takes the right file's language, which is wrong when the two differ (a.rs against b.py highlights
Python as Rust). VS Code highlights each side of a file compare in its own file's language.

## Change

In the compare in crates/fe-workbench/src/git_diff.rs (the block around line 1355 that builds `left`
from `other`'s buffer), once `new_diff` has made the view, set the left side's language with
`EditorView::set_language` to what `fe_syntax::detect_language_name(Some(&other), <its text start>)`
names, or no language when it names none. Nothing else in the compare changes: the left model stays
path-less (a path would make the workbench treat it as an editor of `other`).

Tests in a new crates/fe-workbench/src/git_diff_language_tests.rs, declared in
crates/fe-workbench/src/git_diff_tests.rs by `#[path = "git_diff_language_tests.rs"] mod language_tests;`,
using only base API and the parent's helpers (`repo`, `window`, `ex`, `diff`, `sides`): a repository with
`a.rs` and `b.py`, `a.rs` open, `:diffsplit b.py`; read a side's language as
`view.syntax().map(|s| s.language().name())`.
