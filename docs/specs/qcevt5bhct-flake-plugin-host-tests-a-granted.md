---
task_uid: "01m6ksce2j6h9fzmqcevt5bhct"
task: "T1382"
rev: 5
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-workbench regression test plugin_host::plugin_host_tests::a_granted_program_that_outlasts_the_call_deadline_completes: a granted program that sleeps 0.3 s then prints hi completes; the run-command call returns Ok(Reply::RunCommand(Ok(()))) and the plugin logs hi. It fails at base on an assert_eq of that reply, and plugin_host_tests.rs uses base API only"
    required: true
    manual: false
  - id: "C2"
    text: "kiln_plugin::limits::PROGRAM_DEADLINE is 10 s and replaces process::SPAWN_DEADLINE as process::State's default; the fe-workbench loader no longer clamps the spawn deadline to call_deadline; CALL_DEADLINE stays 100 ms for wasm execution; Limits gets no new field and HostState::deadline stays Option\u003cInstant\u003e"
    required: true
    manual: false
  - id: "C3"
    text: "Time spent waiting on a program run is not charged to the call: in both kiln-plugin runner.rs and fe-workbench plugin_host.rs, the epoch callback, the pending-import poll check, the after-call overran check, and HostState::deadline as imports see it all move back by each run's wall time"
    required: true
    manual: false
  - id: "C4"
    text: "All program runs of one call share PROGRAM_DEADLINE (a run is killed at the earlier of its own deadline and the call's remaining program budget); shown by a kiln-plugin src-declared cfg(test) test with a sub-second budget where a call's second run is killed with SpawnError::TimedOut; direct process::State use without a runner keeps per-run deadlines (existing kiln tests/process.rs pass unchanged); the program check (grants::check_program_by: chunked read and hash) and the grant-store lock wait are bounded by the same absolute share end: a check still running when the share ends is refused with SpawnError::TimedOut without suspending the grant, shown by a test in that module. Out of scope, a known limit recorded in the hand-in: grant-suspension persistence, expiry checks before accepting the lock and before every program check, and a re-check of the end immediately before launch are follow-up T1383"
    required: true
    manual: false
  - id: "C5"
    text: "limits.rs and process.rs docs, and the defaults line in docs/decisions/vmtnmkay62-extensions-and-plugins.md, state the 10 s program-run limit and that the 100 ms call deadline does not count it"
    required: true
    manual: false
  - id: "C6"
    text: "plugin_host::plugin_host_tests::a_granted_program_runs_and_an_ungranted_one_is_denied is unchanged, there is no test-only Limits seam, and it passes 20 runs in a row with the machine loaded (e.g. while a cargo build runs), shown in the hand-in notes"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean, and the kiln-plugin and fe-workbench tests pass"
    required: true
    manual: false
---

# Give a plugin's process.spawn runs their own 10 s limit outside the 100 ms call deadline, so a granted program that takes longer than 100 ms completes (fixes the plugin_host_tests spawn flake)

## Problem

A plugin with a `process.spawn:<program>` grant can run that program for only 100 ms. The loader builds
`Limits::default()` (crates/fe-workbench/src/plugin_host.rs:1402) and cuts kiln-plugin's 30 s
`process::SPAWN_DEADLINE` down to the call deadline (`.with_deadline(limits.call_deadline)`, :1414). The call's
deadline is an absolute Instant that keeps running while the host waits on the child (spawn is a synchronous
import), so a run past 100 ms fails the call on three checks: the epoch callback (:2043), the pending-import
`passed()` poll check and the after-call `overran` check in `Guest::guarded` (:2086-2128). Real tools (git,
formatters) take longer than that, and under load it makes
`plugin_host::plugin_host_tests::a_granted_program_runs_and_an_ungranted_one_is_denied` fail with "a call ran past
its 100ms deadline" (Mac stack 76 gate, load 35-59). kiln-plugin's own runner (runner.rs `Guest::guarded`,
:510-556) has the same structure: there a spawn keeps its 30 s limit, but any run longer than 100 ms still makes
the call overrun.

