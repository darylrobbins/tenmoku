---
task_uid: "01m55txr1km4dt8ap1dsab1bkg"
task: "T930"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Both tests compare the fastest of at least 5 interleaved bounded and full-parse runs, and still assert that the bounded reparse gives up (returns None)"
    required: true
    manual: false
  - id: "C2"
    text: "20 runs of both tests pass while the workspace's heavy tests run in parallel"
    required: true
    manual: false
---

# Flake: fe-gpui auto_indent_host_tests a_stale_tree_is_parsed_within_the_time (and a_slow_seed_stops_at_the_deadline) fail under load on a single-sample bounded*2 < parse timing: compare best-of-interleaved runs

## Problem

`crates/fe-gpui/src/auto_indent_host_tests.rs:138`, `a_stale_tree_is_parsed_within_the_time`, failed twice on
2026-10-17 under load, each time on its single-sample wall-clock assertion `bounded * 2 < stale`:

- main-red fast CI on b11e4eff (run 01m55syd09gwfft8g6va5dq46z): "42.654458ms bounded, 59.234708ms stale";
- gate on b213c897 (run 01m55gd95d13hpg9xpjyynx7vx): "92.202667ms bounded, 65.028584ms stale".

`bounded` is one `reparsed` call with a 2 ms deadline; it should stop within a few ms, so 40-90 ms is the
thread being descheduled under load (it passed in the other ~23 runs of the day, 0.36-1.4 s). Nothing in
the change it failed on touches fe-syntax or `commands.rs`. The sibling `a_slow_seed_stops_at_the_deadline`
(`bounded * 2 < seed`) has the same single-sample shape.

## Change

Test-only. In both tests, interleave the bounded `reparsed` call and the full parse it is compared with
(5 rounds, rebuilding the stale state each round as needed) and compare the fastest of each, as earlier flake
fixes did for timing ratios. Keep what the tests guard: a stale tree, or a slow seed, gives up at the deadline
rather than parsing on.

## Tests

No regression test (the fix is to the tests). Prove it with 20 runs of both tests while the workspace's
heavy tests run in parallel.
