---
task_uid: "01m4p6c4c1y8w13xs668541rj5"
task: "T17"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-buffer test disk_io::snapshot_writes_like_write_to: a SaveSnapshot written on another thread gives write_to's bytes for UTF-8, CRLF, windows-1252 and UTF-16LE-with-BOM buffers, and the same InvalidData refusals"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test disk_io::saved_snapshot_dirty_state: did_save_snapshot with edits made after the snapshot leaves the buffer dirty and undoing those edits makes it clean; with none it matches did_save"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test disk_io::reload_is_one_edit: reloading a file that changes one middle row keeps anchors in the first and last rows at their offsets, is one undo step of its origin (undo brings back the old text, dirty), adopts CRLF and the encoding, and leaves the buffer clean; identical text makes no edit; from_file records the stamp"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test save_conflict::refuses_a_changed_file: after EditorModel::open, another write (a different length; and the same length with a later mtime) makes save fail with SaveError::ChangedOnDisk and leaves the other writer's bytes; save_overwriting writes; a file deleted since it was read saves; a second save after a save doesn't conflict, and a split model sees the same stamp"
    required: true
    manual: false
  - id: "C5"
    text: "fe-editor test save_conflict::job_off_thread: prepare_save, SaveJob::run on a std::thread and finish_save, with typing in between, leaves the buffer dirty and the file holding the snapshot; crates/fe-editor/tests/save_encoding.rs and cargo test -p fe-buffer pass unchanged"
    required: true
    manual: false
---

# Save from a sendable buffer snapshot with a changed-on-disk check, and reload a buffer from disk as one undoable edit (Mac T431)

