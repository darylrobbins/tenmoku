---
task_uid: "01m4h0vz3w6g9agccgmkbheewn"
task: "T299"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The hand-in names which assertion failed under load (reproduced at base, or argued from the test). If a safety assertion (an Ok that isn't the inside symbol, or an Err other than Unreadable(NotFound)) can fail, the work stops and this is reported as a production race to re-file as a bugfix"
    required: true
    manual: false
  - id: "C2"
    text: "extraction_never_reads_through_a_directory_being_swapped still asserts, on every extract that races the swapper, that each Ok is the inside file's symbol and each Err is Unreadable(NotFound), and asserts that an extract returns the inside symbol while src/a is known to be in place, by coordinating with the swapper (a hold/parked handshake, as T186 did) rather than relying on a chance catch over many tries"
    required: true
    manual: false
  - id: "C3"
    text: "No production code changes; only crates/fe-nav/tests/open_under.rs changes. The task's own spec file under docs/specs/, committed by Regatta, is expected in the diff and is not a violation"
    required: true
    manual: false
  - id: "C4"
    text: "10 consecutive runs of cargo nextest run -p fe-nav -E 'test(=extraction_never_reads_through_a_directory_being_swapped)' pass while background load holds the load average at about twice the core count, with the load average and pass count recorded in the hand-in; the load generator registers trap 'kill $(jobs -p) 2\u003e/dev/null' EXIT INT TERM before starting and leaves no process behind"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-nav passes"
    required: true
    manual: false
---

# Make the flaky fe-nav test extraction_never_reads_through_a_directory_being_swapped deterministic under load

# Make the flaky fe-nav test extraction_never_reads_through_a_directory_being_swapped deterministic under load

## Symptom

`extraction_never_reads_through_a_directory_being_swapped` (crates/fe-nav/tests/open_under.rs:92) failed in T270's fast gate (run 01m4h0ab9s80c4p48kq297ebgp, commit 098f4876, 4 lanes loading the machine). T270 doesn't touch fe-nav, and the same gate passed on T270's earlier commit. The panic was at open_under.rs:119, `never caught the directory in place`.

## Cause

This is the weakness T186 fixed in the sibling test `files_are_never_read_through_a_directory_being_swapped` (crates/fe-nav/tests/references.rs, commit abb4a7c). A swapper thread renames `src/a` aside, symlinks `outside` in its place, removes the symlink and renames `src/a` back, with no pause between swaps. Meanwhile the test calls `extract` 500 times. It makes two kinds of assertion:

1. **Safety:** every `Ok` result is the inside file's symbol (`inside`), and every `Err` is `Unreadable(NotFound)`. Neither failed. `fe_project::open_file_under` opens with openat and O_NOFOLLOW, so no read follows the swapped-in symlink.
2. **Liveness:** `read > 0`, meaning at least one of the 500 extracts caught `src/a` in place. Under load, scheduling can make every extract miss the short window between `rename(aside, a)` and the next `rename(a, aside)`. This assertion is the one that failed.

So this is a flake in the test, not a race in production code.

## Fix direction

Do what T186 did: make the test deterministic.

- Keep the safety assertions on the extracts that race the swapper.
- Add a `hold`/`parked` atomic handshake. After the racing loop, hold the swapper with `src/a` in place, then assert that one `extract` returns `inside`. Don't count on a chance catch.
- Add no production hooks. Change only crates/fe-nav/tests/open_under.rs.
