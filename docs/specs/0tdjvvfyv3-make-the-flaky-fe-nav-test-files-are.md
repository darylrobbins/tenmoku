---
task_uid: "01m4cd44z290qpse0tdjvvfyv3"
task: "T186"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The hand-in names which assertion failed under load (reproduced at base, or argued from the test). If it was dropped == 0, the work stops and this is reported as a production race to re-file as a bugfix"
    required: true
    manual: false
  - id: "C2"
    text: "files_are_never_read_through_a_directory_being_swapped still asserts that no hit is read from outside (dropped == 0) while the directory is being swapped, and asserts a verified hit at src/a/x.rs from a search run while the directory is known to be in place, by coordinating with the swapper rather than relying on a chance catch over many tries"
    required: true
    manual: false
  - id: "C3"
    text: "No production code changes; only crates/fe-nav/tests/references.rs changes, plus crates/fe-nav/tests/references_swap.rs if a new test is split out. The task's own spec file under docs/specs/, committed by Regatta, is expected in the diff and is not a violation"
    required: true
    manual: false
  - id: "C4"
    text: "50 consecutive runs of cargo nextest run -p fe-nav -E 'test(=files_are_never_read_through_a_directory_being_swapped)' pass while other cargo builds load the machine (load average at least 40), with the load average and pass count recorded in the hand-in"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-nav passes"
    required: true
    manual: false
---

# Make the flaky fe-nav test files_are_never_read_through_a_directory_being_swapped deterministic under load

## Symptom

`files_are_never_read_through_a_directory_being_swapped` (crates/fe-nav/tests/references.rs) is flaky under load:

- It was noted flaky at base, with 3 failures in 10 runs at a load average of about 40.
- T173's worker saw it fail at a load average of about 170.

Which assertion fails was not recorded. Finding that out is the first step (see below).

## Suspected cause

A swapper thread loops with no pause: rename `src/a` aside, symlink `outside` in its place, remove the symlink, rename it back. Meanwhile 60 `find` calls run. The test makes two assertions:

1. **Safety:** `summary.dropped == 0` ("read from outside"). Production guarantees this: files are opened with `fe_project::open_file_under` (openat with O_NOFOLLOW, see `open_under` in crates/fe-nav/src/references.rs and crates/fe-project/src/listing.rs), so no read can follow the swapped-in symlink.
2. **Liveness:** `verified > 0` ("never caught the directory in place"). This needs at least one of the 60 searches to list `src/a`, read `src/a/x.rs` and verify the hit, all while the directory is in place. The swapper never pauses, so the directory is in place only for the moment between `rename(aside, a)` and the next `rename(a, aside)`. Under load, scheduling can make all 60 searches miss that window. This is a probabilistic, timing-based assumption in the test itself, and it is the likely failure. The newer sibling test `no_preview_is_read_through_a_directory_being_swapped` (crates/fe-nav/src/references_followup_tests.rs) holds each state for 300 µs to make such misses less likely, which reduces the chance but does not remove it.

So this is a test flake, not a production race, unless the failing assertion turns out to be `dropped == 0`. If it is, that is a real production race (a read through the swapped-in symlink). Stop and report it so the task can be re-filed as a bugfix with a regression test, instead of changing this test.

## Fix direction

Make the test deterministic. Its result must not depend on scheduling or wall-clock time:

- Keep the safety assertion (`dropped == 0`, no hit from `outside`) over searches that race the swapper.
- Prove the liveness half deterministically: coordinate with the swapper (for example a channel or `Barrier` handshake, or pausing it with the directory in place) so that at least one search runs while `src/a` is known to be in place, and assert that search's verified hit at `src/a/x.rs`. Do not count on a random catch across N tries.
- Add no production hooks. Keep the fix in the integration test. If you split it into separate tests, any new test goes in a new file, `crates/fe-nav/tests/references_swap.rs`.

## Implementation notes

- The flaking assertion was `verified > 0` ("never caught the directory in place"), argued from the test; `dropped == 0` is guaranteed by `openat` + `O_NOFOLLOW`, so this is a test flake, not a production race.
- Fix, all in `crates/fe-nav/tests/references.rs`: the swapper parks between swaps on a `hold`/`parked` atomic handshake. After the 60 racing searches (still asserting `dropped == 0`), the test holds the swapper with `src/a` in place and asserts exactly one verified hit at `src/a/x.rs`.
- This spec file is Regatta's own commit (b3b2fe8); the worker's code change is only `references.rs`.
- 50/50 runs passed at load average 327–386.
