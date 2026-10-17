---
task_uid: "01m4xqgna9tb2s4gqmzth53hen"
task: "T132"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test git_log_open_tests::expands: Enter on a commit lists its files under it (A, M, D, and R old → new for a rename) after a 'Loading…' row, and Enter again hides them; a merge lists the files that differ from its first parent; j/k and up/down move over commit and file rows"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test git_log_open_tests::opens: Enter (or o, or a click) on a modified file opens a read-only DiffView of the file at the first parent against the commit, titled '\u003cname\u003e (\u003cshort\u003e^ ↔ \u003cshort\u003e)' and focused; on a rename the left is the old path's text; an added file's left side and a deleted file's right side are empty; opening it again focuses the same tab"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench passes; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Expand a Git Log commit to its changed files with Enter and open a file's change at that commit in a DiffView (Mac T610)

## Problem

The Git Log panel (T609) lists commits with their graph, but a commit can't be opened: the user asked that
Enter open the commit's changed files and their diffs through the DiffView, as fugitive's commit buffer and VS
Code's Source Control Graph do (user, 2026-10-13: a repo-wide log with a graph after file history).

This card builds on `Repo::commit_files` and `Repo::text_at`/`Repo::parent` (fe-git, T608 and T588), the panel
(`crates/fe-workbench/src/git_log.rs`, T609) and the DiffView tab opener in `crates/fe-workbench/src/git_diff.rs`
(T551), used as T590's History panel uses it.

## Change

In `crates/fe-workbench/src/git_log.rs`:

- Enter (or `o`, or a click) on a commit row expands it: its `commit_files` are listed under it,
  indented, as `<letter> <path relative to the root>` (`A`/`M`/`D`, a rename as `R old → new`); Enter again
  collapses it. Enter on a file row opens, through `git_diff.rs`'s tab opener as T590 does, a DiffView with both
  sides read-only: left `<short>^` = `text_at(first parent, from.unwrap_or(path))` (empty for a root commit or an
  added file), right `<short>` = `text_at(id, path)` (empty for a deleted file), tab title
  `<name> (<short>^ ↔ <short>)`, focus to the diff; opening the same one again focuses that tab.
- `j`/`k` (vim on) and `up`/`down` move over commit and file rows alike; `o` = Enter; a click on a file row opens it.
- `commit_files` is read on the background executor; the commit row shows `Loading…` under it until it arrives,
  and an error as a muted row under it.

## Tests

New file `crates/fe-workbench/src/git_log_open_tests.rs`, declared `#[cfg(test)]` from git_log.rs, with temp
repos built with the `git` CLI, developer config shut out, as T609's tests do.

## Decided by the lead overnight

- **Enter expands the commit's files in place; Enter on a file opens its diff**, as fugitive's commit buffer and
  VS Code's graph do, rather than opening a multi-file diff tab.
- **A merge shows its change against its first parent** (VS Code, GitHub), not a combined diff.
- The tab opener and its title format are T590's, so a commit opened from History and from Git Log is the same
  tab.

## Implementation notes

- The tab opener is `Workbench::open_commit` in `git_history.rs` (T590), not `git_diff.rs`; scope was respecced to
  add that file, whose only change is `pub(super)` on `open_commit`, so History and Git Log share one tab per
  commit and file.
