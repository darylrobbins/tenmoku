---
task_uid: "01m548nn14pxws6wbzajsehmt4"
task: "T746"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "auto_save::auto_save_tests::split_insert::focus_or_window_save_mid_insert_in_another_split_runs_no_save_fixes passes: with one split in normal mode and another in vim insert, an on_focus_change save on focus change and on window deactivation leaves trailing blanks the insert typed, in the buffer and on disk, and the inserting split stays in insert"
    required: true
    manual: false
  - id: "C2"
    text: "The existing auto_save::auto_save_tests tests pass unchanged; auto_save_tests.rs gains only the #[path] mod line"
    required: true
    manual: false
  - id: "C3"
    text: "The workspace gate (nextest and clippy -D warnings) passes and only the three scoped files change"
    required: true
    manual: false
---

# Run no save fixes or format hook in a focus or window auto-save while another split of the buffer is in a vim insert (T543 review F7)

## Problem

T543 review F7 (medium, review 5g0vnf, `crates/fe-workbench/src/auto_save.rs:430`),
not carded until now:

> With focus/window auto-save, a buffer's first split can be in normal mode while
> another split is inserting. Saving through shown[0] lets save_with run formatting
> and whitespace fixes against the shared buffer mid-insert, because its eligibility
> check sees only that editor's Vim mode.

`Workbench::auto_save_buffer` blocks a save mid-insert only for `AfterDelay` and
`OnInsertLeave` (`mid_insert`), which is right: VS Code saves on focus and window
change whatever the mode. But it then always saves through `shown[0]`, and
`EditorView::save_with` decides whether the save hook (LSP format) and `SaveFixes`
run from *that* editor's vim mode (`edits_allowed` → `typing_mode()`). With split 0
in normal mode and split 1 in insert, the fixes edit the shared buffer under the
insert.

Reproduced at c092189b: vim on, `on_focus_change`, `a.txt` = `one\n` split with
`cmd-\`, `SaveFixes { trim_trailing_whitespace: true }` on both editors; in the
right split `shift-a space space` (still in insert), then focus the left split.
The focus-out auto-save trims the blanks the insert just typed: the buffer is
`one\n`, expected `one  \n` (assertion `left: "one\n" right: "one  \n"`).

## Change

In `Workbench::auto_save_buffer` (`crates/fe-workbench/src/auto_save.rs`), save
through a split that is in vim insert or replace mode when there is one
(`insert_mode(editor) == Some(true)`), else `shown[0]` as now. `save_with` then
refuses the hook and fixes for the whole save, as it does when the saving editor
itself is inserting, and the text is saved as it is. Nothing in fe-gpui changes.
(A scratch implementation of exactly this passed the new test and the existing 13
`auto_save::auto_save_tests` tests.)

Regression test in a new file `crates/fe-workbench/src/auto_save_split_insert_tests.rs`,
declared from the existing `crates/fe-workbench/src/auto_save_tests.rs` by
`#[path = "auto_save_split_insert_tests.rs"] mod split_insert;` (it reuses that
file's helpers through `use super::*;`). Test
`focus_or_window_save_mid_insert_in_another_split_runs_no_save_fixes`: the repro
above, asserting the right split is still in `insert`, the buffer text and the file
on disk are both `one  \n`; then, still inserting in the right split, type more
spaces in it (refocusing it), deactivate the window while it is focused and in
insert, and assert the buffer and the file on disk keep every typed space (the
window-change save, also through `shown[0]` at base, runs no fixes either). This
test was run in a scratch worktree: it fails at c092189b on the first assertion
and passes with the change above.
