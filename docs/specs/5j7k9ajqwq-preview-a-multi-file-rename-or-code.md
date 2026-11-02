---
task_uid: "01m6c8pknjysh7wn5j7k9ajqwq"
task: "T281"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "A rename whose edits touch two files (fake server) opens the preview in the AI Changes panel, headed 'Rename foo to bar: 2 files, N edits', and changes no buffer or file before Apply. A rename inside the source file only applies at once, as before"
    required: true
    manual: false
  - id: "C2"
    text: "After Reject File on one file and a hunk Reject in another file's review tab, Apply makes exactly the kept edits: each open buffer in one transaction (one cmd-z restores it whole), closed files written to disk; the rename's caret move runs after Apply"
    required: true
    manual: false
  - id: "C3"
    text: "Cancel changes nothing, runs no code-action command, closes the review tabs and leaves 'No pending changes'. Editing a previewed open buffer before Apply refuses Apply with the existing stale message, changes nothing and keeps the preview"
    required: true
    manual: false
  - id: "C4"
    text: "A multi-file code action (fake server, resolved edit) previews the same way and runs its command only after Apply"
    required: true
    manual: false
  - id: "C5"
    text: "New tests are in crates/fe-workbench/src/refactor_preview_view_tests.rs; the existing lsp_rename, lsp_code_actions and ai_changes_panel tests pass with only the multi-file confirm changed; clippy -D warnings is clean"
    required: true
    manual: false
---

# Preview a multi-file rename or code action in the AI Changes panel, keep or leave out files and hunks, then Apply (one undo step per buffer) or Cancel (Mac T1284)

## Problem

