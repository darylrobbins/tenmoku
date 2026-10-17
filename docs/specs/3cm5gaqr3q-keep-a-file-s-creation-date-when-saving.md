---
task_uid: "01m5556qnvfzmtvm3cm5gaqr3q"
task: "T786"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor::save_creation_date a_save_keeps_the_files_creation_date passes: a file dated 2020-01-02 by its birth time, edited and saved through EditorModel::save, reads the new text and keeps metadata().created(), as FileManager.replaceItemAt does; it fails at the base (created() is the save's time)"
    required: true
    manual: false
  - id: "C2"
    text: "The creation date is set on the temp before its DiskStamp is taken and before persist, only on macOS and only when the target existed; a failure to set it is logged and the save still succeeds; fe-editor::save_xattrs a_save_keeps_the_files_extended_attributes and the save_conflict tests pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-editor passes with no existing test edited; cargo clippy -p fe-editor --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Keep a file's creation date when saving it on macOS, as FileManager's safe save does, instead of giving it the time of the last save

## Problem

A save replaces the file by renaming a temp over it (crates/fe-editor/src/disk.rs `SaveJob::run_with`:
`tempfile_in` beside the target, `copy_xattrs` and `set_permissions` from the original, then `persist`).
The temp is a new inode, so the file's creation date (APFS birth time, Finder's "Created" and Get Info,
`stat -f %SB`) becomes the time of the last save. macOS's own safe save keeps it: Apple documents that
`FileManager.replaceItemAt(_:withItemAt:)`, the Foundation call for a safe save, preserves "the creation
date, permissions, Finder label and color, and Spotlight comments of the original item". Checked on this Mac
with a Swift script: a file dated 2020-01-02 03:04 keeps that creation date through `replaceItemAt`, and gets
today's through a plain `String.write(atomically: true)` rename, which is what fastedit does now. fastedit
already keeps permissions and extended attributes (Finder tags and colours live in xattrs;
tests/save_xattrs.rs); the creation date is the one left.

Reproduced at Mac main 3e3eaa03 with a new integration test: a file whose birth time is moved to 2020-01-02
(`touch -t 202001020304`, which moves APFS's birth time back with the mtime), opened with
`EditorModel::open`, edited and `save()`d, reads its new text but `metadata().created()` is the save's time
(`left: tv_sec 1792248189`, `right: tv_sec 1577952240`).

## Change

In crates/fe-editor/src/disk.rs, where `run_with` copies the original's xattrs and permissions onto the temp
(only when the target exists), also give the temp the original's creation date on macOS: read
`std::fs::metadata(&target)?.created()` and set it on the temp's descriptor with `fsetattrlist` and
`ATTR_CMN_CRTIME` (declared with `extern "C"` beside `fcopyfile`, an `attrlist` with `bitmapcount = 5`,
`commonattr = ATTR_CMN_CRTIME` (0x200) and a `timespec` buffer). It runs before the temp's `DiskStamp` is
taken, so the stamp stays the committed file's. A failure is logged with `log::warn!` and the save goes on,
as `copy_xattrs` does. Other platforms are unchanged. A brand-new file (no target) gets its own date as now.

New test file crates/fe-editor/tests/save_creation_date.rs (`#![cfg(target_os = "macos")]`), test
`a_save_keeps_the_files_creation_date` as above: it asserts the file was dated more than a day back, saves,
and asserts the new text and the unchanged `created()`. A prototype of exactly this test fails at 3e3eaa03.
