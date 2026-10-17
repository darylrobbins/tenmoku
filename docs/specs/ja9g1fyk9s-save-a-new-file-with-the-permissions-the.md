---
task_uid: "01m54dppjp25bkg6ja9g1fyk9s"
task: "T752"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor test a_new_file_gets_the_permissions_the_umask_gives (crates/fe-editor/tests/save_new_file.rs) passes: a buffer saved through EditorModel::save to a path where nothing is gets the same permission bits as a file std::fs::write makes in that folder (0644 under umask 022), with its text; it fails at the base (0600)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test a_new_file_saves_on_an_exfat_volume passes on macOS: a new file saved onto a 4 MB exFAT disk image made and attached with hdiutil (detached again by a Drop guard, panicking rather than skipping if hdiutil fails) holds the buffer's text; it fails at the base with Os code 45, Operation not supported"
    required: true
    manual: false
  - id: "C3"
    text: "Saving over an existing file is unchanged: its temp is still created owner-only and given the original's permissions, an existing target is still committed with a rename, and a file that appears at a missing target before the commit is still kept and reported as SaveError::ChangedOnDisk, by the rename or by create_new in the fallback"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-editor -p fe-buffer passes with no existing test edited; cargo clippy -p fe-editor --all-targets -- -D warnings is clean; no Cargo.toml or Cargo.lock change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Save a new file with the permissions the umask gives instead of owner-only, and on exFAT and other volumes with no exclusive rename instead of failing with Operation not supported

## Problem

Saving a buffer to a path where no file is (an untitled tab's first save, Save As to a new name,
or saving a file deleted since it was opened) goes through `SaveJob::run_with` in
crates/fe-editor/src/disk.rs. Two things go wrong there, both reproduced at Mac main 5837b415 with
the integration test below:

1. **The new file is owner-only, `-rw-------`.** The temp file comes from
   `tempfile::Builder::tempfile_in`, which creates it 0600. `permissions` is `None` when nothing was
   at the target, so nothing widens it, and the rename keeps 0600. A file written by
   `std::fs::write` in the same folder (umask 022) is 0644. VS Code, TextEdit and vim give a new file
   the umask's permissions (0666 less the umask). Seen: `left: "600" right: "644"`.
2. **A new file can't be saved on an exFAT volume at all** (USB sticks, SD cards; FAT and some
   SMB shares likewise). The fresh-file commit is `temp.persist_noclobber(&target)`. tempfile 3.27
   (`file/imp/unix.rs`, `persist`) calls `renameat_with(.., RenameFlags::NOREPLACE)`, which is
   `renamex_np(RENAME_EXCL)` on macOS; exFAT answers `ENOTSUP` (45), and tempfile falls back to
   link-then-unlink only for `EINVAL`, so the save fails with `Os { code: 45, "Operation not
   supported" }`. Saving an existing file there works (plain rename). Reproduced on a 4 MB exFAT
   disk image made with `hdiutil create -fs ExFAT` and attached with `-nobrowse -mountpoint`.

## Change

In crates/fe-editor/src/disk.rs, `SaveJob::run_with`:

- When `permissions` is `None` (nothing at the target), build the temp file with
  `tempfile::Builder::permissions(Permissions::from_mode(0o666))`, so `open(2)` applies the umask,
  and leave the existing-file path (0600 temp, then the original's permissions through the handle)
  as it is. Update `SaveJob::run`'s doc: a new file gets 0666 less the umask; an existing file's
  temp stays owner-only mid-save.
- When the no-clobber commit fails with `ENOTSUP`, `EOPNOTSUPP`, `EINVAL` or `ENOSYS` (the file
  system has no exclusive rename), fall back to writing the target in place, still never replacing
  anything: create it with `OpenOptions::create_new(true)` (`O_CREAT | O_EXCL`, which also refuses a
  symlink there) and std's default mode 0o666 (umask applied), write the
  snapshot, `sync_all`, take the `Saved` stamp from that file's metadata, and drop (delete) the temp.
  `AlreadyExists` from `create_new` is `SaveError::ChangedOnDisk`, as the no-clobber rename's is
  today. If the write or sync fails, remove the file only if its identity (dev, inode) is still the
  one just created, and return the error. Overwriting saves (`persist`) are unchanged.
- Tell "no exclusive rename" apart in a small `fn no_exclusive_rename(error: &io::Error) -> bool` on
  `raw_os_error()`, with the numbers behind `cfg` (macOS: `ENOTSUP` 45, `EOPNOTSUPP` 102, `EINVAL` 22,
  `ENOSYS` 78; Linux: `EOPNOTSUPP` 95, `EINVAL` 22, `ENOSYS` 38), each named in a comment. fe-editor
  gets no new dependency, so neither its Cargo.toml nor Cargo.lock changes.

Regression tests, in a new integration test crates/fe-editor/tests/save_new_file.rs (top-level
fns), using only the base API (`EditorModel::new` with `Buffer::new`, `set_disk_stamp(None)`,
`save`):

- `a_new_file_gets_the_permissions_the_umask_gives`: saves a new file and compares its mode with a
  reference file made by `std::fs::write` in the same tempdir.
- `a_new_file_saves_on_an_exfat_volume` (`#[cfg(target_os = "macos")]`): creates a 4 MB exFAT image
  with `/usr/bin/hdiutil create -quiet -size 4m -fs ExFAT -layout NONE` in a tempdir, attaches it
  with `-nobrowse -mountpoint <tempdir>/mnt`, detaches it by device with `hdiutil detach -force` in
  a `Drop` guard, and saves a new file there; it panics (does not skip) if hdiutil fails. About 2 s
  idle, about 12 s at load 40.
