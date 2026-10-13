---
task_uid: "01m4s2vd145x2a2srzrbra28g0"
task: "T64"
rev: 4
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fs_ops_race_tests: a copy whose top folder or a child folder is replaced (a different device and inode) before permissions are set leaves the replacement's permissions unchanged and reports the error; permissions go through the checked descriptor (F11)"
    required: true
    manual: false
  - id: "C2"
    text: "fs_ops_race_tests: failure cleanup and creation and Trash rollbacks keep entries another process inserted or swapped in before cleanup checks them (F13; the check-to-unlink window is out of scope, see Not in scope)"
    required: true
    manual: false
  - id: "C3"
    text: "fs_ops_race_tests: with statx failing or STATX_MNT_ID missing, a bind-mounted project's Trash and mount-root walk use the mount identity from /proc/self/fdinfo (Linux, skipped with a message without a bind mount) (F9)"
    required: true
    manual: false
  - id: "C4"
    text: "a folder a copy or create makes is rolled back when a later open or metadata call fails, including under a restrictive umask (round 1 F5)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-project and cargo clippy -p fe-project --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Harden fe-project file operations against concurrent replacement and unidentified mounts (T59 review F9, F11, F13)

Follow-up from T59 (Mac T510), filed on helium. T59's review went 3 rounds, and the lead respecced it to put
these three medium findings out of its scope: they harden fe-project's file operations against other processes
racing them, and against mounts statx can't identify. Neither is in Mac T510's spec. Held in the inbox until the
Mac lead says whether helium or the Mac takes it.

## Problem

Review round 3 of T59, candidate 01m4s23mpdjvtxtksy0vmww67y (rgt review show T59), found:

### F9 (crates/fe-project/src/fs_ops.rs:433)

