---
task_uid: "01m6mz36eb3h3v5y49bx4xz1kp"
task: "T1383"
rev: 4
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-plugin src-declared cfg(test) regression test prelaunch_tests::delayed_setup_past_the_share_end_is_refused: command setup delayed by a seam past the share end is refused with SpawnError::TimedOut and starts no child; it fails at base on an assertion"
    required: true
    manual: false
  - id: "C2"
    text: "Grant-suspension persistence never runs unbounded on the plugin thread: with a delayed store-I/O seam the call returns within the share end plus a small margin, the grant is suspended in memory at once, and the suspension is persisted later or retried on the next load, spawn or unload of that store, never lost while the store's writer thread runs; shown by a test in prelaunch_tests. Lead decision (review 5frt29 F1): the one case not retried, where the writer thread cannot be created at load and no later load, spawn or unload happens before exit, is an accepted limit, documented on SuspensionWriter; no engine-ticker retry"
    required: true
    manual: false
  - id: "C3"
    text: "Expiry is checked before accepting the grant-store lock and before every program check, metadata-error paths included; a controlled hashing read seam shows a check stops within one chunk of the share end"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean, and the kiln-plugin and fe-workbench tests pass"
    required: true
    manual: false
---

# Bound all of a plugin program run's pre-launch work by the call's program-share end: re-check it right before launch, bound or defer grant-suspension persistence, check expiry before the grant-store lock and every program check (follow-up of T1382)

## Problem

T1382 made a call's program runs share `kiln_plugin::limits::PROGRAM_DEADLINE` (10 s) and bounded the program
check (`grants::check_program_by`, chunked hash) and the grant-store lock wait by the share's absolute end. Its
review (codex, rounds 3 to 5, F1 high) found pre-launch work that is still not bounded by that end, so a call
can still hold its plugin thread past `CALL_DEADLINE` + `PROGRAM_DEADLINE`:

- grant suspension persistence (the store write after a changed program is found, kiln-plugin grants.rs around
  :432 and :672-716) runs on the plugin thread with no deadline;
- expiry is not checked before accepting the grant-store lock, nor before every program check, including the
  metadata-error paths;
- command setup in process.rs `run` (around :229-238) is not given the absolute end and is not re-checked
  immediately before the child is launched.

## Change

1. Carry the share's absolute end into `process` `run` and check it immediately before launching the child;
   after expiry refuse with `SpawnError::TimedOut`.
2. Make suspension persistence bounded: either defer it off the plugin thread (queue the suspension and persist
   it from a worker, with the in-memory grant suspended at once), or bound the complete operation (lock, read,
   write, rename) by the absolute end. A suspension that cannot persist in time stays in memory and is retried,
   never dropped.
3. Check expiry before accepting the grant-store lock and before every program check, including paths that end
   in a metadata error.

## Tests

In a `#[cfg(test)]` module declared from kiln-plugin src (new API allowed there), using controlled seams rather
than big files or sleeps on real I/O:
- delayed command setup past the share end: refused with `SpawnError::TimedOut`, no child started;
- a delayed store I/O seam during suspension: the call returns within the share plus a small margin, and the
  suspension is persisted later (or retried) rather than lost;
- a controlled hashing read seam: a check stops within one chunk of the share end.

The regression test is the delayed-setup one; it must fail at base (where the setup is not re-checked) on an
assertion and pass with the fix.

## Implementation notes

- `process::run` takes one absolute end, the earlier of the run's own deadline and the share's, and refuses with `Failure::TimedOut` right before `command.spawn()`, after all setup, so no child starts past it.
- A suspension is deferred rather than bounded, because a started write and its sync can't be interrupted. `State::suspend` withdraws the grant in memory at once, queues a `grants::SuspensionWrite` with the store's `SuspensionWriter` and waits for it only until the share's end (the run's own deadline with no share). The call returns `SpawnError::Suspended` either way: nothing ran and the program did change. Base returned `TimedOut` when the store was slow.
- There is one `SuspensionWriter` per store path, process-wide, so the queue outlives any `State` and an unload can't drop a write (review round 1, F1). Its thread starts at `State::recorded_in`, when the plugin loads, so finding a suspension later needs no new thread. It retries a failed write from 50 ms, doubling to 30 s.
- Accepted limit (review round 2, F1; lead's decision, C2 at rev 4): if the writer's thread can't be created at load and no later load, spawn or unload on that store comes before the process ends, nothing retries the queued write: nothing can run without a thread, and the engine's ticker isn't given this job. The suspension then isn't recorded, and the next launch's load finds it only if the program is still changed. This is documented on `SuspensionWriter`.
- The writer records the one finding rather than sweeping again, and only while the store still holds the grant it was found under: the same identity and the same `GrantRecord::grant_token`. Identity alone isn't enough, because a file that lost its execute bit and got it back keeps its identity, so a late write would withdraw a grant made since (review round 1, F2). A count in the record isn't enough either, because forgetting the plugin resets it (review round 2, F2). So each binding in `grant` and `narrowed` gets a token from the clock, the process and a counter. Tokens are stored in the grants file, so another process's grant counts too. A record from before tokens has none, and matches a finding with none. Tests: `a_late_suspension_leaves_a_program_granted_anew_alone`, `a_late_suspension_leaves_the_same_file_granted_again_alone`, `a_late_suspension_leaves_the_same_file_granted_after_a_forget_alone`.
- `check_with` looks at the clock on entry, after opening the file, and after a failed hash, so an error found past the end is `CheckTimedOut`, not a suspension. `locked_by` looks before opening the lock file and before each try, so a free lock isn't accepted past the end.
- `GrantStore::check_programs_by` no longer has a production caller with a deadline: the spawn path doesn't sweep. It stays as the deadlined public form of `check_programs`, and C3's checks are tested through it.

### Regression evidence

Regatta's overlay carries `prelaunch_tests.rs` onto base but not `lib.rs`, which declares it, and T1383 has no `test_support_paths`. So at base the test doesn't exist: `rgt finish` passed the gate, then stopped at `needs_human:infra`, because nextest exits with "no tests to run" there, on both tries. Adding `kiln/crates/kiln-plugin/src/lib.rs` to the task's `test_support_paths` would carry the declaration too. The candidate's `lib.rs` differs from base only by that declaration, and the seams live in the test file, so it compiles against base. Run by hand with `cargo nextest run -p kiln-plugin --lib` on the test ID:

- Base `process.rs` and `grants.rs` plus only the test file and the `lib.rs` line: fails on `assertion failed: matches!(ran, Err(SpawnError::TimedOut { .. }))`. The seam isn't wired there, so the child simply runs. This is what the overlay would see with `lib.rs` carried, but it doesn't show the bug by itself.
- Base `process.rs` plus only the seam calls (`during_setup`, `launching`) right before `command.spawn()`: fails on `assert_eq!(seams::launches(), 0)` with `left: 1, right: 0`. This is the bug: setup runs past the share's end and a child still starts. Base then kills it at once, so `TimedOut` alone wouldn't catch it, hence the launch count.
- Candidate: passes.
