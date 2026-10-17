---
task_uid: "01m4rhrgyg9xajv0rf1xpet7y9"
task: "T542"
rev: 6
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test save_reason_tests::delay_skips_edits: with format_on_save on and a save hook that would edit, save_with(AutoDelay) writes the buffer text unchanged and the hook isn't run; save_with(Explicit) runs it; save_with(AutoFocus) runs it unless the editor's vim is in insert mode"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test save_reason_tests::insert_session_kept: with vim in insert mode after typing 'abc', save_with(AutoFocus) saves, then Esc and '.' repeat the whole insert and one 'u' undoes it as one step"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test save_reason_tests::auto_never_prompts: save_with(AutoDelay) on a file changed on disk shows no prompt and resolves with an error that is SaveError::ChangedOnDisk; the same file with save (Explicit) still prompts as T432 does"
    required: true
    manual: false
  - id: "C4"
    text: "T479's and T481's tests pass unchanged; cargo test -p fe-gpui -p fe-workbench passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Pass a save reason through EditorView::save so auto-saves after a delay, or during a vim insert, skip format-on-save and save-time trims and never prompt

## Problem

Auto-save (user, 2026-10-12) must respect format-on-save (T479) the way VS Code does: an `afterDelay` auto-save runs
no save participants (no format, no trimming), or the file reformats under the user's fingers while they type. The
vim-mode requirement (coordinator, same day) adds: no save-time edit while a vim insert session is open, so `.` and
the insert's single undo step survive. T479 (format on save, `crates/fe-gpui/src/pane_item.rs` save hook and
`crates/fe-workbench/src/lsp_format.rs`) and T481 (trim and final newline on save, `crates/fe-workbench/src/pane.rs`)
add save-time edits with no notion of why the save happened.

## Change

- `crates/fe-gpui/src/pane_item.rs`: `EditorView::save_with(reason: fe_editor::auto_save::SaveReason, window, cx) ->
  Task<…>`; `EditorView::save` (cmd-s, `:w`, close) is `save_with(SaveReason::Explicit, …)`. The save hooks T479
  added receive the reason, and run their edits only when `reason.edits_allowed(vim_insert)` (the auto-save policy
  card), `vim_insert` being whether this editor's vim is in insert mode.
- `crates/fe-workbench/src/lsp_format.rs` (T479's format-on-save hook) and `crates/fe-workbench/src/pane.rs` (T481's
  trim and final-newline hook) check the reason and skip when edits aren't allowed; the file is still saved as it is.
- An auto reason never prompts: T432's changed-on-disk prompt and failure prompt are for `Explicit` only; an auto save
  that fails resolves its task with the error (`SaveError::ChangedOnDisk` kept distinguishable) and shows nothing.

## Tests

New file `crates/fe-gpui/src/save_reason_tests.rs`, declared from pane_item.rs (`#[cfg(test)] #[path =
"save_reason_tests.rs"] mod save_reason_tests;`).
