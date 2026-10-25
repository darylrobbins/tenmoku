---
task_uid: "01m5qphs3kq84xv4gg6h1wdven"
task: "T319"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-keymap tests/tree_vim_clipboard_keys.rs: in project_panel with vim on, x, c and p resolve to project_panel.cut, project_panel.copy and project_panel.paste on both platforms; in project_panel without vim, cmd/ctrl-x, -c and -v resolve to the same commands (VS Code's explorer); no binding is added in an editor context"
    required: true
    manual: false
  - id: "C2"
    text: "project_panel_clipboard_tests::copy_paste: c on a file then p on a folder creates a copy in it; p on a file pastes beside it; a name clash gets VS Code's ' copy' suffix (' copy 2' next) and never overwrites"
    required: true
    manual: false
  - id: "C3"
    text: "project_panel_clipboard_tests::cut_paste: x then p moves the entry, keeps an open buffer of it pointing at the new path (the rename path of T511), and a cut cannot be pasted into its own subtree (a toast says why, nothing moves)"
    required: true
    manual: false
  - id: "C4"
    text: "project_panel_clipboard_tests::marks: a cut row is drawn dimmed until pasted or until another x or c replaces it; the pending item survives a tree refresh and is dropped when its path is deleted, as seen through the normal watcher and listing observation (the missing-path check clears the clipboard only for a path it still finds missing). Documented limits (review round 3): (F6) an existence check held across a tree reopen (for example toggling ignored entries) is not invalidated by the reopen and can clear a clipboard whose path was recreated meanwhile, carried by follow-up T335; (F7) the willRename text-edit joiner merges same-path edits across resource operations, carried by follow-up T336."
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-keymap -p fe-workbench and cargo clippy -p fe-keymap -p fe-workbench --all-targets -- -D warnings pass; no existing test is edited"
    required: true
    manual: false
---

# Cut, copy and paste files and folders in the project tree with x, c and p, as nvim-tree does, moving or duplicating on disk with open buffers following (Mac T1127)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" The tree can create, rename, delete, duplicate and copy a path (T511/T523) but has no cut, copy and paste of entries, which nvim-tree binds to x, c and p and VS Code to cmd/ctrl-x/c/v.

## Current state

project_panel_ops.rs has duplicate_selected (:704) and copy_path (:741, the path text to the system clipboard); rename moves a file and its open buffers. No cut/copy/paste command exists in project_panel.rs or fastedit.toml (:830-900 binds a % r R d D Y gy only).

## Change

- New project_panel_clipboard.rs (declared from project_panel.rs): an in-panel clipboard `Option<(Cut|Copy, PathBuf)>` and commands project_panel.cut/copy/paste. Paste target: the selected folder, or the selected file's parent. Paste reuses the T511 move/copy primitives and LSP file-operation notifications (T951/T952) so an import-fixing willRename still applies to a cut.
- Name clashes follow nvim-tree (it prompts) but cheaper: VS Code's " copy" suffix, never overwrite.
- fastedit.toml: `x`/`c`/`p` in `project_panel && vim`; `secondary-x/c/v` in `project_panel` (VS Code explorer). Neither takes a key from an editor context.

## Tests

New files only: project_panel_clipboard_tests.rs (declared from project_panel.rs by #[cfg(test)] #[path]) over a tempdir tree, and fe-keymap tests/tree_vim_clipboard_keys.rs.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.

## Implementation notes

- **Scope widened (rev 2) to `crates/fe-project/src/fs_ops.rs` and a new `crates/fe-project/tests/fs_ops_paste.rs`.** fs_ops had no primitive that moves or copies an entry into *another* folder: `rename` stays in the entry's folder and `duplicate` copies beside it. A paste built on plain `std::fs` would lose T511's guarantees (every path resolved under the root through handles, no symlinked folder followed, nothing replaced), so three primitives were added beside them, sharing `duplicate`'s copy loop:
  - `fs_ops::copy_into(root, path, dir)` copies into `dir` under the entry's own name, or the first free copy name (`name copy.ext`, `name copy 2.ext`, …, as `duplicate` names them), each try atomic (`O_EXCL`/`mkdirat`).
  - `fs_ops::paste_name(root, path, dir)` looks up that name, for a cut: willRename servers and `Workbench::plan_rename` need the new path before the move.
  - `fs_ops::move_into(root, from, dir, name)` moves between the two folders' handles with `RENAME_NOREPLACE`/`RENAME_EXCL`, so a name taken after it was looked up fails rather than being replaced. A move to where the entry already is does nothing.
  - Both refuse a folder into itself or a folder inside it (`InvalidInput`) before touching anything. They are reached as `fe_project::fs_ops::*` (`lib.rs`'s re-exports were left alone; it isn't in scope).
- **Paste target.** The selected folder; beside the selected file; the root with nothing selected. A copied folder pasted with its own row selected goes beside it (`src copy`), as VS Code does. A cut pasted on its own row, or into a folder inside it, is refused with a toast ("“lib” can’t be moved into itself or a folder inside it.") and nothing moves. A cut file pasted into the folder it's already in does nothing and stays cut.
- **A cut moves much as a rename does** (`project_panel_clipboard.rs`, `move_now`): on the main thread; refused with a toast while `Workbench::busy_under` says the old or new path is being saved, opened or worked on; servers whose `willRename` filters match are asked first. Unlike a rename, their edits are made *after* the move (see review round 1, F1). The move is announced as `PanelEvent::Renamed`, so open editors take the new path (T511's `plan_rename`/`follow_moves`) and servers get `didRenameFiles` (T951/T952). An expanded folder stays expanded. After a successful move nothing is left to paste. A copy is made off the main thread, held in `RUNNING` (its source and its target folder) so a rename of either waits for it, then announced with `PanelEvent::Created` (`didCreateFiles`) and selected. It stays on the clipboard to be pasted again, as in VS Code.
- **The entry to paste** is held by its path from the root, so it survives a refresh (`g r`) and the tree being opened again (`H`). It's dropped once it's gone. When the tree suggests so (the nearest loaded folder above it is listed and up to date without it, checked after each watcher batch and each listing), `fs_ops::exists` asks the disk off the main thread (review round 1, F3). A cut row is drawn dimmed by passing `TreeRow::ignored(true)` (muted label, icon and chevron); kiln-ui has no separate "cut" style.
- **The name field.** cmd/ctrl-x and -c now resolve to `project_panel.cut`/`copy` while the field is open (the `text_input` context has no cut or copy binding of its own; its paste binding still wins). `run_in_field` sends them, and paste, to the field as `text_field::Cut`/`Copy`/`Paste`, so the field keeps its clipboard keys.
- **Review round 1 (kywpdq).**
  - F1: the move is made *before* the servers' edits. Everything is checked again once they answer: something acting on either path, the entry's device and inode, and the move itself (`RENAME_NOREPLACE`, so a new path taken meanwhile fails it). A move that fails makes none of the edits. It's an fs operation, so it can't be undone after the edits the way a buffer edit can. Each text edit for a file under the moved entry is remapped to the file's new path, matched by the panel's spelling or by the resolved path the server named. The edits are made in a `window.defer` after the `Renamed` event, once the editors have taken their new paths: `check_file_edits` resolves an open file's path, and an open file inside a moved folder only resolves once its editor has the new one. The server's buffer versions are unchanged by a move, so open-file edits still match. (Round 2 extended the remapping to every path in the edit: see F5.) Tests: `a_cut_checks_again_once_servers_answer` (a held answer, during which the new path is taken, then the file starts saving: each time nothing moves and no edit is made) and `a_cut_asks_language_servers_as_a_rename_does` (now also checks an edit to the moved, closed file itself, which lands at its new path).
  - F2: `copy_into` and `move_into` check whether the target folder is inside the entry by identity. They walk up from the target folder's opened handle through `..` until the root, so another spelling of the folder on a case-insensitive volume is refused too. A copy's traversal also skips any folder it made itself (`copying.owned`), so a target folder moved into the entry after the check can't make it copy its own output. A `move_into` the system refuses because the folder was moved inside the entry meanwhile reports `InvalidInput`. Tests: `fs_ops::paste_race_tests` (a new `Point::Pasting` race hook); `fs_ops_paste::another_spelling_of_the_folder_is_still_inside_it`, which runs only on a case-insensitive volume (macOS) and skips on Linux. The panel's own `dir.starts_with(&source)` check compares the tree's listed spellings, which are exact within one tree; fs_ops refuses anything that gets past it.
  - F3: a tree that hides ignored entries doesn't list one that's there, so dropping the entry to paste now waits for `fs_ops::exists`. That's new in fs_ops: it looks through the root's handles and never follows a symlinked folder. Test: `the_entry_to_paste_outlives_being_ignored`.
  - F4: names stay `OsString` from `paste_name` through `move_into` (which takes `impl AsRef<OsStr>` and validates a non-UTF-8 name for `/` and NUL only). Tests: `a_name_that_isnt_utf8_is_cut_and_pasted`, `fs_ops_paste::a_name_that_isnt_utf8_is_moved_and_copied`. Both skip on a volume that takes only UTF-8 names (APFS).
- **Review round 2 (xc30b4).**
  - F5: every path the servers' workspace edit names under the moved entry is remapped to the moved spelling, not only text edits'. That's each entry's `path` (text edits, creates, renames and deletes alike, and the `Depends` entries whose targets follow an operation) and a rename's destination (`EditTarget::Rename { to }`). A move is a one-to-one relocation of the subtree, so the edit made after it, with its operations in their order, does to the moved files what it would have done before. Test: `a_cut_folder_keeps_its_servers_file_operations` (moving `src/pkg`: rename a child and edit it after, create over an existing file with `overwrite` and fill it, then edit a file outside).
  - F6: each cut or copy gets a serial, so two of the same path never compare equal, and the clipboard counts what it has seen (`Clipboard::seen`: cuts and copies, watcher batches, listings). A look at the disk counts only if nothing has been seen since it started (`ProjectPanel::clip_looked`). Test: `a_late_look_never_drops_a_newer_entry` holds a "gone" answer while the file is made again and cut or copied again; the answer is ignored and the cut mark stays.
- Tests: `fe-keymap` `tree_vim_clipboard_keys::{macos,linux}`; `fe-workbench` `project_panel::project_panel_clipboard_tests::{copy_paste,cut_paste,marks,the_name_field_keeps_its_clipboard_keys,a_cut_asks_language_servers_as_a_rename_does,a_cut_checks_again_once_servers_answer,a_cut_folder_keeps_its_servers_file_operations,the_entry_to_paste_outlives_being_ignored,a_late_look_never_drops_a_newer_entry,a_name_that_isnt_utf8_is_cut_and_pasted}`; `fe-project` `fs_ops_paste::*`, `fs_ops::paste_race_tests::*`.
- **Review round 1 after rev 3 (eehdcr).**
  - F8: the resolved spelling of the moved entry, used to remap the servers' paths, is now taken through its folder only (`by_folder`: the folder canonicalized, the entry's own name kept), at both ends of the move. Before, `lsp::resolve` followed a moved symlink to what it led to, so edits and file operations naming that target (`src/impl.rs` for a moved `src/link.rs -> impl.rs`) were sent to a file of the same name where the link went (`dst/impl.rs`). Test: `a_moved_symlinks_target_keeps_its_edits` (a create-with-overwrite and a text edit of the target stay at `src/impl.rs`; `dst/impl.rs` is untouched).
  - F9: the new path is checked with `Workbench::busy_under` every time, both before the servers are asked and once they answer, whether or not a Save As reservation exists. So a paste into a folder the panel is trashing, duplicating into or creating in waits, with a toast naming the folder. (A rename gates the check on a reservation; a move into another folder can't.) Tests: `a_cut_waits_for_work_in_the_folder_it_goes_in` (nothing moves, the cut stays and the buffer's path is unchanged; once the work is done the paste goes ahead) and `a_cut_checks_its_folder_again_once_servers_answer`.

## Known limits

Review round 3 (1b8n69) left two medium findings, carried by follow-up cards rather than fixed here (lead respec, rev 3; C4 names both):

- **F6, T335: a look at the disk held across a tree reopen.** The clipboard's `seen` counter advances on each cut or copy, watcher batch and listing, but not when the tree is opened again (`ProjectPanel::reopen`, as toggling ignored entries does, or `opened`). Take a "missing" answer that started before a reopen, with the path recreated meanwhile: it can still clear the clipboard, even once the new tree lists the path again. Under the normal watcher and listing observation that C4 covers, stale answers are discarded (`a_late_look_never_drops_a_newer_entry`). The fix belongs in T335: advance `seen` on reopen, then reconcile the clipboard against the new tree.
- **F7, T336: the willRename joiner merges edits across resource operations.** `lsp_file_operations.rs`'s `joined` puts one file's text edits from every server into a single entry, at the first one's place, even when a create, rename or delete of that file comes between them. So a closed child edited, then overwritten by a `CreateFile`, then filled ends up empty: the second insertion was merged into the first edit, which lands before the create. This is the existing joiner (T951/T952), outside this card's scope. The cut's own remapping keeps each entry's order (`a_cut_folder_keeps_its_servers_file_operations`). Limiting the joining to runs between resource operations belongs in T336.
