---
task_uid: "01m79q7r0r7a3fmzcnqszcveed"
task: "T1778"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fs_ops_substitution (cargo nextest run -p fe-project --test fs_ops_substitution) passes: a replacement injected between making a folder or symlink and recording it is never recorded as made; a restore whose published entry differs from the validated one reports failure and moves nothing back"
    required: true
    manual: false
  - id: "C2"
    text: "fs_ops_undo and the T1754 tests::project_undo tests pass unchanged"
    required: true
    manual: false
---

# fs_ops: narrow same-user substitution races when recording created, copied and restored entries (T1754 F3/F7 follow-up)

# fs_ops: narrow same-user substitution races in create, copy and restore (T1754 F3/F7 follow-up)

T1754 (project tree undo) records the identity of what fs_ops creates, copies and restores, and Undo acts on it. Codex review of T1754 (F3 high, F7 medium) found two windows where another process of the same user can swap an entry: between mkdir_at/symlink_at and the stat_at that records a made folder's or symlink's identity (F3), and between validating a staged restore and publishing it by name (F7). Lead decision 2026-11-13: same-user races are outside fs_ops's threat model and T1754 ships them as a known limit. No POSIX or macOS call binds a rename to a checked identity (renameat, renameatx_np take names), so these can only be narrowed, not closed.

## Change
- Narrow F3: take a made folder's or symlink's identity from a handle opened on what was made (O_NOFOLLOW / O_DIRECTORY open of the new entry, or fstat on the fd that made it where the platform allows), not from a later by-name stat; record nothing when they disagree.
- Narrow F7: after publishing a restore, compare the published entry's identity with the validated one before reporting success, and on a mismatch leave the entry where it is (never move an arbitrary occupant back), report the restore as failed with a toast, and drop the history entry.
- Deterministic tests that inject a replacement at each boundary through a test seam in fs_ops (no sleeps, no real Trash: use the test trash seam).

## Out of scope
Closing the races outright (no API for it). Changing T1754's history shape.

## Implementation notes

- No call makes a folder or symlink and returns a handle on it, so the window between `mkdirat`/`symlinkat` and the by-name lookup stays. What narrows it: a made entry is looked up by name (a folder must be a folder there: `made_dir_id`) and counts as made only once a handle opened on it agrees. For a folder, `confirm_made_dir` / `own_made_dir` check the handle is that identity and still **empty**, before any permission change; one its owner can't read can't be told empty and fails (`PermissionDenied`). For a symlink, `check_made_link` opens the link itself (`O_SYMLINK` on macOS, `O_PATH|O_NOFOLLOW` on Linux), requires a symlink with `nlink == 1`, and re-reads its target and identity by name. A disagreement fails the operation with `AlreadyExists`. What a same-user process can still slip in is an empty folder or a fresh link to the same place: losing that to Undo's Trash is harmless. Applies to `create` (every folder made, the last one included), the copy's folders and symlinks, and `restore`'s private staging folder.
- Nothing unconfirmed is ever recorded, used, or added to the recursive cleanup set (review r1 F1/F2). When confirming fails for an ordinary reason (`EMFILE`, no access), the folder is removed only by `rmdir` and only while it's still a folder with the looked-up identity (`drop_unconfirmed_dir`), so a full occupant survives and its permissions never change (F3: the leaf folder of a create is removed too, and its new ancestors with it). The one exception is Linux's `own_closed_dir` path for a copy under a umask that takes the owner's read access: it has to give the folder its owner's access before it can be read.
- A symlink that isn't confirmed is never added to the copy's owned set (so never unlinked by cleanup): a file swapped in for it would otherwise be unlinked. The partial copy is then kept rather than removed.
- `restore` confirms what landed after the publish rename: success only when the by-name entry there is the validated identity. Anything else (a mismatch, or a lookup that fails) leaves the entry where it is, removes the staging folder only if empty, sets `Trashed::location` to `None` and fails (`NotFound`, or the lookup's error). The workbench's `put_back` then toasts the failure and drops the history entry (`in_trash()` is `Ok(false)`); no workbench change was needed.
- Test seam: `fs_ops::testing::{at, fail_at, reached, Boundary}` (`src/fs_ops/seam.rs`, `cfg(test)` or `test-support`), a thread-local one-shot hook at `Boundary::Made`, `Confirming`, `Staged` and `Published` (`fail_at` fails the next step with `EMFILE`); a hook may re-arm itself to reach the second boundary of an operation. The unit `race_tests` hook is unchanged.
