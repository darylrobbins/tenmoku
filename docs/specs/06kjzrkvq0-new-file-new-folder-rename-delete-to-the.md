---
task_uid: "01m50ew68d5zf90506kjzrkvq0"
task: "T118"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "project_panel_ops_tests::create: new_file on a folder row shows a name field; typing src/x.rs and Enter creates it, selects its row and opens it focused; an existing name shows the error and keeps the field; escape creates nothing"
    required: true
    manual: false
  - id: "C2"
    text: "project_panel_ops_tests::rename_follows: renaming an open file, and a folder holding one, moves the file(s) and every editor of them (including a split in another pane) shows the new path in its tab and title"
    required: true
    manual: false
  - id: "C3"
    text: "project_panel_ops_tests::delete: delete asks Move “x” to the Trash?; Cancel keeps the file, Move to Trash removes it from the folder and the tree, and its open editor stays open with its buffer"
    required: true
    manual: false
  - id: "C4"
    text: "project_panel_ops_tests::misc: duplicate creates name copy.ext and selects it; copy_path and copy_relative_path put the absolute and /-separated relative path on the clipboard; select_first/select_last select the first and last rows"
    required: true
    manual: false
  - id: "C5"
    text: "project_panel_ops_tests::vim: with vim on the panel's key context carries the vim flag, keys typed in the name field reach the field only, and a file opened from the tree is in normal mode; cargo test -p fe-workbench and clippy -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
  - id: "C6"
    text: "fe-editor move_path_tests (F1): after EditorModel::move_path(b) on a saved buffer whose file was moved to b, b changing on disk behind the editor is still detected as a save conflict (the stamp moved with the path), and no reload happens; a tree rename of an open file uses it for every split"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench disk_sync_rename_tests (F3): a disk check for the old path held in flight across a rename, then completed, neither clears the buffer's stamp nor shows Reload/Keep Mine; the next check runs against the new path"
    required: true
    manual: false
---

# New file, new folder, rename, delete to the Trash, duplicate and copy path in the project tree, with open buffers following renames (Mac T511)

## Problem

