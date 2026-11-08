---
task_uid: "01m6y59cfn17dkyzgmnk4tnjrw"
task: "T1616"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "discard in crates/fe-project/src/fs_ops.rs checks and removes the entry where it is when rename_to_temp fails with PermissionDenied, as its Unsupported branch already does (an unreadable folder is removed only if empty; the ownership checks are unchanged); every other rename error still returns without removing anything; the doc comment says so"
    required: true
    manual: false
  - id: "C2"
    text: "fe-project test fs_ops::race_tests::cleanup_leaves_a_folder_swapped_in_after_its_check_as_it_is first probes the platform in a temp folder (a 0o500 folder renamed within its parent, mode restored after). If that rename works, today's assertions are unchanged; if it fails with PermissionDenied, it asserts the root holds exactly ours, src and one .fastedit-discard-* entry, that entry has mode 0o500 and is empty, and ours holds a.txt and b.txt; any other probe error fails the test. The hand-in records which branch ran on this Mac"
    required: true
    manual: false
  - id: "C3"
    text: "fe-project tests create_removes_its_folders_under_a_restrictive_umask and duplicate_works_under_a_restrictive_umask keep every assertion and pass, and cargo nextest run -p fe-project passes"
    required: true
    manual: false
  - id: "C4"
    text: "Only crates/fe-project/src/fs_ops.rs and crates/fe-project/src/fs_ops_race_tests.rs change (crates/fe-project/tests/fs_ops.rs at most in comments), and cargo clippy -p fe-project --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# CI (macos-15): fe-project discard checks a folder in place when renaming it is refused, so the restrictive-umask and swapped-folder fs_ops tests pass on GitHub's macOS runner

# CI (macos-15): fe-project's discard leaves a folder behind when renaming it is refused, so three fs_ops tests fail on GitHub's macOS runner

## Evidence

GitHub CI's `check` job (runner image macos-15-arm64, macOS 15.8.2) fails three fe-project tests on every
push to main, including the Windows-port merge aa9e4988b (run 42503331207) and every run on 2026-11-08
before it. They pass on this Mac (macOS 27) and on CI's Linux job.

    FAIL fe-project fs_ops::race_tests::cleanup_leaves_a_folder_swapped_in_after_its_check_as_it_is
      panicked at crates/fe-project/src/fs_ops_race_tests.rs:345:5
      left:  [".fastedit-discard-78156-0", "ours", "src"]
      right: ["ours", "src", "src copy"]

    FAIL fe-project::fs_ops create_removes_its_folders_under_a_restrictive_umask
    FAIL fe-project::fs_ops duplicate_works_under_a_restrictive_umask
      (both: the _child panics at crates/fe-project/tests/fs_ops.rs:29:40, `fs::read_dir(dir).unwrap()` in
      `snapshot`: Os { code: 13, kind: PermissionDenied }; a folder made under umask 0o700, mode 000, was
      left in the root)

All three came in with T59/T64 (335f2eb5f, 4fa734e15, 2026-10-12/13). CI on main has been red since
before then for other reasons, so they were never seen.

## Cause (best reading; this Mac can't reproduce it)

All three go through `discard` in crates/fe-project/src/fs_ops.rs. It first renames the entry to
`.fastedit-discard-<pid>-<n>` (`rename_to_temp`, `renameatx_np(RENAME_EXCL)`) and renames it back
(`restore`) when it isn't ours. Both outcomes on CI are what happens if macOS 15 refuses to rename a folder
whose owner has no write permission, even within the same parent:

- umask tests: `mkdir` under umask 0o700 makes a mode-000 folder; `rename_to_temp` fails with something
  other than `Unsupported`, so `discard` returns at `Err(_) => return` and the folder stays.
- race test: the folder the test swaps in at the staged name is mode 0o500; `restore` can't rename it
  back, logs "left ... as ...", and the root keeps `.fastedit-discard-*` instead of `src copy`.

Checked on this Mac (macOS 27.0): `mkdir a; chmod 500 a; mv a b` succeeds (a move to another parent fails
with EACCES), which is why the gate passes. Don't take the macOS 15 rule as proven; the code change below
is right whatever the error.

## Change

1. `discard` (fs_ops.rs): when `rename_to_temp` fails with `PermissionDenied`, check and remove the entry
   where it is, as the existing `Unsupported` branch already does (`staged = name.to_owned()`). A folder
   that can't be renamed can't be written either, so it is only removed if it is empty (`unlinkat` with
   `AT_REMOVEDIR` needs only the parent's write access), and the ownership checks stay as they are. Update
   the doc comment to say so. Other errors still return without removing anything.
2. `cleanup_leaves_a_folder_swapped_in_after_its_check_as_it_is` (fs_ops_race_tests.rs): the restore of a
   folder its owner can't change may be refused by the platform, and nothing may change that folder's
   permissions to get round it. Before running the scenario, the test checks the platform in a temp
   folder (make a folder, set it to 0o500, rename it in the same parent, put its mode back). If that rename
   works, keep today's assertions unchanged. If it fails with PermissionDenied, assert the other outcome:
   the root holds exactly `ours`, `src` and one `.fastedit-discard-*` entry, which is the swapped-in folder
   with mode 0o500 and nothing in it (its permissions not touched), and `ours` still holds `a.txt` and
   `b.txt`. Any other rename error fails the test.
3. The two umask tests in crates/fe-project/tests/fs_ops.rs stay as they are. With step 1 they hold on both
   macOS versions; edit them only if a macOS-specific comment there becomes wrong.

## Do not touch

Anything else in fs_ops.rs (rename/trash/duplicate/create logic, `own_dir`, `own_closed_dir`, the
Linux and Windows paths), the other race tests, or any assertion in the umask tests. No CI config change.

## Note on regression tests

This is a portability fix for a macOS 15 behaviour the macOS 27 gate can't show, so no test fails at the
task base here and the card has no regression tests. The proof is CI's `check` job on the first push of
main after landing, which the lead checks (a lane can't run GitHub CI, so it isn't a criterion). Record in
the hand-in what the platform probe in step 2 found on this Mac.

## Implementation notes

- Platform probe on this Mac (macOS 27): the rename of a 0o500 folder within its parent succeeds
  (`platform renames a 0o500 folder: true`, observed with `cargo nextest run --no-capture`), so the test
  ran today's assertions unchanged. The PermissionDenied branch has not run here.
