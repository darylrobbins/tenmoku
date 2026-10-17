---
task_uid: "01m4rhe10x8hwp8qv2dngcb0yq"
task: "T509"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "hot_exit_tests::quit_restores: in a window with a workspace layout (a folder window), a dirty file buffer and an untitled buffer with text quit without a prompt, and a new workbench restoring the layout shows both dirty with their text and carets; the file buffer's first cmd-z returns it to the disk text. Rootless windows (no-argument launch) are out of scope here: they keep today's prompt and get hot exit in a follow-up card"
    required: true
    manual: false
  - id: "C2"
    text: "hot_exit_tests::debounced: an edit writes a backup about 1 s after the last keystroke (not per keystroke), saving or undoing to clean removes it, and closing the tab with Discard removes it"
    required: true
    manual: false
  - id: "C3"
    text: "hot_exit_tests::crash: with backups and the layout written but no quit (the workbench dropped), a new launch restores the same editors; an unreferenced backup opens as a tab in the first window and the store is pruned"
    required: true
    manual: false
  - id: "C4"
    text: "hot_exit_tests::close_still_asks: closing a dirty tab or window (not quitting) still asks Discard or Cancel as today"
    required: true
    manual: false
  - id: "C5"
    text: "hot_exit_tests::vim: with vim on, a restored editor is in normal mode at its saved caret, and one u undoes the restored changes, including when the restored text equals the disk text; Buffer::from_backup records the restore as a user-undoable edit keeping its encoding, BOM and replaced flag (fe-buffer, with tests/backup.rs updated); cargo test -p fe-workbench -p fe-gpui -p fe-buffer and clippy -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Hot exit: quit without asking and restore unsaved and untitled buffers with the layout, backed up as you type so a crash loses at most a second

## Problem

fastedit loses unsaved work on quit (it asks per editor) and on a crash, and never restores untitled buffers
(`EditorView::serialize` returns `None` for them, crates/fe-gpui/src/pane_item.rs:249). Day-one audit 2026-10-12.
The previous card adds `fe_buffer::BackupStore` and `Buffer::from_backup`.

## Change

Conventional hot exit, as VS Code's default (`files.hotExit: onExit`), always on:

- New `crates/fe-workbench/src/hot_exit.rs` (declared from workbench.rs): the app's `BackupStore` at
  `<state_dir>/backups`, set as a global for fe-gpui. For each open buffer (once per buffer, not per split): while
  dirty, a backup is written off the UI thread 1 s after its last edit (debounced, and at most one write in flight
  per buffer); when it becomes clean (saved, or undone to clean) or its last editor closes, its backup is removed.
- `crates/fe-gpui/src/pane_item.rs`: `serialize` adds `"backup": <id>` for a dirty buffer and now saves untitled
  buffers too (backup id and title, no path). `restore` with a backup id reads it and builds the buffer with
  `Buffer::from_backup` (file read as today); a missing backup falls back to the file as today. `may_close` while
  the app is quitting (`kiln_shell::layout::quitting`) flushes the buffer's backup synchronously and answers
  `Close` without asking; closing a tab or a window still asks, as today.
- Quit: the layout's at-quit save (`Workbench::set_layout`) runs after every backup is flushed.
- Crash: backups and layouts are both written as things change, so the next launch restores the same way. At
  startup, after layouts restore, backups no restored editor references are opened as tabs in the first window
  (untitled, or their path), then the store is pruned to the referenced ones.
- Vim: a restored editor opens in vim normal mode with its caret where it was saved. Vim marks, the jumplist,
  registers and undo history aren't persisted today and aren't restored; one `u` (or cmd-z) right after restore
  undoes the restored changes back to the file on disk.

## Tests

New file `crates/fe-workbench/src/hot_exit_tests.rs`, declared from hot_exit.rs, with a temp state dir.

## Implementation notes

- **One store per root.** Backups live at `<state_dir>/backups/<key>`, beside the root's
  `<state_dir>/layouts/<key>.json`, derived from the window's `LayoutStore` (so tests use their temp
  state dir). A single shared `<state_dir>/backups` would let a launch on one folder prune, or open
  as tabs, another folder's backups. Hot exit is on in windows with a saved layout. A rootless window
  (a no-argument launch, which main gives no `WindowLayout`) keeps today's prompt at quit; the
  follow-up card gives it hot exit (C1, rev 6).