The bind-mount defect remains when statx fails or omits STATX_MNT_ID: Volume::of silently records None, and Volume::same then treats device equality as mount equality. Home Trash selection can still fail with EXDEV, and the volume-root walk can cross a bind-mount boundary. STATX_MNT_ID is unavailable before Linux 5.8, and the API permits requested fields to be absent ([statx documentation](https://man7.org/linux/man-pages/man2/statx.2.html)).

Fix: Obtain missing mount identity from validated descriptor information, such as /proc/self/fdinfo, rather than falling back to device equality; explicitly handle unavailable identity and test both failed statx and missing-mask cases on a bind mount.

### F11 (crates/fe-project/src/fs_ops.rs:643)

Deferred permissions now reopen directories by name without checking their identities. If another operation replaces the copy's top directory or a child after copying, this code applies the source's permissions to the replacement, potentially exposing private contents, and can report success. Previously, retained handles applied permissions only to the directories actually copied.

Fix: Record each copied directory's identity alongside its path and mode, verify reopened handles before changing permissions, and retain a bounded anchor for the copy. Add deterministic top-directory and child-directory replacement tests.

### F13 (crates/fe-project/src/fs_ops.rs:857)

Existing failure cleanup has siblings of the replacement-deletion race from F6. It removes entries by name without establishing that they still belong to the operation, and recursive cleanup deletes unexpected children. In particular, copy_new detects a concurrently inserted destination child as Taken, then remove_copy permanently deletes that child's contents. Creation rollback and Trash metadata rollback can likewise remove replacements.

Fix: Track operation-owned entries and preserve concurrent additions or replacements during cleanup; use protected staging where necessary to avoid check-then-unlink races. Add deterministic inserted-child and replacement cleanup tests.

## Change

In crates/fe-project/src/fs_ops.rs, as each finding's fix says:
- F11: record each copied folder's identity with its path and mode, and check a reopened handle against it before
  setting permissions, without keeping one descriptor per folder (T59's F8).
- F13: cleanup after a failure removes only entries the operation created, and keeps entries other processes added
  or swapped in.
- F9: when statx doesn't report STATX_MNT_ID, take the mount identity from /proc/self/fdinfo instead of comparing
  devices; when neither works, say so rather than assume one mount.

## Tests

New file crates/fe-project/src/fs_ops_race_tests.rs, with deterministic hooks (no sleeps): a copy's top folder and a
child folder replaced before permissions are set; a child inserted into a copy's destination before cleanup; a
creation rollback and a Trash rollback whose entry was replaced; and, gated to Linux with a bind mount available
(skipped with a message otherwise), mount identity with statx failing and with STATX_MNT_ID missing.

## Not in scope (rev 2)

Review round 1 of T64 (rgt review show T64, candidate 01m4s8735d90mg2c4xa8z8aqfq) asked for more than F9, F11 and
F13. The bar for this task: check an entry's identity (device and inode) when fe-project acts on it, act through the
handle it checked where the API offers one, and fail safe (keep the entry, report the error) when the check fails.
That is already more than Zed or VS Code do. These are out of scope; dispute findings that ask for them, citing this
section:

- Protected or descriptor-anchored staging (a private staging folder, or renaming into one) to close the window
  between checking an entry and removing, renaming or publishing it by name (round 1 F1's staging part, F2). A process
  that swaps an entry in during that window can win.
- Inode numbers reused after a replacement (round 1 F3). Device and inode identify an entry; no generation number,
  birth time or other incarnation identity on top.
- Hard links another process adds to entries the operation created (round 1 F4). Cleanup may remove a name that
  points at an inode the operation created.

In scope from round 1:
- Permissions are set through the descriptor that passed the identity check (fchmod on that handle), never by name
  again (round 1 F1's permissions part).
- F5: a folder the operation creates is recorded for rollback as soon as mkdir succeeds, so when a later open or
  metadata call fails (for example under a restrictive umask), rollback removes it. No protected creation.
- crates/fe-project/tests/fs_ops.rs: the F9 regression test, which must use the public API so it compiles at base.

## Implementation notes

- Races are run at `fs_ops::Point`s through a test-only hook (`fs_ops_race_tests::at`), which also replaces
  T59's `AFTER_CHECK` (now `Point::Checked`).
- F11: `Copying` records each copied folder's path, identity and mode, and holds the copy's top open (its one
  anchor). `set_modes` walks from that anchor, checks each reopened folder's identity before `set_permissions`,
  then checks the copy's name still leads to the anchor; a mismatch fails with `AlreadyExists`.
- F13: `discard` replaces `remove_copy` and the by-name unlinks in `create`'s rollback and the `.trashinfo`
  rollback. It removes only identities the operation recorded (`Owned`), renaming each entry to a private
  `.fastedit-discard-*` name before re-checking it, and renames back anything not its own or not emptied. A
  file system without an exclusive rename is checked in place. A folder that keeps another's entry is kept.
- F9: `Volume` always carries a mount id: `statx`'s, else `/proc/self/fdinfo`'s `mnt_id` (read only from a
  procfs, and checked against the descriptor's `ino` where fdinfo has one). With neither, `Volume::of` fails
  with `Unsupported`, so the trash refuses rather than compare devices.
- The bind-mount test runs its child under `bwrap`, as `tests/fs_ops.rs` does, and skips with a message when
  bubblewrap can't make the mount.
- The regression test is `a_bind_mounted_project_trashes_on_its_own_mount_without_statx` in
  `crates/fe-project/tests/fs_ops.rs` (outside the scope): the regression check runs the named tests against the
  base code, where the hook-based race tests don't exist. It needs no hook: its `bwrap` child denies `statx` with a
  seccomp filter, so at the base the bind-mounted project's trash picks the home trash and fails with `EXDEV`.
- Review round 1: permissions are set only through a descriptor checked to be the folder (`own_dir`): the
  handle opened, or, for a folder its owner can't read, an `O_PATH` descriptor whose `/proc/self/fd` link takes the
  `chmod` (Linux). Elsewhere (macOS) nothing opens such a folder, so it is left as it is and the step fails with
  `PermissionDenied`: under a umask that takes a new folder's owner access, `duplicate` fails there and leaves
  nothing, where Linux copies. A folder made is recorded for rollback as soon as `mkdir` succeeds, with its
  identity looked up in its parent before anything that can fail opens it, checked on that open and before
  removal; one that can't be looked up is kept. Cleanup removes a folder it can't open only when empty.
  `fs_ops::Point::Opening` lets a test fail the open that follows a `mkdir`.
