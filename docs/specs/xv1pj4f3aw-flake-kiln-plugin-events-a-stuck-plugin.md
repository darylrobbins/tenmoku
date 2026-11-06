---
task_uid: "01m6pn9sb2ch38k5xv1pj4f3aw"
task: "T1387"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_stuck_plugin_does_not_delay_the_sender still fails if a push blocks until the stuck plugin returns, and no longer relies on a sub-100 ms wall-clock bound for 1000 pushes"
    required: true
    manual: false
  - id: "C2"
    text: "The test passes 200 consecutive runs while a cargo build of the workspace runs alongside, recorded in the finish evidence"
    required: true
    manual: false
---

# Flake: kiln-plugin events a_stuck_plugin_does_not_delay_the_sender fails under load (1000 pushes took 64 ms against a 50 ms wall-clock bound)

`kiln-plugin::events a_stuck_plugin_does_not_delay_the_sender` (kiln/crates/kiln-plugin/tests/events.rs:488) times 1000 `queue.push` calls on the wall clock and asserts they take under 50 ms. Under gate load (load average ~35, stack 81 run 2 on d237d753, 2026-11-05) it measured 64.0 ms and failed; it passed in run 1 of the same stack and 3/3 in isolation afterwards. The stack did not touch the event queue.

The test's point is that a push never waits on the plugin, which stays stuck inside `deliver` while the pushes run. A wall-clock bound that tight can't tell "never blocks" from "scheduler preempted the test thread". Make the check robust to load without weakening what it proves: for example, keep the plugin blocked for a known, much longer time (or until released) and assert the pushes finish before it is released (the existing `returns.try_recv() == Empty` check), with any wall-clock bound only as a generous backstop far below the plugin's stall. Test-only: no production change.

Prove it with repeated runs under load (e.g. 200 runs of the test while a workspace build runs), recorded in the finish evidence.
