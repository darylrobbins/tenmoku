---
task_uid: "01m7m6nppd66w966nh8y7vdjxm"
task: "T672"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "kiln-plugin::process output_over_the_cap_and_runs_over_the_deadline_are_killed passes 20 consecutive runs while another kiln-plugin test loop loads the machine; only kiln/crates/kiln-plugin/tests/process.rs changes"
    required: true
    manual: false
---

# Flake: kiln-plugin process output_over_the_cap_and_runs_over_the_deadline_are_killed fails under load (the output-cap call's 1.5 s deadline fires before printf's flood reaches the cap) (Mac T1926)

## Problem

`kiln-plugin::process output_over_the_cap_and_runs_over_the_deadline_are_killed`
(kiln/crates/kiln-plugin/tests/process.rs, second half) failed main's post-merge check of
5f61e1cf5 (run 01m7m51mzpx44z44v4vnmzappe, 07:29Z 11-17, load ~30 with two other gates):

    panicked at kiln/crates/kiln-plugin/tests/process.rs:545:54:
    called `Result::unwrap()` on an `Err` value: TimedOut { program: "printf", deadline: 1.5s }

The output-cap half gives the flood `printf` a 1.5 s deadline. The call is meant to be killed at
the output cap, long before the deadline; under load, starting `printf` and writing enough to
reach the cap took longer than 1.5 s, so the deadline fired first. It passes alone.

## Fix

Test-only: give the output-cap call a deadline far above any load-induced start-up delay (e.g.
30 s). The assertion that matters is that the cap kills the program (stdout cut at the cap,
status None); a generous deadline does not slow the passing case, because the cap ends the call.
If the test also wants to show the cap acts before the deadline, bound the elapsed time loosely
(well under the deadline) rather than tightening the deadline. Leave the first half (the
300 ms deadline kill) unless it shows the same flake.

Prove it with repeated runs under load (e.g. 20 runs of the test while a parallel
`cargo nextest run -p kiln-plugin` loop runs), not a regression test.

## Implementation notes

C1 evidence: with a continuous `cargo nextest run -p kiln-plugin` loop running in the background
(still alive after the runs; load average ~22), 20 consecutive
`cargo test -p kiln-plugin --test process output_over_the_cap` runs all passed (20/20).
The output-cap call's deadline is now 30 s, with a loose 20 s elapsed bound.
