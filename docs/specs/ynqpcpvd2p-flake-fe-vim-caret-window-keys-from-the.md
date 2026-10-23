---
task_uid: "01m5jq2tfr6ragz3ynqpcpvd2p"
task: "T229"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "keys_from_the_middle_of_a_100_mb_row_land_as_on_a_short_row asserts the bounded work (a counter covering everything the caret window reads: the window slices, bracket-scan buffers and discarded expansion attempts, so a whole-row copy fails it), not a tight wall time; the landing-place assertions are unchanged; it passes 40 runs at -P 4 under 16 busy loops (documented limit: the first 200-run stress at load 50 took over an hour and held the machine and the trunk sync gate, so the run count was cut by lead decision; the run count and command are recorded in the commit message)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim passes and cargo clippy -p fe-vim --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Flake: fe-vim::caret_window keys_from_the_middle_of_a_100_mb_row_land_as_on_a_short_row fails under load: "f(%" took 1.17 s against a 1000 ms wall limit

Flake, requested by the Mac lead (z49).

## Failure
```
fe-vim::caret_window keys_from_the_middle_of_a_100_mb_row_land_as_on_a_short_row
panicked at crates/fe-vim/tests/caret_window.rs:93:9: "f(%" took 1.166528s
```
On Mac main 2b787020, run 01m5jnxjh9za58697w3ms56tq3, during two overlapping full test runs. The fast rerun of the same commit passed.

## Problem
The test asserts each key's wall time on a 100 MB row is under `limit()` (1000 ms in debug, 50 ms in release). Under load wall time is not what it means to prove: that the caret window reads a bounded window of the row, not the whole row.

## Work
Make it load-proof without losing that proof. E.g. assert on the work done (bytes or graphemes the window reads, via a test counter in fe-vim's caret window code, `CARET_WINDOW` in `crates/fe-vim/src/motion.rs`), or compare against the same keys on the short row with a generous ratio. Keep a wall-time check only where it is robust (a very generous bound that the whole-row copy and segmentation, which took many seconds, would still fail). The landing-place assertions (`got == want`) stay as they are. Prove it with repeated runs under load (stress-ng or parallel nextest loops, load 30+, at least 200 runs). Do not just raise limit().

Files: `crates/fe-vim/tests/caret_window.rs`; `crates/fe-vim/src/motion.rs` and `crates/fe-vim/src/lib.rs` only for a test counter.
