---
task_uid: "01m4xqgkp4hw4q08gfhmxsw3m9"
task: "T126"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_commit_tests::commits: typing 'Fix it' in the box and running git.commit_box.commit commits the staged change with that message (git log), clears the box and refreshes the panel; a first row of 55 characters shows the '55/50' hint after it, of 80 in the Warning tint"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test git_commit_tests::amend: checking Amend into an empty box fills it with HEAD's message; committing amends HEAD (same parent); unchecking an untouched box restores it empty"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test git_commit_tests::failures: with nothing staged the stage-all prompt appears and 'Stage All and Commit' stages and commits; a failing pre-commit hook printing 'lint failed' shows a prompt whose detail contains 'lint failed' and the message stays in the box"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test git_commit_tests::vim_box: with vim on, git.commit focuses the empty box in insert mode; after typing a message, Esc then ':wq' commits, 'ZZ' commits, ':q!' and 'ZQ' clear it and focus the last editor, ':q' on a non-empty box answers E37; 'cc' in the focused panel focuses the box and 'ca' with Amend checked"
    required: true
    manual: false
  - id: "C5"
    text: "The box never appears in tabs, auto-save, hot exit or recent files; cargo test -p fe-workbench passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Commit from a message box in the Changes panel: subject-length hint, amend, the stage-all question, hook failures shown, and vim's :wq, ZZ and :q! in the box (Mac T554)

## Problem

The user (2026-10-12): "a commit message box in the Changes panel (first-line length hint), commit staged, amend
toggle; surface hook failures/errors." The vim-mode requirement (coordinator, same day): if the box is an editor, it
edits in vim mode, `:wq`/`ZZ` commit and `:q!` aborts. `Repo::commit` exists (the commit card); the panel
(`crates/fe-workbench/src/git_panel.rs`) has no box.

## Change

In `crates/fe-workbench/src/git_panel.rs` and `crates/fe-workbench/src/git_commit.rs` (replacing its stubs):

