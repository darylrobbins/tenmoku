# fe-host, phase 1a: choices and one deviation from D § 7 (T722)

The decision is `docs/decisions/j8wvgszah0-remote-development.md` (D). This
card adds `crates/fe-host` with `DiskStamp`, `FsHost`, `WriteExpect`,
`Conflict`, `HostSet` and `LocalFs`, and moves fe-editor's save commit and
the loader's read into `LocalFs`.

## Deviation: `Absent` follows a link present when the write starts

D § 7 has `WriteExpect::Absent` as "nothing, not even a symlink, may be at
the path", for both a new file (`SaveJob.expected == Some(None)`) and a Save
As destination (`new_entry`). Before this card the two differed in one case,
a symlink to nothing at the path when the save starts:

- `Some(None)`, a missing file the buffer opened (through a dangling link,
  say), wrote through the link and created its referent, no-clobber. fe-editor
  `no_clobber_tests::an_ordinary_save_through_a_dangling_symlink_creates_its_referent`
  pins this.
- `new_entry` (Save As) committed at the path itself, so the link made the
  no-clobber rename fail: `ChangedOnDisk`, and the overwrite prompt.

A four-case `WriteExpect` can't carry both, and phase 1 changes no
behaviour (D § 5, C5). Lead decision (2026-11-20):

- `Absent` in fe-host follows a symlink that is at the path when the write
  starts, requires the file it names to be missing, and commits no-clobber
  at that file. Anything that appears at the path, or at that file, before
  the commit is a `Conflict`.
- The Save As refusal stays out of `WriteExpect`. As first built it was a
  caller-side check in `SaveJob` (a symlink to nothing at a `new_entry`
  path: `ChangedOnDisk`), but review round 1 (F2) found the window between
  that check and the commit's first look at the path: a dangling link made
  there was followed. So it moved into the local commit as an option,
  `CommitOptions::at_the_path`, which `SaveJob` sets for `new_entry`. The
  commit then lands at the path itself, as before this card, so the
  no-clobber rename refuses a link there whenever it appeared. That is
  today's behaviour exactly, race included. `FsHost::write_atomic` has no
  such option yet. What a remote host does for a Save As is for the card
  that sends a `new_entry` save over the wire (phase 4 or 9): carry the
  option, or a fifth case.

Tests: `crates/fe-host/tests/local_fs_absent_dangling_link.rs` (Absent
follows the link; a referent created before the commit is kept),
`crates/fe-host/tests/local_fs_at_the_path.rs` (a dangling link at a Save As
path, there at the start or made at the boundary hook before the re-check,
is refused) and `crates/fe-editor/tests/save_as_dangling_link.rs` (Save As
is refused, and an overwriting one writes through), the last added to the
scope by respec, since a fe-host test that drives `SaveJob` would need
fe-host to dev-depend on fe-editor (C1).

## `Matches` and a file deleted since: strict in `FsHost`, kept for saves

D § 7 has the host compare the current stamp with a `Matches` stamp before
writing and again before the rename, so a file deleted since the stamp was
taken is a conflict, and `FsHost::write_atomic` does exactly that. Before
this card, though, `SaveJob` saved a file deleted since it was read ("A
file deleted since is saved", its doc), and tests pin it: fe-editor
`tests/save_conflict.rs` ("Deleted since it was read: saves.") and
fe-workbench `disk_sync_tests.rs` ("The file deleted, its stat held, then
recreated by a save"). Phase 1 changes no behaviour (D § 5, C5).

Review round 1 (F1) asked for the strict contract; round 2 accepted that
the save behaviour must stay and asked for it to be carried apart from the
contract. So `CommitOptions::deleted_is_written` makes a local `Matches`
commit write a missing file, no-clobber, and `SaveJob` sets it. A file
made at the path before the commit is still refused, by the re-check or the
no-clobber rename. Whether a queued remote save of a deleted file should be
refused is phase 9's, with its queued saves; the wire has only the strict
`Matches` until then. Tests: `crates/fe-host/tests/local_fs_matches_deleted.rs`.

## Other choices

- **Errors.** `write_atomic` returns `Result<DiskStamp, WriteError>`, where
  `WriteError` is `Conflict(Conflict { current })` or `Io(io::Error)`;
  `current` is the file at the path, symlinks followed, as the host found
  it on refusing. fe-host holds no UI text: the three failures with no
  `io::ErrorKind` of their own (a directory where there's no `EISDIR`, not
  a regular file, a symlink loop) carry a `PathError`, and fe-editor words
  them with its existing catalogue keys, so messages and kinds are as
  before.
- **Streaming.** `FsHost::write_atomic` takes bytes, as D § 5 has it.
  `SaveJob` calls `LocalFs::commit` instead, which streams the snapshot
  into the temp file (and again into the file itself on a volume with no
  exclusive rename), so a large save isn't encoded into memory first.
  `write_atomic` is `commit` of a byte slice: one code path.
- **Boundary hooks.** `CommitOptions` (`before_commit`, after the temp file is
  synced and before the re-check; `before_persist`, after the last check
  and just before the rename) replace `SaveJob::run_with`'s two closures,
  which now pass through to them.
- **Reads.** The loader reads bytes with `LocalFs.read` (stamp from the open
  handle before reading, as `Buffer::read_file` took it) and decodes them
  with new `Buffer::from_bytes`, `from_bytes_with`, `read_bytes` and
  `read_bytes_with`. The `.editorconfig` charset path detects from the same
  bytes rather than seeking the file back.
- **The rest of `FsHost`.** `stat` follows symlinks and gives a `FileKind`
  and a `DiskStamp`; `list_dir` gives each entry's name and kind (links not
  followed); `canonicalize` is `dunce::canonicalize`. `create`, `rename`,
  `remove` and `trash` arrive with the fe-project listing and file-op
  sites. `FsHost` is synchronous: callers already run reads and saves off
  the UI thread.
- **`HostSet`** has `fs: Arc<dyn FsHost>` and `HostSet::local()`; the
  workbench doesn't hold one yet (card 1a-wb), so the loader and `SaveJob`
  call `LocalFs` directly. `ProjectLocation` (1f) goes in fe-host's root.
- The macOS-only `a_creation_date_before_1970_is_kept` unit test moved with
  `keep_creation_date` from `disk.rs` into `fe-host/src/local.rs`.