The user's steer: a program run gets its own, longer limit.

## Change

1. `kiln_plugin::limits::PROGRAM_DEADLINE` = 10 s: how long a plugin's program runs may take. It replaces
   `process::SPAWN_DEADLINE` as `process::State::new`'s deadline. It is a named constant, not a `Limits` field:
   tests/limits.rs builds `Limits` with a full struct literal, and that file is carried onto base. The
   fe-workbench loader stops clamping the spawn deadline to `call_deadline`.
2. Time spent waiting on a program run is not charged to the call. Each run pushes the running call's deadline
   back by its wall time. That covers `HostState::deadline`, which imports such as `commands.invoke` read, and
   every check both runners make: the epoch callback, the pending-import poll check and the after-call overran
   check. The poll check runs while the call holds `&mut Store`, so the runners need a shared cell (for example
   one new pub `HostState` field holding an `Arc`, armed by the runner at each call's start, charged by
   `process`'s `Host::spawn`). Keep `HostState::deadline` an `Option<Instant>` (tests/commands.rs sets it
   directly).
3. All the program runs of one call share `PROGRAM_DEADLINE`. A run is killed at the earlier of its own deadline
   and what is left of the call's program budget. A call therefore holds its plugin's thread for at most
   `CALL_DEADLINE` + `PROGRAM_DEADLINE`; unload and reload join that thread (`HostedPlugin::shutdown`). Without
   the cap, a plugin spawning in a loop would never return. Using `process::State` directly, with no runner
   arming the budget (kiln tests/process.rs), behaves as before: one deadline per run.
4. `CALL_DEADLINE` stays 100 ms for wasm execution. Update the docs: limits.rs and process.rs module and item
   docs, and the defaults line in docs/decisions/vmtnmkay62-extensions-and-plugins.md (about :410): "10 s for a
   call's program runs (process.spawn), which the call's 100 ms deadline doesn't count".

5. Checking the program counts against the same share and is bounded by it (rev 4, review round 3's F1). Today
   `spawn_from` refuses an already used-up share, then calls `grants::check_program` (grants.rs:378), which reads and
   hashes the whole file with no deadline, and only then caps the run. Give the check the share's absolute end
   (for example `check_program_by(identity, Option<Instant>)`, keeping `check_program` as the no-deadline form for
   its other callers): hash in chunks and stop with a timeout once that Instant passes, without suspending the
   grant (a slow read is not a changed program). `spawn_from` turns that into `SpawnError::TimedOut`. Wait for the
   grant-store lock with a poll that gives up at the same end. Rev 5: the rest of the pre-launch work (grant-suspension
   persistence, an expiry check before accepting the lock and before every program check including metadata-error
   paths, and a re-check of the end immediately before launch after command setup) is follow-up T1383, a known
   limit here (review rounds 3 to 5, F1).

## Tests

- Regression, in crates/fe-workbench/src/plugin_host_tests.rs. That file is carried onto base, so the whole file
  must compile against base API: no Limits seam. `a_granted_program_that_outlasts_the_call_deadline_completes`:
  like `a_granted_program_runs_and_an_ungranted_one_is_denied`, but the granted script is
  `#!/bin/sh\nsleep 0.3\necho hi\n`. Its first failing check at base must be an `assert_eq!` on the call's reply
  being `Ok(Reply::RunCommand(Ok(())))`; then assert the plugin logged "hi\n".
- Test the per-call cap from item 3 in a `#[cfg(test)]` module declared from kiln-plugin src (it may use new API
  there), with a sub-second budget: two runs in one call, the second killed with `SpawnError::TimedOut`.
- In the same kiln-plugin cfg(test) module: a share that ends before a check of a large granted program file
  finishes (for example a share of a few ms and a file of tens of MB, or a seam that slows the read) is refused
  with `SpawnError::TimedOut`, the grant is not suspended, and the time taken stays within the share plus one
  chunk's read.
- Leave the old test as it is. Discard rev 2's uncommitted seam in the worktree (`PluginHost.limits`,
  `LoadContext.limits` and the test setting it).