- The box is an `EditorView` (Zed's commit editor; chosen over a text input so vim mode applies) at the top of the
  panel over a path-less buffer: soft wrap, 1 to 10 rows tall as the text grows, placeholder `Message (⌘⏎ to commit on
  "<branch>")` (`ctrl-enter` on Linux; VS Code's wording). It isn't a tab: no auto-save, hot exit or recent files.
  Registered with `Workbench::own_editor(…, Owner::CommitBox)`.
- Subject hint (VS Code's `git.inputValidationSubjectLength` 50, body 72): once row 1 is over 50 characters an
  `AfterLine` decoration on it shows `<n>/50`, `Muted`, `Warning` tint over 72; body rows (after the first blank row)
  over 72 show `<n>/72` in `Warning`.
- Below the box: an "Amend" checkbox and a "Commit" button (label "Amend" when checked), disabled while the message is
  blank and amend is off. `git.commit_box.commit` commits (the keys card binds `cmd-enter`/`ctrl-enter` in the box).
  Checking Amend into an empty box fills it with `Repo::head_message()`; unchecking restores the box if untouched.
- Committing runs `Repo::commit` on the background executor, the button showing "Committing…" disabled. `NothingStaged`
  (VS Code's smart-commit prompt): prompt "There are no staged changes to commit." detail "Would you like to stage all
  your changes and commit them directly?" ["Stage All and Commit", "Cancel"]. `HookFailed`: prompt Warning "Git:
  <hook> hook failed" with the hook's output as detail (first 2000 characters, then `…`), the message kept.
  `Failed`: "Git: commit failed" with git's output. `EmptyMessage`: the box's placeholder turns to `Enter a commit
  message` in `Warning`. Success: box cleared, Amend unchecked, panel and branch refreshed.
- `git.commit` (`:Git commit`, `cc` in the panel with vim on, fugitive's) shows the panel and focuses the box;
  `git.commit_amend` (`:Git commit --amend`, `ca` in the panel) does so with Amend checked.
- Vim, through `git_commit::host_command` (the box follows `vim.enabled` like any editor; focused while empty it
  starts in insert mode, otherwise normal): `:wq`, `:x` and `ZZ` commit; `:w` keeps the message and says `Message
  kept; :wq commits`; `:q!` and `ZQ` clear the box (fugitive's abort) and return focus to the last editor; `:q` on a
  non-empty box answers `E37: No write since last change (add ! to override)`, on an empty one returns focus.

## Tests

New file `crates/fe-workbench/src/git_commit_tests.rs` (declared `#[cfg(test)]` from git_commit.rs), with hooks in
temp repos.

## Implementation notes

- The status bar's vim line follows the tabs' editor, so the panel hosts the box's own command line (a `TextInput`
  under the box in the `vim_line` key context, so Enter and Escape bind as in the bar's) and shows the box's vim
  messages there. The host commands the box's line and keys leave (`:wq`, `ZZ`, `ZQ`) are taken by the panel's observer
  and run through `Workbench::run_vim_host_command`, which routes them to `git_commit::host_command` as the box's owner.
- The panel asks the workbench for the box (via `window.defer`, as the workbench reads the dock's panels) when it's
  first drawn; `git.commit` makes it too. The workbench builds it in the pane's editor theme and registers it with
  `own_editor`. It's in no pane, so tabs, hot exit and recent files never see it (there's no auto-save yet).
- The placeholder is an `AfterLine` decoration on the empty first row, from the same provider as the length hints.
- The box is 1.5× its text's rows tall (rows + 3 past 6 rows), at most 10 rows: `fe-display` scrolls to keep the caret
  a third of the height, or 3 rows, from the bottom, which would scroll the first rows of a box sized exactly to its
  text. Past about 7 rows a long message scrolls within the 10.
- Review round 1: a commit's success empties the box only if it still holds the message committed (F1); every explicit
  open (`git.commit`, `cc`, `ca`, a focus from elsewhere) puts the box in insert mode if empty, else normal, and so does
  the Amend fill landing in a focused box, while the box's own `:` line handing focus back leaves the mode be (F3);
  `Enter a commit message` shows for every empty-message refusal (blanks, comments), until the box is next edited (F4).
- Review round 2: F2 (counters clipped at a wrapped row's right edge) is fixed in fe-gpui with the lead's approval
  (respec adding `view.rs` and `element.rs`): `EditorView::set_wrap_reserve(ems)` has soft wrap leave that much more of
  the text area free, 0 by default so no other editor changes; the box reserves 6 em (`HINT_ROOM`), its hint's gap and
  up to 7 characters. `view::wrap_reserve_tests` fills a wrapped row to the wrap width and checks the count's drawn
  right edge stays inside the text area with the reserve, and runs past it without. `Enter a commit message` for a
  refused message of blanks or comments shows under the box, as it's wider than that room; an empty box's still
  replaces the placeholder. F6: a refusal warns only if the box is still as submitted. F7: `:q`/`:q!` leaving the box
  ends the command line's refocus exemption.
- Review round 3: Stage All and Commit commits the request it was asked about (message, Amend, repository), keeping a
  draft typed since, and nothing once the root has changed (F8). Commits, stage-all with them, queue in `git_stage`'s
  mutation queue (`mutate`, now `pub(super)`, respec adding `git_stage.rs`), so a commit waits for staging asked before
  it and commits that index (F9).
- `git_stage.rs` carries T125's F6 (intent-to-add entries through hunk undo) and F7 (an outside conflict during undo),
  both non-blocking and about hunk staging; this card only makes `mutate` visible, and their regression tests belong in
  `git_stage_hunk_tests.rs`, outside its scope, so they stay with T125's follow-up.
- `git.commit` outside a repository answers `Not in a git repository`, as the other git features do; `:e`, `:wa` and
  `:qa` typed in the box run in the active editor, the diff commands answer `E99`.
- `git_commands_tests` asserted the stubs' answers (`:Git commit`, `:w` in a commit box), so it changes with them.
- Follow-up for fe-gpui: an editor without a gutter (line numbers, fold column), which takes a third of the panel's
  width here; the placeholder is clipped in a narrow panel.
