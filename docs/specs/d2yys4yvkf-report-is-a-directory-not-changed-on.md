---
task_uid: "01m4qz4xgrf9j59sd2yys4yvkf"
task: "T27"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor test save_not_a_file::saving_over_a_path_that_became_a_directory_reports_is_a_directory (new regression test; fails with T431 alone): after EditorModel::open, replacing the file with a directory makes save and save_overwriting fail with an io::Error of kind IsADirectory whose message contains \"Is a directory\", not SaveError::ChangedOnDisk, and the directory is left as it was"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test save_not_a_file::saving_over_a_fifo_is_an_io_error (unix): a path replaced by a FIFO fails the save with an I/O error that is not ChangedOnDisk, without blocking"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor tests save_conflict (T431) pass unchanged: a regular file whose mtime or size changed still gives SaveError::ChangedOnDisk, and a deleted file still saves"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-editor -p fe-buffer -p fe-workbench and cargo clippy --workspace --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Report Is a directory, not changed-on-disk, when saving over a path that became a directory (helium T17 = Mac T431 follow-up) (Mac T464)

Filed on helium from Mac T464 (~/coordination/helium-cards/T464.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-j.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

T431 (helium T17, fa4cc18) added a changed-on-disk check to saving: `SaveJob::run_with` in
`crates/fe-editor/src/disk.rs` compares `DiskStamp::of(&path)` with the stamp recorded when the file was read,
and fails with `SaveError::ChangedOnDisk` ("the file changed on disk since it was read") when they differ.

That check runs before anything else, so when the path has since become a directory (or any other
non-regular file), the save reports "changed on disk" instead of the OS error. Before T431 the save reported
"Is a directory", as vim does (E502). The Mac's vim `:wa` test
`workbench::vim_host_tests::write_all_reports_the_first_failure` (crates/fe-workbench/src/vim_host_tests.rs,
from Mac T357's follow-ups) relies on that: it replaces b.txt with a directory and expects the `:wa` message
to contain "Is a directory". With T431 on top of Mac main it fails deterministically at line 553.

Decision (Mac lead): the test is right.

## Change

In the save path (`SaveJob::run_with`, and `DiskStamp::of` in fe-buffer if the check belongs there):

- If the path (after resolving symlinks, as the save already does) exists and is not a regular file, the save
  fails with the I/O error for that path, before the changed-on-disk check, writing nothing. For a directory
  that is `io::ErrorKind::IsADirectory`, whose message contains "Is a directory" (EISDIR); for any other
  non-regular file (FIFO, socket, device) an I/O error that is not `ChangedOnDisk`. `save_overwriting` behaves
  the same: overwriting never replaces a directory.
- `SaveError::ChangedOnDisk` applies only when the path is a regular file whose stamp (mtime/size) differs from
  the one recorded at read time, as now. A file deleted since it was read still saves. The re-check after
  writing the temp file is unchanged except that it too reports a path that became a non-regular file as
  that I/O error, not as `ChangedOnDisk`.
- No change to the vim host, the workbench or the test in vim_host_tests.rs.

## Notes

- `vim_host_tests::write_all_reports_the_first_failure` arrived on the Mac with neon batch 7 (cc29a007). If
  your trunk doesn't have it yet, sync mac/main first so C1 can run.
- New focused tests go in `crates/fe-editor/tests/save_not_a_file.rs`.
