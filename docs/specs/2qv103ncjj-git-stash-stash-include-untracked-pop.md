---
task_uid: "01m5tf0mr78bagfy2qv103ncjj"
task: "T349"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-git stash::push_list_pop: in a temp repo with a modified tracked file and an untracked one, stash_push(Some('wip'), false) removes the tracked change and leaves the untracked file, stash_list shows one entry 'wip', stash_pop(0) restores the change and empties the list"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git stash::include_untracked_and_apply: stash_push(.., true) also removes the untracked file; stash_apply(0) restores both and keeps the entry; a conflicting pop returns an error naming the conflict and the entry remains"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git stash::nothing_to_stash: on a clean tree stash_push returns the 'No local changes to save' error and creates no entry"
    required: true
    manual: false
  - id: "C4"
    text: "git_stash_tests::commands: git.stash with a message, git.stash_pop through the picker and git.stash_apply each run against a temp repo, refresh the Changes panel rows, and report errors in the status line; the four ids and titles appear in git_commands_tests and the catalog tests"
    required: true
    manual: false
  - id: "C5"
    text: "Stash \u0026 Checkout tests (git_branch_tests, switch_ref) pass unedited; cargo nextest run -p fe-git -p fe-workbench and clippy pass"
    required: true
    manual: false
---

# Git: Stash, Stash (Include Untracked), Pop Stash… and Apply Stash… commands, as VS Code's git.stash* (Mac T1162)

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Git: Stash, Git: Stash (Include Untracked), Git: Pop Stash… and Git: Apply Stash… commands (git.stash, git.stashIncludeUntracked, git.stashPop, git.stashApply) are missing. fe-git already stashes internally for Stash & Checkout (`crates/fe-git/src/branch.rs:150`, `git stash push --include-untracked -m`), and the branch switch tells the user to pop it, but there is no way to pop or apply it, or to stash on demand.

## Current state
- `Repo::stash` is private to `branch.rs:150` and runs the CLI through `run_raw(&self.workdir, Access::Write, &args, None, &[])`; `failed(&output)` builds the error.
- Git commands in the workbench are a `GitCommand` enum with titles in `crates/fe-workbench/src/git_commands.rs:49-115` and typed methods in `workbench_commands.rs` (`git.diff_head`, `diff.compare_file` near :140-160); the branch picker is `crates/fe-workbench/src/git_branch.rs`. Titles are asserted in `git_commands_tests.rs:118` and ids in the catalog tests.
- fe-git integration tests build temp repos with the git CLI (`crates/fe-git/tests/switch_ref.rs`).

## Change
- fe-git: new `crates/fe-git/src/stash.rs` (+ `mod` in `lib.rs`): `Repo::stash_push(message: Option<&str>, include_untracked: bool)`, `stash_list() -> Vec<StashEntry { index, message, branch }>`, `stash_pop(index)`, `stash_apply(index)`; errors as `GitError` with git's message. A pop or apply that conflicts leaves the stash in place (git's behaviour) and reports the conflict; a push with nothing to stash reports "No local changes to save".
- Workbench: commands `git.stash` ("Git: Stash", asks for an optional message in an input), `git.stash_include_untracked` ("Git: Stash (Include Untracked)"), `git.stash_pop` ("Git: Pop Stash…") and `git.stash_apply` ("Git: Apply Stash…"), the last two through a picker of stashes (latest first, `stash@{n}: message`). Refresh the Changes panel and open editors after each (disk sync picks the files up; the panel is told as `git_commit.rs` does). Messages in the status/toast the way other git commands report. No default keys, no menu entries.

## Tests
New `crates/fe-git/tests/stash.rs` (temp repos) and `crates/fe-workbench/src/git_stash_tests.rs`; update the title and id assertions.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y65).

## Implementation notes
- **Errors.** `stash_push`, `stash_pop` and `stash_apply` return `fe_git::StashError` rather than a bare `GitError`, as `commit` (`CommitError`) and `switch` (`SwitchError`) do: `NothingToStash` ("No local changes to save"), `Conflict { conflicts }` (git's `CONFLICT (…)` lines, naming each file; the entry is kept), `Failed { output }` (all git printed) and `Git(GitError)`. `stash_list` returns `GitError`. Every message is git's own.
- **`StashEntry::id`.** Entries also carry their commit id. The picker's pop or apply checks that the entry is still at its index before running, so a stash pushed meanwhile (from a terminal) can't make it pop another entry; it then says "The stash changed since it was listed; pick again".
- **`cli.rs` (respecced in).** `run_raw` passed `--literal-pathspecs` to every git call. With it, `git stash push --include-untracked` stashes the untracked files but leaves them in the tree, because its internal clean uses a `:/` pathspec that is then taken literally. Stash & Checkout had the same bug, and with Pop Stash it would show: popping fails with "already exists". `stash` calls (never given paths) now run without the flag. `tests/stash.rs::stash_and_switch_then_pop` covers Stash & Checkout's stash round trip.
- **Reporting.** No ex forms, so nothing is typed on a vim line: answers and errors are toasts, as the branch commands' are. Nothing to stash and no stashes are info toasts, a conflict a warning, and git failing an error with its output.
- **Pickers.** A modal of their own (`git_stash`), shown as the encoding picker shows its own, in place of any palette. The message picker's one row always stashes; an empty message uses git's.
- **Known limit: unsaved editors.** VS Code asks to save dirty files before stashing; this doesn't. Their edits stay in the buffers, not the stash, and disk sync treats the files git changed under them as it does after a branch switch.
