---
task_uid: "01m7cmcej146vz9zp8zc64g411"
task: "T383"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "confirming_uninstall_removes_everything no longer times out waiting for the plugins to run under load; the cause is named in the decision file"
    required: true
    manual: false
  - id: "C2"
    text: "20 runs of the fe-workbench plugin_manage_tests alongside a parallel cargo build all pass, recorded in the decision file"
    required: true
    manual: false
---

# Flake: fe-workbench plugin_manage confirming_uninstall_removes_everything times out waiting for the plugins to run under machine load (Mac T1871)

`fe-workbench plugin_manage::plugin_manage_tests::confirming_uninstall_removes_everything` failed in T1583's Mac finish gate rerun on 2026-11-14: it panicked after 31 s with "timed out waiting for the plugins to run", load average around 28 (a main post-merge check running alongside). The other 5028 tests passed; on the lane all 22 plugin_manage_tests passed three runs in a row in under 4 s each. No other failure in the last 40 gate logs.

Find out why the plugins can miss a 30 s start window under load: a wall-clock wait in the test that should be driven by the test's own executor or clock, or a real stall in plugin start-up on a loaded machine (a blocking call on the foreground thread, a worker that waits on a frame). Fix the test (or the start-up path, with a regression test that fails at base) so the wait cannot time out while work is still progressing.

Prove it with 20 runs of the plugin_manage_tests alongside a parallel `cargo build` of another crate, recorded in the decision file. Other plugin_manage tests that share the wait get the same fix.
