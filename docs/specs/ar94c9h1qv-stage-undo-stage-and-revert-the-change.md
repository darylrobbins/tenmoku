---
task_uid: "01m4xqgkdkmajwrxar94c9h1qv"
task: "T125"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_stage_hunk_tests::stage_hunk: with three unstaged changes in a.rs, git.stage_hunk with the caret in the second stages only it (git diff --cached shows that hunk alone), the buffer and disk unchanged; an unsaved edit at the caret is staged as typed; on an unchanged row the message is 'No change to stage at this line'"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test git_stage_hunk_tests::visual_range: from vim visual line mode over 2 of a hunk's 4 added rows, \":'\u003c,'\u003eGitsigns stage_hunk\" stages exactly those 2 rows (index_text has them, not the other 2)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test git_stage_hunk_tests::undo_stage: ':Gitsigns undo_stage_hunk' after a stage restores the previous index text; after an outside 'git add' it refuses with 'Index changed since; not undone'"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test git_stage_hunk_tests::revert: git.revert_hunk (':Gitsigns reset_hunk') at a changed row puts HEAD's rows back with no prompt and one 'u' restores the change; a range over parts of two changes reverts both whole"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test git_stage_hunk_tests::diff_view_put: in an Index diff, 'dp' and ':diffput' on a right-side hunk stage it and the left side then shows it; cargo test -p fe-workbench passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Stage, undo-stage and revert the change at the caret or a visual line range, from editors, the diff view and the palette (Mac T553)

## Problem

The user (2026-10-12) asked to stage and unstage individual hunks from the diff view and the gutter, and to revert a
hunk. The vim-mode requirement (coordinator, same day): reachable from normal mode (`:Gitsigns stage_hunk`, etc.), and
a visual line range stages just those lines if the engine supports it (it does: `TextDiff::apply_rows`).

## Change

In `crates/fe-workbench/src/git_stage.rs` (the hunk commands' stubs) and `crates/fe-workbench/src/git_diff.rs`
(`dp` into the index):

- `git.stage_hunk` (`:Gitsigns stage_hunk`, palette "Git: Stage Change"): in a tab editor, diff the file's index text
  against the buffer's current text (`TextDiff::new(index, buffer)`; VS Code stages from the editor's text, saved or
  not). With an empty selection: the hunks meeting the caret's row (`apply`). With a selection or an ex range
  (`:'<,'>Gitsigns stage_hunk` from visual mode): just those rows (`apply_rows`), as gitsigns stages a partial hunk.
  Writes the result with `Repo::set_index_text` on the background executor, then refreshes; a failure shows an error
  toast (T517) with git's output. Nothing to stage there:
  message `No change to stage at this line`.
- `git.unstage_hunk` (`:Gitsigns undo_stage_hunk`): gitsigns' undo: restores the index text from before the last
  `stage_hunk` of this file in this session (a per-file stack), refused with `Index changed since; not undone` if
  the index text differs from what that stage wrote.
- `git.revert_hunk` (`:Gitsigns reset_hunk`, "Git: Revert Change"): `EditorModel::revert_changes(rows)` against the
  editor's diff base (HEAD, what the change bar shows); an ex or visual range reverts every whole change it meets (the
  engine reverts whole hunks only; stated). An undoable edit, so no confirmation (VS Code's Revert Change asks none);
  whole-file discard keeps its confirmation.
- In a DiffView over the Index, on the right side: `git.stage_hunk`, `dp` and `:diffput` stage the hunks in those rows
  (fugitive's `:Gdiffsplit` + `dp` puts into the index); the left side then shows the new index text. Over HEAD,
  `git.stage_hunk` on the right side stages as in a tab.
- Gutter: the commands act on the caret's row, so clicking a change bar (caret moves to it) then running them stages
  or reverts it. No inline gutter buttons (fastedit has no gutter widgets or context menus yet; stated).

## Tests

New file `crates/fe-workbench/src/git_stage_hunk_tests.rs` (declared `#[cfg(test)]` from git_stage.rs).

## Implementation notes

- Staging reads the index's text as its turn in the git work queue comes (after any stage queued before it), and
  diffs the editor's text as it was when asked. A file not in the index answers `a.rs is not in the index` (gitsigns
  doesn't stage hunks of an untracked file either; `git.stage_file` adds it whole).
- Which rows: an ex range (`:'<,'>`, `:5`) stages just those rows; a selection does too, in vim only a visual one, so
  the palette's "Git: Stage Change" in visual line mode stages the selected rows. Otherwise the changes meeting the
  caret's row stage whole, a deletion meeting the row it sits above or the row above that, as `do` and `revert_changes`
  count it.
- Review (F1–F5): staging works on index entries read with the git CLI, as `git_blame` does: the blob's bytes (refused
  unless UTF-8, as is a buffer in another encoding), and the entry (mode and blob id) before and after
  `set_index_text`, so undo checks and restores the exact blob with `update-index --cacheinfo`, not reapplying clean
  filters; a missing entry counts as changed. Through a symlink it stages the file the link resolves to, the buffer's
  text, and refuses an entry that isn't a regular file. In vim, a visual selection's rows are its anchor's through
  its cursor's, not the model selection's (linewise ones exclude the last row's line break).
- The undo stack is per file (its real path) for the session, shared with the queued git work so a stage and its
  undo run in the order asked. An undo refused because the index changed drops that entry: it can never apply again.
  With nothing to undo it answers `No staged change to undo`.
- In a diff view, `git.stage_hunk` stages from the working side; from a git side (the left, or a staged diff's) it
  answers `Stage from the Working Tree side`. `dp`/`:diffput` stage only on the working side of an Index diff; over
  HEAD they still answer `E793`. `git.revert_hunk` in a diff side reverts to its left side's text, which is that
  side's diff base; with no change there it answers `No change to revert at this line`.
- `git_commands_tests::ex_routes` asserted the hunk stubs' `Not supported yet` answers, so this card changes its two
  answers (scope widened for it once T127, which also edits it, landed).
