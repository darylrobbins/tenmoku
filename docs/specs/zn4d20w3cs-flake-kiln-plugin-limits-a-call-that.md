---
task_uid: "01m5xfe92qadcvdtzn4d20w3cs"
task: "T1202"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_call_that_returns_past_its_deadline_times_out loads grow_memory with up to 5 tries, retrying only on LoadError::Unloaded with Reason::Timeout (any other load error panics), as an_infinite_loop_is_interrupted does at limits.rs:167-181, then runs the unchanged call and assertions (timed_out, logged 'grown', state Unloaded)"
    required: true
    manual: false
  - id: "C2"
    text: "No assertion or deadline changes: limits() stays 250 ms, the 400 ms sink delay stays, and src/runner.rs, src/limits.rs, tests/fixtures/grow_memory.wat and the other limits.rs tests are untouched"
    required: true
    manual: false
  - id: "C3"
    text: "Evidence in the notes: inside kiln/ with 4+ CPU-burning background loads, 'cargo nextest run -p kiln-plugin --test limits' passes 20 consecutive runs; plus clippy and fmt clean in kiln/"
    required: true
    manual: false
---

# Flake: kiln-plugin limits a_call_that_returns_past_its_deadline_times_out fails under load (load of grow_memory hits the 250 ms deadline at limits.rs:206)

## Evidence

Main's check at 48dfc786 (run 01m5xdy2qfw3wrv2m9qspw7vvh, test.fastedit.log ~line 5932) failed only on
`kiln-plugin::limits a_call_that_returns_past_its_deadline_times_out`, with the Mac running two full test
suites at once and its disk nearly full:

    panicked at kiln/crates/kiln-plugin/tests/limits.rs:206:29:
    called `Result::unwrap()` on an `Err` value: Unloaded(Diagnostic { plugin: "grow_memory",
    reason: Timeout, message: "a call ran past its 250ms deadline" })

## Cause

Line 206's unwrap is the plugin's load, not the call under test. `guarded` in
kiln/crates/kiln-plugin/src/runner.rs wraps instantiation (:493) and `init` (:500) in the same wall-clock
call deadline (:517-556, Instant-based), and `limits()` in tests/limits.rs:35 sets it to 250 ms. A starved
thread can't instantiate and run init in 250 ms, so the load returns Unloaded(Timeout). The deadline can't
just be raised: the test needs the 400 ms sink delay to exceed it. The sibling test
`an_infinite_loop_is_interrupted` (limits.rs:167-181) already retries its load up to 5 times for this reason.
runner.rs behaves as designed: there is no production bug.

## Fix

In tests/limits.rs only: load grow_memory with the same retry as `an_infinite_loop_is_interrupted` (up to 5
tries, retrying only on Unloaded with Reason::Timeout; any other load error panics), then run the unchanged
call and assertions.

## Do not touch

The production deadline, src/runner.rs, src/limits.rs, tests/fixtures/grow_memory.wat, the 250 ms test
deadline, the 400 ms sink delay, or the other tests in limits.rs. This is a test-only fix, so there is no
regression test: prove it with 20 consecutive passing runs under load instead (see the criteria).

## Implementation notes

- tests/limits.rs only: `a_call_that_returns_past_its_deadline_times_out` now loads grow_memory with up to 5 tries, retrying only on `Unloaded`/`Reason::Timeout`, as `an_infinite_loop_is_interrupted` does.
- Evidence (C3): in `kiln/`, with 4 CPU-burning `while :; do :; done` loops on a 14-core Mac, `cargo nextest run -p kiln-plugin --test limits` passed 20 consecutive runs. clippy (`-p kiln-plugin --tests`) and `cargo fmt --all` clean.
- With 6 burners other, untouched limits tests (`calls_run_on_the_plugin_thread`, `a_trap_unloads_the_plugin_with_a_diagnostic`, `memory_growth_past_the_limit_fails`) fail the same way on their unretried loads; out of scope here (C2), worth a follow-up.
