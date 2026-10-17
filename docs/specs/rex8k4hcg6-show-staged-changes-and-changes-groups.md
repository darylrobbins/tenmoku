---
task_uid: "01m4xqgk55w8qm3vrex8k4hcg6"
task: "T124"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_panel_groups_tests::groups: with one staged-only, one unstaged-only, one staged-and-changed-again and one untracked file, the panel shows 'Staged Changes' with 2 rows and 'Changes' with 3 (the twice-changed file in both, the untracked one as U); with nothing staged the Staged group is absent"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test git_panel_groups_tests::stage_unstage: the Stage button on a Changes row stages it and Unstage on a Staged row unstages it (status_split agrees after the refresh); Stage All and Unstage All act on every row; Enter on a Staged row opens HEAD ↔ Index, on a Changes row Index ↔ Working Tree"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test git_panel_groups_tests::discard_confirms: Discard on a tracked row prompts 'Are you sure you want to discard changes in a.rs?'; Cancel changes nothing, Discard File restores the index text on disk; an untracked row prompts the DELETE wording and Delete File removes it"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test git_panel_groups_tests::vim_and_ex: with vim on and the panel focused, 'j' 'k' move, 's' stages, 'u' unstages, '-' toggles, 'X' asks to discard; with vim off those letters do nothing; ':Gwrite' saves then stages the active file, ':Gread' replaces the buffer with its index text undoably and unsaved"
    required: true
    manual: false
  - id: "C5"
    text: "A git failure shows an error toast with git's stderr; cargo test -p fe-workbench passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show Staged Changes and Changes groups in the Changes panel, and stage, unstage and discard files from it, with fugitive's keys in vim mode (Mac T552)

## Problem

The user (2026-10-12) asked to stage and unstage whole files from the Changes panel and to discard a file with
confirmation. The panel (`crates/fe-workbench/src/git_panel.rs`) lists one combined status per file
(`Repo::status`), so a file can't be shown as staged or not, and has no actions. The vim-mode requirement (coordinator,
same day) asks for vim-reachable commands; fugitive's status window keys are the convention.

## Change

In `crates/fe-workbench/src/git_panel.rs`, `crates/fe-workbench/src/git.rs` and `crates/fe-workbench/src/git_stage.rs`
(replacing its file-level stubs):

- `GitState` refreshes with `Repo::status_split`. The panel shows VS Code's groups: "Staged Changes" (only when there
  are any) and "Changes", each with a count; a file staged and changed again is in both. Untracked files are in
  Changes (letter `U`). Enter on a Staged row opens HEAD ↔ Index, on a Changes row Index ↔ Working Tree (the diff
  wiring card's view; HEAD ↔ Working Tree stays the change bar's).