## Out of scope

`net.fetch` (kiln-plugin net.rs) is also a blocking import charged to the call deadline. Note it in the hand-in
as a follow-up; don't change it here.

Bounding grant-suspension persistence, the expiry checks before the grant-store lock and before every program
check, and a re-check of the share end immediately before launch: T1383. Record them under Known limits in the
hand-in, naming T1383.

## Implementation notes

- `process::ProgramBudget` (shared `Arc` cells, `HostState::program_runs`) is armed at each call's start by both runners' `guarded`, charged by `Host::spawn`, and disarmed after the overran check. The epoch callback, poll check and overran check read `deadline + spent`; `Host::spawn` also moves `HostState::deadline` back by each run's wall time.
- `State::spawn_within(.., left)` kills a run at the earlier of its deadline and the call's remaining share; a spent share refuses with `TimedOut` without spawning.
- The old `a_spawn_is_killed_at_the_call_deadline` assumed the 100 ms limit; it became `a_spawn_in_flight_ends_before_the_plugin_unloads` (a 1 s run completes; a preview-off mid-run unloads once it ends). The kill itself is covered by kiln-plugin `program_budget_tests`.
- Loaded runs: with 2x ncpu busy loops (load average 82-87), `a_granted_program_runs_and_an_ungranted_one_is_denied` and the new regression test passed 20/20 runs.
- Follow-up: `net.fetch` (kiln-plugin net.rs) is also a blocking import charged to the call deadline.
- Review round 1 (F1): `spawn_within` refuses a spent share (`TimedOut`) right after the cheap grant lookup, before `check_program` reads the file; `ProgramBudget::charge` caps what a call excludes from its deadline at its budget and returns what it charged, which `Host::spawn` uses to move `HostState::deadline` (the runners read the same cells). Tests: `a_spent_budget_refuses_before_the_program_is_checked`, `the_time_a_call_excludes_from_its_deadline_is_capped_at_its_budget`.
- Review round 2: `spawn_from` counts the call's share from when it was read, so the time `check_program` takes comes out of the child's allowance, and a run is refused when the check used the share up (`checking_the_program_comes_out_of_the_share`). The two-run test (`a_calls_second_run_is_killed_at_what_the_first_left`) now arms a 900 ms share, runs the first through `HostState::spawn` (charged its measured time), and kills the second at what's left.
- Review round 3 / item 5: `grants::check_program_by` hashes in 64 KiB chunks and returns `CheckTimedOut` once the share's absolute end passes, without suspending (`check_program` keeps the no-deadline form); `GrantStore::check_programs_by` bounds the suspension path the same way. `spawn_from` turns a timeout into `SpawnError::TimedOut`. Test: `a_share_that_ends_mid_check_refuses_without_suspending_the_grant` (32 MiB program, 5 ms share).
- Review round 4: the share's absolute end bounds all pre-launch work. `spawn_from` builds the command first and then recomputes what's left of the share, refusing with `SpawnError::TimedOut` if none is left just before `run`. The suspension path takes the grant-store lock with a try-lock poll (`locked_by`, 1 ms) until the end, and skips the write (and returns `TimedOut` from `spawn_from`) if the end passes before it; the in-memory suspension still applies. Test: `a_share_that_expires_before_launch_refuses_and_a_held_store_lock_does_not_block_past_it`.

### Known limits

Follow-up T1383 (depends on T1382) owns these, which T1382's C4 leaves out: persisting a grant suspension within the share (the store write and its fsync are not interruptible once started), expiry checks before accepting the store lock and before every program check, and a re-check of the share end immediately before launch with a deterministic hashing-read seam. T1382 already refuses a spent share before checking, polls the lock until the share's end, skips the write if the end passed before it, and recomputes what is left of the share after command setup, but without that coverage.
