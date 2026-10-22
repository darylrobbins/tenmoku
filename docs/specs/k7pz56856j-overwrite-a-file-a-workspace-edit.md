---
task_uid: "01m5fk86hz0hc6axk7pz56856j"
task: "T211"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-project tests of the truncating open pass: it empties a file under the root, refuses a final symlink and an ancestor swapped for a symlink, and keeps mode and group"
    required: true
    manual: false
  - id: "C2"
    text: "a fe-workbench ops test passes: an overwriting create keeps the file's inode, mode and a user.* xattr (skipped without xattr support); it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-project -p fe-workbench passes and cargo clippy -p fe-project -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Overwrite a file a workspace edit creates over by truncating it through a symlink-safe handle, keeping its owner, ACLs and xattrs (T188 F9)

Follow-up of T188 review finding F9 (rounds 2 and 3, codex), filed at the lead's request.

## Problem

A workspace edit's `CreateFile` with `overwrite` on an existing file (`apply_operation`, `crates/fe-workbench/src/workspace_edit.rs`) moves the file to the Trash and creates it again through `fs_ops`. It then restores the permission bits (`0o777`) with `fchmod` through `fe_project::open_file_under`. The file's group, ACLs and extended attributes (macOS quarantine, Finder tags) are lost, where truncation would keep them. For example, a 0640 file with a group other than the user's default ends up readable by a different group. Path-based truncation was refused in review (F1), because an ancestor swapped for a symlink could lead it outside the project, and fe-project has no symlink-safe open for writing.

## Change

Add to `fe_project` (`crates/fe-project/src/fs_ops.rs` or `listing.rs`) a symlink-safe open for writing under the root: through the same directory handles `open_file_under` uses, `O_WRONLY | O_TRUNC | O_NOFOLLOW`, refusing a non-regular file. Have the overwriting create truncate through it instead of trashing and re-creating, so ownership, ACLs and xattrs stay. didCreateFiles is no longer sent for an overwrite, which empties an existing file.

## Tests

- fe-project: the truncating open empties a file under the root, refuses a symlink at the last component and an ancestor swapped for a symlink (the race hook `fs_ops` tests use), and keeps the file's mode and group.
- fe-workbench (`workspace_edit_ops_tests.rs`): an overwriting create keeps the file's inode, mode and an extended attribute (Linux `user.*`, skipped where the filesystem has none).

## Implementation notes

- The open is `fe_project::fs_ops::open_truncated(root, path)`, beside the other operations: the path resolves through `Root::resolve`'s handles, and a new `Point::Truncating` (after the folder is reached, before the file is opened) is where the race tests swap an ancestor or the file itself for a symlink.
- It opens `O_WRONLY | O_NOFOLLOW | O_NONBLOCK | O_NOCTTY` without `O_TRUNC`, checks the handle is a regular file, and only then `ftruncate`s it, so a FIFO or device that appears there is refused before anything is done to it (`O_TRUNC` would act at open). A symlink or non-regular file fails with `InvalidInput`, a folder with `IsADirectory`, a missing file with `NotFound` (it never creates).
- The overwriting create truncates only an existing regular file. Anything else there (a symlink, which could lead outside the project, or a FIFO) is still moved to the Trash and a file made in its place, with `didCreateFiles` sent, as before.
- A later text edit of the emptied file is still written by the atomic save, which makes a new inode as every save does; the C2 test checks the create alone.
