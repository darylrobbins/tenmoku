---
task_uid: "01m4rjfjey7hm8qwhqd78yttzy"
task: "T51"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "backup: write then read round-trips text and meta (path or untitled title, disk stamp, encoding, BOM, line ending) byte for byte, including CRLF and non-UTF-8-encoded files' text; files are mode 0600 and the directory 0700"
    required: true
    manual: false
  - id: "C2"
    text: "backup: a text file without its meta, or a meta without its text, is not listed by ids() and is removed by prune; prune keeps exactly the ids keep() accepts"
    required: true
    manual: false
  - id: "C3"
    text: "backup: Buffer::from_backup over the disk file is dirty with the backup text, and one undo returns it to the disk text; over None it is untitled-style with the backup text"
    required: true
    manual: false
  - id: "C4"
    text: "backup: the restored buffer's disk stamp is the backup's, so a save after the file changed on disk since the backup is refused as a stale save"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-buffer and cargo clippy -p fe-buffer --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Keep backups of unsaved buffers in fe-buffer: an atomic, private on-disk BackupStore and Buffer::from_backup that restores them dirty over the disk text (Mac T508)

Filed on argon from Mac T508 (card ~/coordination/argon-cards/T508.json rev 1). Mac task uid 01m4rhe0h0591vdaj60v7qb28b. Depends on argon T47 (Mac T487, MarkerSet), which overlaps it in fe-buffer. Title the host commit "... (Mac T508)".

## Problem

Quitting with unsaved changes asks per editor (`PaneItem::may_close`, crates/fe-gpui/src/pane_item.rs), and a
crash loses them: the saved layout keeps only each editor's path and caret (`EditorView::serialize`, pane_item.rs
:249), untitled buffers aren't saved at all, and `Workbench::restore_layout` (crates/fe-workbench/src/
workbench.rs:1078) restores panes and tabs only. Day-one audit 2026-10-12 (user: "card the day-one ones"): hot exit
and crash recovery are table stakes. This card is the storage engine; the next wires it.

## Change

New module `crates/fe-buffer/src/backup.rs`, exported from `crates/fe-buffer/src/lib.rs`; no GPUI.

- `BackupStore::new(dir)`: one directory (the app passes `<state_dir>/backups`, i.e. `~/.local/state/fastedit/
  backups` by fastedit's XDG convention, `fe_config::paths::state_dir`). Created on first write, mode 0700.
- `BackupId`: `<unix-millis>-<pid>-<counter>`, unique per process without new dependencies; `Display`/`FromStr`.
- A backup is two files: `<id>.txt` (the text, UTF-8) written first, then `<id>.toml` (the meta) as its commit,
  each through a temp file and rename, mode 0600 (contents may be sensitive). `BackupMeta`: the file's path, or
  none for an untitled buffer plus its tab title; the `DiskStamp` the buffer had; what `SaveSnapshot` needs to
  save it as it would have been (encoding, BOM, line ending); when it was written.
- `write(&self, id, &BackupMeta, &Text) -> io::Result<()>`, `read(&self, id) -> io::Result<Backup>`,
  `remove(&self, id)`, `ids(&self) -> Vec<BackupId>` (complete pairs only), and `prune(&self, keep: impl Fn(&BackupId)
  -> bool)` which also removes half-written pairs.
- `Buffer::from_backup(disk: Option<std::fs::File>, backup: &Backup) -> io::Result<Buffer>` (a `pub(crate)` hook in
  `crates/fe-buffer/src/buffer.rs` if needed): the file's current text as the base (empty when untitled or the
  file is gone), the backup's text applied over it as one transaction with `Origin::Host`, so the buffer is dirty
  and one undo returns to the disk text; its disk stamp is the backup's, so a save of a file changed since the
  backup is caught as any stale save is.

## Tests

New file `crates/fe-buffer/tests/backup.rs` (temp dirs).

## Implementation notes

- The meta is a small TOML subset written and parsed in `backup.rs` (basic
  strings, integers, booleans, one `[disk_stamp]` table), so fe-buffer gains no
  dependency. `dev`/`ino` are written as unsigned decimals, which may exceed
  TOML's i64; only this reader reads them. A path that isn't UTF-8 is stored
  as `path_bytes` (hex) on unix.
- `ids()` and `prune()` return `io::Result`, since reading the directory can
  fail; a store whose directory doesn't exist yet is empty.
- A rewrite removes the old meta (synced) before replacing the text, so an
  interrupted rewrite leaves a text without its meta (unlisted, pruned), never
  new text under old meta. The meta also carries an FNV-1a 64 hash of its
  text, so any pair that isn't one write's reads as `InvalidData`.
- Writes hold a shared `flock` (std `File::lock_shared`) on `<dir>/.lock`, and
  `prune` and `remove` an exclusive one, so neither, from any process or store
  instance, deletes a write in progress's temp files or half-written pair. Under that lock every temp file is an interrupted
  write's; temp names are matched by their full grammar
  (`.<id>.<txt|toml>.<pid>-<n>.tmp`), other files are left alone.
- `BackupMeta::for_buffer` builds the meta from a buffer, including whether
  reading the file replaced malformed bytes with U+FFFD, so a restored
  buffer still refuses to save them back.
- `Buffer` now tracks the encoding as of the last save, as it already did the
  line ending, and `is_dirty` includes it. Nothing but a reload or save
  changes a buffer's encoding otherwise, and those update both.
- `from_backup` keeps a restore record until the next save: while its
  `Origin::Host("backup")` transaction is applied the buffer is dirty whatever
  its text, and undoing or redoing that transaction switches between the
  file's save format (encoding, BOM, line ending, U+FFFD-on-read) and the
  backup's. A backup whose text equals the file's gets an empty transaction,
  so it too restores dirty with one undo back to the file, clean (review
  round 2, F4).
- `from_backup` sets the disk stamp only for a backup with a path, so an
  untitled restore is untitled-style (`disk_stamp_known()` false) and a file
  deleted since keeps the stamp it had.