The project tree has no file operations (day-one audit 2026-10-12: project_panel.rs "has reveal + selection
only"). The previous card adds `fe_project::fs_ops`; T434 adds `EditorModel::set_path`, which open buffers need to
follow a rename.

## Change

In `crates/fe-workbench/src/project_panel.rs`, with the operations in a new `crates/fe-workbench/src/
project_panel_ops.rs` declared from it. Commands in the panel's registry (context `project_panel`), on the selected
row; keys and menu items are a later card:

- `project_panel.new_file`, `project_panel.new_folder`: an inline name field as a new row in the selected folder
  (or the selected file's folder), VS Code style. Enter creates (`create_file`, `create_dir`, `/` allowed), escape
  or blur cancels; an error (`NameError`, exists) shows under the field and keeps it open. A new file opens in the
  active pane, focused.
- `project_panel.rename`: the same field on the row, its stem preselected. Enter renames.
- `project_panel.delete`: asks "Move “name” to the Trash?" (Move to Trash / Cancel, Cancel default), then `trash`.
  Editors of a deleted file stay open with their buffers.
- `project_panel.select_first`, `project_panel.select_last` (for vim's `gg`/`G`); `project_panel.open` on a folder
  toggles it, so `l`/Enter open a file and expand a folder.
- `project_panel.duplicate`, `project_panel.copy_path`, `project_panel.copy_relative_path` (to the clipboard).
- After a rename, every editor in every window whose path is the old path, or under a renamed folder, takes the
  new one through `EditorModel::set_path` (T434), so its tab, title, language and LSP document follow. The
  panel emits `PanelEvent::Renamed { from, to }` and the workbench applies it (workbench.rs).
- The watcher's events update the tree as for any outside change; the new or renamed row is selected.
- Vim: the panel's element adds the `vim` flag to its key context while `fe_gpui::vim_enabled`, for the keys
  card's netrw/nvim-tree keys; the name field is `text_input`, so vim never sees keys typed there. A file created
  or opened from the tree opens in vim normal mode; a renamed open buffer keeps its vim mode and caret.

## Tests

New file `crates/fe-workbench/src/project_panel_ops_tests.rs`, declared from project_panel_ops.rs, on a temp
folder with the panel's test helpers (`tests/project_panel.rs`).
## Rev 2 (Mac lead decision, 2026-10-15 note zu): widen the scope for review F1 and F3

Review F2, F4, F5, F6 and F7 are fixed (864bf434). F1 and F3 needed files outside the scope; the decision is to widen
the scope rather than accept them:

- **F1, a rename drops save-conflict protection** (`crates/fe-editor/src/disk.rs`): `EditorModel` keeps
  `history.stamped` at the old path after `set_path`, and nothing public moves it without reloading or saving. Add a
  small `EditorModel` API, e.g. `move_path(path)`: `set_path` and move `history.stamped` with it, no reload. Use it for
  renames (and so for every split of a renamed buffer).
- **F3, an old-path disk check in flight across a rename** (`crates/fe-workbench/src/disk_sync.rs`): checks validate
  only the buffer id and stamp, so a check for the old path that completes after the rename can clear the stamp or
  prompt Reload/Keep Mine. Have disk_sync drop a check (or its question) whose editor's path changed after the check
  started: record the path it started with and compare it on completion.

Each fix gets a regression test in a new test file: `crates/fe-editor/src/move_path_tests.rs` (F1) and
`crates/fe-workbench/src/disk_sync_rename_tests.rs` (F3), declared as `#[cfg(test)]` modules from disk.rs and
disk_sync.rs. C5's spirit holds otherwise: nothing outside the scope changes.

## Implementation notes

- **The name field is kiln-ui's `TextField`**, which has a caret, a selection, undo and the clipboard, in a
  `text_input` key context: a rename starts with the stem selected (up to the last `.`, or the whole name for
  `.gitignore` or a folder without one), so typing replaces it and keeps the extension. `TextInput` has no
  selection. The `text_input.*` commands (backspace, delete word left, clear, paste) edit the field, the panel's
  left and right move its caret, and the field's own GPUI keys (`TextField::key_bindings`, bound by
  `project_panel::init` in the field's own `text_field` context) do the rest. Vim never sees what's typed there.
- **Enter and escape without new keys.** The keymaps are a later card. Enter in the field arrives as the panel's
  own `project_panel.open` binding (it is in the panel's key context), which the panel takes as "run the field"
  while the field has focus; `text_input.submit`, and the field's own Enter, do the same. Escape, which nothing
  binds there, is handled as a key-down on the field's row. While the field has focus the panel's other
  commands do nothing.
- **Vim flag off while naming.** The panel's key context is `project_panel vim` with vim on, but plain
  `project_panel` while the name field is open, so the keys card's `project_panel && vim` bindings (`j`, `l`,
  `d`, …) can't take letters typed in the field.
- **Opening from the tree puts the editor in normal mode once it shows**, an editor already open included: the
  workbench sends it escape (as `recent_picker` and an external drop do) when it was left in insert, a visual mode
  or a pending command. An open that waits (behind a Save As of its file) is watched per pane
  (`Workbench::tree_opens`, an observation of the pane) until its editor shows or the pane stops reading it; a later
  tree open in the same pane replaces it, one in another pane doesn't.
- **A rename runs on the main thread**, refused (the field says why, and stays open) while something would still
  act on the old path after it (`Workbench::busy_under`): an editor saving to the file or under the folder, by its
  path or by the file the save writes through a symlink (`fe_editor::save_target`), whose tab and window may have
  closed since (`EditorView::is_saving` covers a save waiting for its hook; the workbench holds each tracked editor
  until it sees its tab close, and hands one still saving, at its tab's close or its own release with its window,
  to the app-wide `DetachedSaves` until the save is over); a Save As destination held there
  (`open::reserved_paths`); in any window, a file there being read into a tab, or the launch file; or a project
  panel's create, duplicate or move to the Trash overlapping it either way, from any window, open or closed since
  (`project_panel::running`: an app-wide registry whose entries are held by the background job and by what takes
  its outcome in, until both are over; a create records its whole destination with the folders its name makes, a
  duplicate the folder its copy goes in, a trash its entry, each spelled as the tree spells it, under the canonical
  root, so the folders match any spelling of the project, a symlinked root's included). Run on the main thread, nothing new can start between the
  rename and the editors taking the new path: `PanelEvent::Renamed` is taken in within the same turn, before any
  later watcher batch, so a batch naming the old path no longer matches an editor. Creating, duplicating and
  trashing stay off the main thread.
- **Paths match by resolved folders, not only lexically, planned before the rename.** An editor whose path spells
  the renamed entry through other folders (the project opened through a symlink, its real path in another window,
  `/var` against `/private/var`, a symlink straight into the renamed folder) is matched by `resolved` (its folders
  canonicalized, the entry itself not dereferenced) and moved under the destination's resolved spelling
  (`moved_to`). Every window's moves are worked out before the rename (`Workbench::plan_rename`), while every
  spelling still resolves (a symlink into the renamed folder dangles after it), and travel in
  `PanelEvent::Renamed { moves }`; each workbench applies its own (`follow_moves`). `busy_under` matches the same
  way (`under`, `overlap`). A symlinked file keeps its own path when its target is renamed.
- **The Trash question is about one entry**: its device and inode are taken when it's asked, and checked again
  before it's moved; an entry renamed meanwhile and another put at its path is kept, the prompt saying so.
- **Rev 2, F1: `EditorModel::move_path(path)`** (fe-editor `disk.rs`) is `set_path` plus the history's stamped
  path moving with it, when it was the old path: a save of the moved buffer still checks the file against the stamp
  it knows, and refuses it if it changed behind the editor. Nothing is read or reloaded; a buffer whose stamp was
  for another path (given a path since) keeps it where it was. `follow_rename` uses it, once per buffer, so for
  every split. `move_path_tests` covers a moved buffer saving, then refusing a change behind it from either split,
  and what `set_path` alone would have missed; `rename_follows` covers a tree rename asking before a save from
  either split overwrites a change made at the new path.
- **Rev 2, F3: a disk check records the path it started with** (`disk_sync.rs`, `Check::path`). As each step lands
  (`checked_editor`, which the stat, the read, the queued question and its answer all go through), an editor whose
  path is no longer that path is treated as a stale check: the result is dropped and the check starts over at the
  new path, as it does when the stamp moved. `disk_sync_rename_tests` holds an old-path stat (which finds nothing)
  across a rename: once landed it neither clears the stamp nor asks, a second check runs at the new path, and a
  later change there asks about the new name.
- **Every editor is collected before any path changes**: `set_path` through one editor is every split's, so each
  affected editor then detects its language from the new name.
- **A new file's name ending in `/`** makes a folder (as `create_file` does) and opens nothing.
- **The tree is updated by the panel, not only the watcher.** Once an operation succeeds the panel applies the
  changed paths to the tree itself (as `apply_fs_changes`, so the workbench hears of them too) and then reveals the
  new or renamed row. Waiting for the watcher alone would leave nothing to select until its batch arrived, and the
  watcher is off in tests. Its later batch for the same paths changes nothing. A folder expanded before its rename
  stays expanded.
- **Editors only.** A rename moves the editors in tabs (every split of a buffer). A read-only viewer tab of a
  renamed large file, or a diff view of it, keeps its old path.
- **Carried finding T114 F2.** `Workbench::focus_handle` now falls back to the active pane's `focus_target` (its
  editor or viewer), keeping the diff-editor handling of `active_editor`. `workbench::focus_tests` covers a Large
  launch file's viewer taking focus and movement keys at launch and after a hidden dock gives focus back.
- Tests are `project_panel::ops::project_panel_ops_tests::{create, rename_follows, rename_waits_for_saves,
  rename_waits_for_what_acts_on_the_old_path, rename_follows_other_spellings, deferred_tree_opens_in_two_panes,
  operations_hold_renames_across_spellings_and_windows, delete, misc, vim}`, `fe-editor`'s
  `disk::move_path_tests` and `disk_sync::disk_sync_rename_tests`. The panel's tests carry their own small helpers
  (panel, rows, focus) because `crate::tests`' are private to that module and outside this card's scope. `delete` uses the real system trash on a temporary folder under the home
  trash's volume, as `fe_project`'s trash tests do, and removes what it trashed.