Filed on helium from Mac T431 (~/coordination/helium-cards/T431.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-c.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The plan's M6 includes "save/encodings" (`~/.claude/plans/glittery-foraging-graham.md` line 110). The 2026-10-11 gap
analysis (M6 "Files changed on disk + save robustness") found three gaps. User 2026-10-11: "Draft any work in
earlier phases we haven't drafted yet."

- `EditorModel::save` (`crates/fe-editor/src/lib.rs:202`) borrows the buffer, writes and fsyncs on the calling
  thread, and cmd-s runs it on the UI thread (`editor.save`, `crates/fe-editor/src/commands.rs:430`). A 100 MB file
  freezes the window for the whole write.
- It overwrites without checking whether the file changed since it was read, so another program's edits are lost.
- There's no way to reload a buffer from disk. T371's spec assumes a reload builds a new `Buffer`, which would drop
  the undo history, the sharing between split views (`Rc<RefCell<Buffer>>`) and the buffer id that LSP documents
  are keyed by (`crates/fe-workbench/src/lsp.rs` ~800).

This card is the headless half: the APIs and their rules. The next cards use them in the app.

## Change

fe-buffer (`crates/fe-buffer/src/buffer.rs`, after T371; export the new types from `lib.rs`):

- `DiskStamp { len: u64, modified: SystemTime }`, plus device and inode on unix, from `fs::Metadata`;
  `DiskStamp::of(path) -> io::Result<Option<DiskStamp>>` (`None` when missing). The buffer holds
  `disk_stamp: Option<DiskStamp>` with `disk_stamp()` and `set_disk_stamp()`, so split views share it.
- `Buffer::from_file(file: File) -> io::Result<Buffer>`: takes the stamp from the handle's metadata before
  reading, then reads as `from_reader_detecting` does (256 KiB `BufReader`).
- `Buffer::save_snapshot(&self) -> SaveSnapshot`, which is `Send + 'static`: the text (an O(1) rope clone), line
  ending, encoding, the replaced-on-read flag and the version. `SaveSnapshot::write_to(out)` holds today's
  `write_to` body, and `Buffer::write_to` delegates to it.
- `Buffer::did_save_snapshot(&mut self, &SaveSnapshot, stamp: Option<DiskStamp>)`: when the snapshot is the current
  version, as `did_save`. Otherwise it records the snapshot's text as the saved state (T371's `saved_text`,
  `saved_line_ending`, `saved_version`), sets `changed_since_save` to the whole text, so the buffer is dirty until
  T371's check finds it equal again, and emits `Saved { version: snapshot's }`. Either way it sets the stamp.
- `Buffer::reload(&mut self, file: DecodedFile, stamp: Option<DiskStamp>, origin: Origin) -> Option<TransactionId>`:
  replaces the text with `file.text` as one transaction of `origin`, as a single edit over the part that differs
  (common prefix and suffix found by walking both ropes' chunks, with no String of the file), so anchors before and
  after it keep their places. It adopts the file's encoding, line ending and replaced-on-read flag, then marks the
  buffer saved (clean) with `stamp`. Identical text makes no edit but still adopts and marks clean.

fe-editor (new `crates/fe-editor/src/disk.rs`, `mod disk;`; `EditorModel::save`'s body moves here):

- `EditorModel::open` records the stamp from the file it reads.
- `prepare_save(&self, overwrite: bool) -> io::Result<SaveJob>`: the path (NotFound for an untitled buffer, as
  today), a `SaveSnapshot` and the expected stamp. `SaveJob` is `Send`. `SaveJob::run(self) -> Result<Saved,
  SaveError>`: unless `overwrite`, if the file exists and its stamp differs from the expected one (including a file
  that didn't exist when read and does now), it fails with `SaveError::ChangedOnDisk` and writes nothing. Otherwise
  it does today's atomic write (temp file beside the target, owner-only, permissions copied, fsync, rename; a
  symlink's target is updated) and returns the snapshot and the new file's stamp. A file deleted since it was read
  saves without a conflict.
- `finish_save(&mut self, Saved)`: `did_save_snapshot` with the new stamp.
- `save()` stays the synchronous path (prepare, run, finish on the calling thread), mapping `SaveError` into an
  `io::Error` whose message says "changed on disk since it was read" and whose inner error downcasts to
  `SaveError`. Add `save_overwriting()`.
- `reload(&mut self, file: DecodedFile, stamp)`: `Buffer::reload` with `Origin::User` (so cmd-z can take it back),
  then a catch-up; carets and folds follow their anchors.

## Tests

New files `crates/fe-buffer/tests/disk_io.rs` and `crates/fe-editor/tests/save_conflict.rs` (integration tests over
the public API, using tempfile).

## Implementation notes

- A buffer knows its file or doesn't (`Buffer::disk_stamp_known`): `from_file`,
  `set_disk_stamp` (`None` for a file known missing), a save and a reload tell it. A buffer made
  from text or a reader doesn't, and a `SaveJob` for it skips the changed-on-disk check; its first
  save stamps it. `EditorModel::new` doesn't stamp from the file as it is now, which would trust a
  write made after the read. So the app's loaders carry the read's stamp, beyond this card's
  scope: `fe-workbench`'s `loader::read` and `fe-gpui`'s pane-item restore read with
  `Buffer::from_file`, and a missing file opens through `loader::buffer_or_new` as an empty
  buffer that knows its file was missing (`crates/fe-workbench/src/pane.rs`).
- A save through a symlink whose target was deleted recreates the target, following the links
  by hand when `canonicalize` can't, rather than replacing the link with a file.
- `SaveJob::run` checks again after the new file is written and synced, just before the rename:
  the file's stamp must be the one found at the start, and a symlink must still resolve to the
  same target. Only the rename itself is left unguarded.
- `finish_save` returns whether it recorded the save: not when the file on disk is no longer the
  one the job wrote (another save or writer replaced it; saves may finish out of order), nor when
  the buffer already holds that stamp (a reload read it).
- `prepare_save` ends the run of typing in every model of the buffer (a count of prepared saves in
  the shared `History`, checked in `typing.rs`), so the snapshot is an undo step's edge.
- `Buffer::reload` with an open transaction of its origin closes it before the reload and opens a
  new one, as deeply nested, after it; one of another origin is set aside and kept open.
- `EditorModel::open` still reads as UTF-8 (`Buffer::from_reader`), as before; it takes the stamp
  from the handle's metadata before reading.
- `Buffer::read_file(File) -> (DecodedFile, DiskStamp)` is the read behind `from_file`, for a
  reload to use.
- The common suffix is found a 64 KiB window at a time from the end (`Text` only iterates chunks
  forward), copying those windows into two reused buffers rather than the file.
- `did_save_snapshot` with edits since the snapshot runs T371's check over the whole text
  (`refresh_dirty`): dirty at once when the lengths differ, compared when they're equal.
- `fe-buffer` gains `tempfile` as a dev-dependency for `tests/disk_io.rs`.