- **The quit's own close.** `kiln_shell::layout::quitting` is set only by `save_at_quit`, after the
  close transaction has asked every item, so `may_close` can't see it during `app.quit`. Hot exit
  replaces kiln-shell's `app.quit` handler with one that builds the same `CloseKind::Quit` request,
  through `close::confirm`, from items wrapped in `BackedUp`. Only that wrapper writes a dirty
  editor's backup (`pane_item::back_up_to_close`) and answers `Close`. So a tab or window close
  pending when the quit is asked for, or one asked for while the quit waits, asks as ever (review
  F6). `EditorView::may_close` does the same only under `layout::quitting`. The quit's commit is
  `cx.quit()`; in tests, `hot_exit::set_quit`, as kiln's `set_quit` can't be read from outside.
- **Undoing the restore.** `Buffer::from_backup` now records the restore as `Origin::User` (it was
  `Origin::Host("backup")`, which cmd-z and vim's `u` skip), so the user's first undo takes it back to
  the file's text and save format, clean, including a restore that changes no text. The buffer keeps
  the backup's encoding, BOM, line ending and replaced flag while it's applied. `pane_item` restores
  through `from_backup` as it is.
- **Freshness.** A backup is current when it holds the buffer's `BackupRevision`: its text version,
  path, line ending, encoding and disk stamp. So a rename, a Save As, a format change, or a save that
  finishes while newer edits stay unsaved is backed up again without another edit. The debounce, the
  quit's flush and `back_up_to_close` all compare the same thing (review F2).
- **Two slots.** `BackupStore::write` uncommits the backup it replaces before writing the new one, so
  rewriting one id in place could lose everything in a crash (review F7). Each buffer's backup has
  two slots: its id and the id's twin, which is the id with the top bit of its pid set (no pid has
  it, so a twin never collides with a real id). Each write goes to the slot that doesn't hold the
  newest backup. A read (`pane_item::read_newest`) takes the newer of the slots that read, by
  `meta.written`. `BackupMeta` has no generation field and is outside this card's scope, so each write
  keeps its `written` past the newest slot's, by at least 1 ns, whatever the clock says. That makes it
  a per-backup generation, even across a clock going back (review F10). Layouts, owner files and
  orphans go by the first id. A backup is given up as corrupt only if neither slot reads and each
  is corrupt or missing. A slot that fails otherwise, such as one that can't be opened, keeps the
  backup for a later launch, and only the slots confirmed corrupt are pruned (review F11).
- **Ownership.** While a backup is on disk, the process keeping it holds an exclusive lock on
  `<id>.owner` beside it (taken before the first write, or when a restore or launch claims it), and
  the OS drops the lock if the process dies. A launch opens or restores only backups it can claim, so
  another live app's backups, named in a layout or not yet, are never adopted or pruned (review F4).
  Owner files are created, locked and unlinked only while holding the store's `owners.lock`. So none
  is unlinked between being opened and locked, and a removal unlinks its own before letting go of it
  (review F9).
- **Retries.** A failed write is tried again, as the buffer is at that point, after a wait that
  doubles from 1 s up to 30 s, until it succeeds, the buffer turns clean, or a newer job replaces it.
  A failed removal retries the same way, and so does a closed buffer's, however long it takes. A
  closed buffer's removal still pending is kept app-wide, and quitting runs it: through the open
  windows' at-quit save, and through an app-level quit hook for windows that already closed
  (review F8).
- **One write at a time.** Each buffer's `BufferBackup` gives every write and removal a turn and runs
  them under a lock, skipping any made before one that already ran. So the synchronous flush at quit,
  a debounced write still in flight, and a removal land in the order they were made. The main thread
  reads what landed under a separate, briefly held lock, so it never waits on a write.
- **Layout saves.** After each backup write or removal the layout is saved in full, with carets, so a
  crash restores what a quit would. Otherwise it's saved when its shape or the backups it names
  change.
- **Orphans.** Orphans are the backups no entry in the root's layout file names (restored or not, so a
  file launch doesn't take the backups of windows it didn't restore) and no one owns. They open once
  the window's own layout restore has finished, since a pane the restore left empty would close with
  them in it. An orphan that doesn't read, because its file can't be opened say, stays for a later
  launch. Pruning removes only half-written backups, those whose text and meta disagree, and owner
  locks with no backup. Nothing is pruned when the store can't be listed (review F1).
