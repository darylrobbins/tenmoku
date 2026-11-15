---
task_uid: "01m7ete2d7twvxbcyz1dvd8v6a"
task: "T657"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test kiln-plugin::cache a_fifo_at_the_entry_is_a_miss passes (cargo nextest run -p kiln-plugin --test cache a_fifo_at_the_entry_is_a_miss), its deadline no longer timing the component compile"
    required: true
    manual: false
  - id: "C2"
    text: "The test passes 20 runs in a row while a parallel cargo build loads the machine, with the runs and load recorded in the finish notes"
    required: true
    manual: false
---

# Flake: kiln-plugin::cache a_fifo_at_the_entry_is_a_miss times out under gate load (2 s deadline covers the compile)

# kiln-plugin::cache a_fifo_at_the_entry_is_a_miss times out under load

## Problem

The batch-184 gate (run 01m7eqk72pkmqtav554ew435q4, load average ~18) failed only on
`kiln-plugin::cache a_fifo_at_the_entry_is_a_miss` after 2.2 s: "blocked on the FIFO: Timeout" at
kiln/crates/kiln-plugin/tests/cache.rs:277. The same test passes alone in 0.017 s.

The test's 2 s `recv_timeout` covers the whole `cache.load`, which includes compiling the component
after the miss, not only the check that the FIFO isn't opened for reading. Under gate load the
compile alone can exceed 2 s.

## What to change (test only)

Keep what the test proves (loading never blocks on a FIFO at the entry path, and a FIFO is a miss
that compiles), and stop the deadline from timing the compile: a deadline generous enough for a
loaded machine (a real block never returns, so e.g. 60 s still catches it), or split the timing so
the deadline covers only the entry check. No production change. Comments under kiln/ must not name
the app or its crates (scripts/check-no-gpui.sh).

Prove it with repeated runs under load (e.g. 20 runs alongside a parallel build), not a regression
test.
