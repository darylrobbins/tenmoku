---
task_uid: "01m75nhfqjvq3k4sew090xqyqv"
task: "T325"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "cargo nextest run -p kiln-plugin program_budget passes, and 30 repeated runs of it pass while a parallel cargo build loads the machine; the test still asserts the first run is charged what it measured and the second is killed at what the first left"
    required: true
    manual: false
---

# Flake: kiln-plugin program_budget::a_calls_second_run_is_killed_at_what_the_first_left kills the first nap under load (900 ms share) (Mac T1725)

Flake: kiln-plugin tests::program_budget::a_calls_second_run_is_killed_at_what_the_first_left failed main's post-merge check on 5ba8a9e6f (run 01m75hepd7naw7k6724zmjv8a7, 2026-11-11 15:18Z, Mac load ~44) with

    called `Result::unwrap()` on an `Err` value: Error::Failed("nap ran past its 899.956458ms deadline and was killed")
    at kiln/crates/kiln-plugin/src/tests/program_budget.rs:79:50

The test arms a 900 ms share for the whole call, and its first run (`nap 0`) must finish inside it. Under load, spawning a process and its zero-length sleep took the whole share, so the first run was killed. The same commit passed a full macOS stack gate an hour earlier.

Fix the test only: keep its intent (the first run is charged what it measured; the second, much longer run is killed at what the first left, not left to run), with margins that hold on a loaded machine. For example, arm a few seconds, make the second nap far longer than the share, and bound the second's elapsed time by the share plus a generous margin rather than a fixed 3 s. Check the file's sibling tests that use sub-second shares for the same pattern and widen them alike if they share the risk.

Prove it with repeated runs under load (e.g. 30 runs of the program_budget tests while a parallel build loads the machine); no regression test, as this is a test-only fix.

## Implementation notes

- The first run's share is now 4 s and the second nap 60 s; `State::with_deadline(120 s)` takes the run's own 10 s `PROGRAM_DEADLINE` out of reach, so only the call's share can kill the second run. The test asserts the `TimedOut` deadline is no more than what the first run left, and that the second ends within share + 20 s. The sibling `checking_the_program_comes_out_of_the_share` bound went from 3 s to 20 s.
- Load check: `CARGO_TARGET_DIR=$PWD/target-load cargo build -p kiln-plugin --tests -j8 &` (fresh target dir, so a full build) alongside 30 runs of `cargo nextest run -p kiln-plugin program_budget` (all 7 tests): 0 failures.