A multi-file LSP rename or code action applies without a preview (see the model card, T280 (Mac T1283), for the evidence:
`crates/fe-workbench/src/lsp_rename.rs` and `crates/fe-workbench/src/lsp_code_actions.rs` call
`Workbench::apply_user_file_edits` directly). The user (2026-11-01) asked for a multi-file preview diff with
per-file and per-hunk accept/reject before applying, reusing the proposal diff UI and the changeset view
(`crates/fe-workbench/src/ai_changes_panel.rs`, the editor's proposal blocks in `crates/fe-gpui/src/element/proposal.rs`)
rather than a new diff surface. T280 (Mac T1283) adds the headless `RefactorPreview`; this card shows it and wires it in.

## Change

- **Wiring.** In `lsp_rename.rs` (the picker's confirm) and `lsp_code_actions.rs` (`apply_alone` and the picker's
  `apply`), once the server's edits are decoded and the source buffer is unchanged, call a new
  `Workbench::preview_or_apply(title, source: BufferId, edits, on_applied, window, cx)` in
  `crates/fe-workbench/src/refactor_preview_view.rs` (declared from `ai_changes_panel.rs` with `#[path]`, so
  `lib.rs` and `workbench.rs` stay untouched). When `RefactorPreview::needs_preview` is false it applies at once as
  today. Otherwise it builds the preview (open buffers' text from their editors, closed files through
  `loader::read`) and shows it. The rename's caret placement and a code action's follow-up command
  (`actioner.run`) run only after Apply succeeds, through `on_applied`; Cancel runs neither. Titles:
  `Rename foo to bar` and the code action's title.
- **The panel.** The AI Changes panel shows the preview in place of a changeset, as a new turn's changeset
  replaces the last today (that changeset's proposals stay on their editors). Its header reads
  `<title>: N files, M edits` and has **Apply** and **Cancel** instead of Accept All, Reject All and Revert Turn;
  each file row keeps its counts and its Accept File and Reject File buttons, which mark the file kept or left out.
  A whole-only preview (file operations) hides the per-file buttons and lists the operations as rows.
- **Per-hunk.** Clicking a row opens that file's scratch copy as a read-only review tab, as new-file previews open
  today (`AiChanges::previews`), showing the preview's `ProposedChange` with the existing proposal blocks; their
  Accept and Reject mark hunks kept or left out, and the row's counts follow the editor's `ProposalEvent`s.
- **Apply** calls `Workbench::apply_user_file_edits(preview.selected(), cx)`: one user transaction per open buffer
  (one cmd-z takes a file's whole refactor back), closed files written as today. A failure (a previewed buffer
  edited since: the existing stale refusal) changes nothing, says why with the existing info prompt, and keeps the
  preview open. **Cancel** applies nothing. Both close the review tabs (`Pane::discard_buffers`) and leave the
  panel with `No pending changes`.
- No new command IDs or keys in this card (Apply and Cancel are buttons); palette commands and keys are a
  follow-up. Server-pushed `workspace/applyEdit`, formatting and the project tree's willRenameFiles edits keep
  applying directly.

Existing tests change only where a multi-file rename or code action now stops at the preview
(`renames_across_files` in `lsp_rename_tests.rs` confirms through Apply). New tests go in
`crates/fe-workbench/src/refactor_preview_view_tests.rs`, using the fake server (`fe_lsp::fake`) as
`lsp_rename_tests.rs` does.

## Implementation notes

- **State.** `Workbench::ai_changes` stays an `AiChanges` (so `workbench.rs` is untouched): a preview is an
  `AiChanges` over an empty `Origin::Lsp` changeset with `refactor: Some(Review)`. `sync_ai_changes`,
  `decide_ai_file`, `open_ai_file` and the Accept All / Reject All commands branch to the preview while it shows;
  Revert Turn finds nothing accepted and does nothing. `set_changeset` and a new preview close both kinds of review
  tab (`discard_ai_reviews`).
- **Review tabs are copies.** `EditorView::set_proposals` needs the proposal's own buffer, and the preview must keep
  its scratch buffers (a closed tab would otherwise take a file's decisions with it). So a row opens a tab on a
  copy of the scratch text, with a proposal made from the preview's undecided hunks, each paired with the preview
  hunk it stands for. The copy has no file (titled `<name> (preview)`, highlighted as the file), so no server opens
  it, and it's unsavable (`EditorView::set_unsavable`, new in fe-gpui: Save, Save As and `write_copy` fail at the
  editor, the save command does nothing, and `workbench.save_as` shows no panel for it). It's kept clean
  (`Buffer::did_save` after an accept), so nothing asks to save it, backs it up or restores it with the layout. It's
  read-only, but takes its hunks' Accepts, buttons and `proposal.accept_*` commands alike
  (`EditorView::set_accepts_when_read_only`, also new; a split copies both flags), when the preview is decidable. The
  copy's `ProposalEvent`s decide the paired preview hunk; Accept File, Reject File and the commands are mirrored
  into every view of it, splits included, on the next sync. A whole-only preview's tab offers Reject only, and
  refuses it, puts the hunk back and says why.
- **Header counts** are what Apply would make now (`preview.selected()`): distinct paths, and text edits plus one
  per file operation. A preview's rows show however much is decided (an operation-only edit has no hunks).
- **Apply** first checks, before anything is made, every refusal applying would make of a target once it got
  there: each open buffer the kept edit touches (a dependent edit's either way) is open, at the server's version
  and still at its file (`has changed since`, `has been saved under another name since`); each file it edits closed
  isn't opened in any window (`has been opened since`), where it is now and where the edit's renames take it; and
  each closed file the preview read still reads the same (`has changed since`). A refusal makes nothing and keeps
  the preview and its tabs. A failure after that, once part of
  the edit was made (a closed file that can't be written, a later operation refused), keeps what was made, as
  `apply_file_edits` always has, says so and ends the preview, as applying it again would make that part twice.
  It's detected exactly: each targeted buffer's version, and each named path's entry and the file it leads to
  (inode, mtime, length), before and after.
- **One undo per buffer** across runs: `apply_file_edits` now makes a buffer's later runs of the same edit (past a
  file operation) with `Buffer::edit_in` its first run's transaction (scope: `workspace_edit.rs`).
- **Preview text.** An open buffer's text is its editor's, refused (`<path> has changed since`) if it's not at the
  server's version; a closed file is read with `loader::read`, and kept for Apply's check.
- **Code actions** check `RefactorPreview::needs_preview` themselves, so a single-file action keeps today's flow
  (the picker waits on the command). Previewed, the picker closes first (the picker's path defers the preview so
  the panel keeps focus), and Apply runs the command through the action's own `CodeActioner`; if its document has
  closed meanwhile, fe-lsp refuses it and that's said. The rename's caret placement is computed from the edits Apply
  made, so a left-out hunk before the symbol doesn't misplace it.
