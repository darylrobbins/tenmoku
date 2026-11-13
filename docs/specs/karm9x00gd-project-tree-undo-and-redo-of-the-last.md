---
task_uid: "01m78dvf9cdgw8ktkarm9x00gd"
task: "T1754"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::project_undo::undo_a_move, undo_a_rename, undo_a_create, undo_a_duplicate, undo_a_trash_restores_it, redo_repeats, refused_when_the_old_path_is_taken, name_field_keeps_its_own_undo pass"
    required: true
    manual: false
  - id: "C2"
    text: "fs_ops_undo (cargo nextest run -p fe-project) passes against the test trash only; project_panel_ops_tests and tests::lsp_file_operations pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Known limit, not a defect of this card: a same-user process replacing an entry between an identity check and a by-name mkdir, symlink or rename (review F3, F7) is not detected; the limit is documented at the fs_ops functions concerned, losing the race never deletes an entry (it goes to the Trash or the restore fails with a toast), and narrowing it is follow-up T1778"
    required: true
    manual: false
---

# Project tree: undo and redo of the last move, rename, create, duplicate and Trash

# Project tree: undo and redo of the last file operation (move, rename, create, Trash)

VS Code's Explorer undoes a move, rename, create and delete with cmd-Z; IntelliJ does not undo a delete but does offer refactor-aware moves. The tree has no undo (project_panel_ops_tests only undoes name-field text).

## Change
- A bounded stack (20) of operations in new project_panel_undo.rs: Create (undo moves it to the Trash), Rename/Move (undo moves back through fs_ops::rename/move_into, refused if the old path is now taken, with a toast), Trash (undo restores from the Trash location fs_ops::trash records; if it cannot, the entry is not recorded, so Undo never lies), Duplicate/Copy-paste (undo trashes the copy). Redo mirrors it.
- Commands `project_panel.undo` / `project_panel.redo` in the tree's context only (cmd-z / cmd-shift-z while it has focus; never while a name field is open, where they are the field's).
- Editors follow undone renames (`PanelEvent::Renamed`), LSP willRename as for the original.

## Pins
project_panel_ops_tests (name-field undo cases at lines ~885-960 stay), project_panel_clipboard_tests, tests::lsp_file_operations, fs_ops_test_trash.rs (the real Trash is never touched: use the existing test trash seam, docs/specs/nrzm8vgrgy-keep-tests-out-of-the-user-s-real-trash.md).
Platform: restoring from the macOS Trash may need a platform call; keep it behind kiln's platform seam, so the card is not linux-ok until the Linux (freedesktop trash) restore exists; if that is too big, ship macOS and Linux no-ops with Undo hidden.

## Known limit (lead decision 2026-11-13, review round 2 F3/F7)
Same-user races are outside fs_ops's threat model, as its existing create/copy cleanup and Trash already assume. Another process of the same user that replaces an entry between fs_ops's identity check and its by-name mkdir/symlink/rename (F3: a made folder's or symlink's identity sampled after it is made; F7: a staged restore published by name after validation) is not detected: no POSIX or macOS call binds a rename to a checked identity. Losing that race moves the other process's entry to the Trash (never deletes it; redo puts it back) or makes a restore fail with a toast. This is stated at the fs_ops functions concerned. Narrowing those windows is follow-up T1778, not this card.

## Implementation notes

- `fs_ops::trash_recorded` returns a `Trashed` per entry moved (where it was, where it went in the Trash if the system says, and its device and inode); `fs_ops::trash` is it with the records dropped. `fs_ops::restore(&mut Trashed)` puts one back: the Trash folder is opened a component at a time with `O_SEARCH` (which the protected macOS `~/.Trash` allows, though not listing or `O_RDONLY`) and the entry checked to be the one trashed. It's moved into a folder of the restore's own (mode 700, unpredictable name), and the record follows it there at once. It's checked there, then moved on from that folder's handle with `rename_noreplace` into its old folder (found through the root's handles), and checked where it lands. If something else landed (swapped into the staging folder in that instant), it's left alone and the restore fails. If putting it back fails, the entry goes back to its Trash name, and the record with it; if that can't be done, it stays staged, where the record already says (`Trashed::in_trash`). Race tests are in `fs_ops::undo_race_tests`. No platform call is needed, so nothing goes through kiln.
- `create_recorded`, `duplicate_recorded` and `copy_into_recorded` say what they made (`Placed`: path and identity, from the operation; a copy is checked to be one it made). A create's first entry is the first folder it made, so a folder another process made meanwhile is never undone (race test on the new `Point::Making`). A rename records the identity sampled just before it; a cut's paste the one it checked.
- **Known limit:** a folder or symlink's identity is read through its parent's handle right after `mkdir`/`symlink` makes it. Another process of the same user that swaps it in that instant would have its entry recorded as made, and an undo would move that entry to the Trash (never delete it; redo puts it back). Closing that window means building every create and copy in a private staging folder and renaming it into place: a rewrite of fs_ops's create and copy machinery, left for a card of its own.
- Where trashes are followed: macOS (`trashItemAtURL:resultingItemURL:`) and the test trash (`fs_ops::SYSTEM_TRASH_RESTORES`, `fs_ops::restores_from_trash`). Elsewhere nothing is recorded, the commands aren't registered and the keys (macOS-only bindings) aren't there, since an undone create couldn't be redone. The card is not linux-ok until a freedesktop restore (rename out of `files/`, remove the `.trashinfo`) exists. `tests::project_undo` is macOS-only for the same reason, and the default-keymap pin lists the two commands as macOS-only.
- One step per operation, however many entries. Undoing a step pushes what the undo did onto the redo stack, and redo reverts that, so both ways run the same code. Both stacks hold at most 20 steps.
- Each entry is checked on its own before a step is reverted. An entry that's gone, or no longer in the Trash, is dropped from its step, with a toast, and the rest go on. A path taken, an entry out of reach (a permission error is never taken for "gone") or something busy keeps the whole step, with a toast. An entry replaced since (a save replaces a file, whoever saves it) is never adopted silently: the panel asks, naming every replaced entry, and goes ahead with only the identities it showed (one replaced again while asked is asked about again). Entries that fail as a step is reverted (permissions, a missing folder, a save that landed while language servers answered) go back to the stack they came from, below anything newer; only entries found gone are dropped.
- A new operation that changed something (even one that can't be recorded) empties the redo stack and moves a generation on, so an undo still under way doesn't refill it; one that changed nothing keeps it (`project_panel::undo::history_tests`).
- Undone moves and renames go through the clipboard's move path (`MoveOf::Revert`), so they get the same busy and race checks, `PanelEvent::Renamed` and willRename edits as a cut's paste. A same-folder move back goes through `fs_ops::rename`, so a rename that only changed case is undone too. Files the servers' edits replace are asked about on the next undo or redo, as any replaced file is.
- The keys are bound in `project_panel` (the context fe-gpui's keymap tests whitelist for the tree). While the name field or the filter box has focus, the panel dispatches the box's own Undo/Redo instead, so cmd-z there stays the field's. Command titles and messages are in project.ftl, which is in scope; commands.ftl isn't.
- `crates/fe-project/tests/fs_ops_undo.rs` is its own test binary; no `tests/main.rs` was needed.