- Row buttons on hover and on the selected row (VS Code's): Changes rows "Open File", "Discard Changes", "Stage
  Changes"; Staged rows "Open File", "Unstage Changes". Group headers: "Stage All Changes"/"Discard All Changes" and
  "Unstage All Changes".
- `git.stage_file`, `git.unstage_file`, `git.discard_file` act on the panel's selected row when the panel has focus,
  else on the active editor's file. `:Gwrite` and `:Gitsigns stage_buffer` save the buffer, then stage the file
  (fugitive's `:Gwrite` writes the work tree, then the index); `:Gitsigns reset_buffer_index` unstages it. `:Gread`
  and `:Gitsigns reset_buffer` (`git.reset_buffer`) replace the buffer's text with its index text as one undoable,
  unsaved edit (fugitive's `:Gread`).
- Discard confirmation, VS Code's wording: tracked, `window.prompt(Warning, "Are you sure you want to discard changes
  in <name>?", None, ["Discard File", "Cancel"])`; untracked, "Are you sure you want to DELETE <name>?" with detail
  "This is IRREVERSIBLE! This file will be FOREVER LOST if you proceed." and ["Delete File", "Cancel"]; several, "Are
  you sure you want to discard changes in N files?" Open editors pick up the restored file through T433's reload.
- Git work runs on the background executor (`Repo::stage`/`unstage`/`discard`), then refreshes; a failure shows an error toast (T517) "Git: <first line of stderr>" with
  the rest as detail.
- With vim on, panel keys follow fugitive's `:Git` status window: `j`/`k` move, `s` stage, `u` unstage, `-` toggle
  staged, `X` discard (with the same confirmation), `Enter`/`o` open the diff. With vim off these
  letters do nothing (no new non-vim panel keys).

## Tests

New file `crates/fe-workbench/src/git_panel_groups_tests.rs` (declared `#[cfg(test)]` from git_panel.rs).

Lead decision after review round 3 (finding F5): when Gwrite, Gitsigns stage_buffer or a staged-diff side looks for the open working editor of a file, match it by file identity (the canonical path, the same way file_model finds a file's model), not by the path's spelling. A dirty buffer opened through a symlinked directory must be saved and staged, never skipped in favour of the older disk contents. Cover it in git_panel_groups_tests: from each side of a staged diff, a dirty buffer opened through a directory alias is staged with its unsaved text.

## Implementation notes

- Scope widened (respec) to `git_diff.rs`, for the HEAD ↔ Index view (`Base::Staged`, both sides read-only and
  re-read on every refresh) and the Changes row's Index ↔ Working Tree, and to the existing tests whose expectations
  this card changes: `git_panel_tests.rs` and `git_diff_tests.rs` (panel rows no longer open HEAD ↔ Working Tree;
  the HEAD-diff tests open it directly through a test-only `Workbench::open_head_diff`), `git_commands_tests.rs`
  (`Git: Stage File` and `:Gread` are no longer stubs) and `git_live_tests.rs` (listed, unchanged).
- `GitState` reads `Repo::status_split` once per refresh; the project tree's and tabs' single status per file is the
  halves combined as `Repo::status` combines them (`git_panel::combined`). A conflicted file is only in Changes.
- After a row leaves its group (staged, unstaged), the selection moves to the row now at its place in that group, so
  `s s s` walks down Changes as fugitive's does; else the same file in the other group.
- kiln-ui's icon set has no plus or undo arrow: Stage is a check, Unstage a minus, Discard an arrow left.
- Several files to discard with untracked ones among them add the detail "This is IRREVERSIBLE! The untracked files
  among them will be FOREVER LOST if you proceed."; the button is "Discard N Files".
- No repository is answered like the diff commands' (vim line, else a toast); a git failure is the error toast.
- `git.reset_buffer` on a file not in the index answers "<name> is not in the index"; on a read-only side, E21.
- Review round 1: git acts on the named path with only its folder resolved, so a symlink is staged and discarded as the
  link; discarding reads the status before asking (tracked or untracked, files with nothing to discard left out) and
  again once confirmed, discarding only files still as asked; a focused panel with no selection acts on nothing; a
  staged diff's sides stay clean as refreshes update them, and file commands typed in them act on its file
  (`Workbench::op_path`); `:Gread` answers E21 in a read-only side, lands only if it's the editor's last read and the
  editor still edits that file, and is an undo step of its own; git work that changes the repository runs one at a
  time in the order asked.
- Review round 2: `:Gread` reads are numbered per buffer (any editor sharing it) and land only on that buffer; staging
  from a staged diff's side saves the file's open working editor first; a save-then-stage takes its place in the git
  work queue when asked, waiting there for its save; `diff.compare_file` reads the active file from disk as well, used
  if its buffer has closed or moved to another file by the time the comparison lands (a file gone from disk is
  answered).
- Review round 3 (lead's decision on F5): a staged diff's side finds the file's open working editor by the file's real
  path (`git_diff::same_file`, as `file_model` matches), so a dirty buffer opened through a symlinked folder is saved
  and staged by `:Gwrite` and `:Gitsigns stage_buffer` from either side.
- Review after the round-3 respec: a save-then-stage queues the save itself, started when its turn comes (after a
  discard queued before it, which the editor's own "changed on disk" question then guards; refused, nothing is
  staged); the queue starts outside the update that asked, so that save can update its window. A staged diff reads
  both blobs at the tracked name with only its folder resolved (`git_diff::link_path`, shared with `git_stage`), so a
  symlink shows its targets, on opening and on refresh.
- Review round 2 after the respec: a queued save-then-stage checks, as its turn comes, that its editor still edits the
  buffer and file it was asked for; if not (Save As, a rename), nothing is saved or staged and it says so. An open
  staged diff is found again by the tracked entry's path (`link_path`), so links to one file, and the file, each have
  their own.
