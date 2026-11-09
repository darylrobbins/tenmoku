---
task_uid: "01m6y59gje2sh428t8y6ky7pvz"
task: "T1619"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "On Windows only (cfg(windows)), RecentList::load in crates/fe-config/src/recent.rs retries a read that fails with PermissionDenied or raw error 32 (sharing violation) up to 10 times 10 ms apart, then returns the error as before; NotFound and other errors, and every non-Windows path, are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "On Windows only, save's fs::rename gets the same bounded retry through the same helper and still removes its temp file on the final failure"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test loads_during_concurrent_saves_never_fail (new, crates/fe-config/tests/recent.rs): 8 threads x 500 save/load pairs, every load error-free and equal to what was saved; it passes on macOS (CI's windows job runs it after the push)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-config passes on macOS, concurrent_saves_never_expose_partial_contents included and unchanged; cargo clippy -p fe-config --all-targets -- -D warnings is clean, and so is cargo clippy -p fe-config --all-targets --target x86_64-pc-windows-msvc -- -D warnings (type-checks the cfg(windows) code; the hand-in quotes its last line)"
    required: true
    manual: false
---

# Windows: RecentList::load retries a read refused while another save renames over recent.toml (fe-config concurrent_saves_never_expose_partial_contents fails on CI)

# Windows: RecentList::load fails while another window's save renames over recent.toml (fe-config concurrent_saves_never_expose_partial_contents fails on CI)

Platform: the bug is Windows-only and neither macOS nor Linux reproduces it, but this card runs on the Mac (user, 2026-11-08):
type-check the Windows code with the installed `x86_64-pc-windows-msvc` target, and GitHub CI's `windows` job after the
push is the proof that it is fixed (as for T1616). So it is a chore with no base-failing regression test.

## Evidence

GitHub CI's `windows` job on the Windows-port merge aa9e4988b (run 42503331207):

    FAIL fe-config::recent concurrent_saves_never_expose_partial_contents
      thread '<unnamed>' panicked at crates\fe-config\tests\recent.rs:190:21: assertion failed: err.is_none()
      thread 'concurrent_saves_never_expose_partial_contents' panicked at crates\fe-config\tests\recent.rs:197:18

It passed on the windows-port branch's last run (42491561875), so it is intermittent. The test (4 threads,
each saving and loading 50 times) came in with T56 (cd910bcbc); the code is crates/fe-config/src/recent.rs.

## Cause

`RecentList::save` writes a temp file and `fs::rename`s it over `recent.toml` (MoveFileExW with
REPLACE_EXISTING on Windows). On Windows, opening a file while another process or thread is replacing it
can fail for a moment with ERROR_ACCESS_DENIED (or ERROR_SHARING_VIOLATION) rather than open the old or
the new file, as it would on Unix. `RecentList::load` returns that as an error and an empty list, so on
Windows a window reading the recent list while another window records an entry logs an error and shows an
empty list (and `recent::update`, which loads under the lock before saving, could act on that empty
list; check whether it does).

## Change (crates/fe-config/src/recent.rs)

1. On Windows only (`#[cfg(windows)]`), `RecentList::load` retries its read when it fails with
   `PermissionDenied` or the sharing-violation raw error (32): up to 10 tries, 10 ms apart, then returns
   the error as today. Factor the retry into a small helper with a doc comment saying why. NotFound and
   every other error behave as today; Unix is unchanged.
2. `save`'s `fs::rename` can fail the same way on Windows while a reader has the file open: retry it the
   same bounded way there (the same helper), removing the temp file on the final failure as today.
3. Add a regression test in crates/fe-config/tests/recent.rs,
   `loads_during_concurrent_saves_never_fail`: 8 threads, 500 save/load pairs each, every load with no
   error and equal to what was saved. It passes on macOS here; CI's `windows` job runs it after the
   push.

## Do not touch

The file format, the lock in `update`, `save`'s temp-file naming, the existing tests (including
`concurrent_saves_never_expose_partial_contents`, which stays as it is), and every other platform's code
path.
