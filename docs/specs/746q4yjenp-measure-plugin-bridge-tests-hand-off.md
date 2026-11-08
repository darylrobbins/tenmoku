---
task_uid: "01m6wdqxdphz8pt3746q4yjenp"
task: "T526"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_hand_off_past_its_deadline_never_starts (and any other test in plugin_bridge_tests.rs with the same ordering) samples its start instant before building the Handoff deadline from it; the \u003e= 20 ms lower-bound assertion is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "the plugin_bridge_tests tests pass 20 runs in a row under host load 30+, with the load recorded in the finish notes"
    required: true
    manual: false
  - id: "C3"
    text: "only crates/fe-workbench/src/plugin_bridge_tests.rs changes, and cargo clippy -p fe-workbench --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Measure plugin_bridge_tests' hand-off deadline waits from before the deadline is set: a_hand_off_past_its_deadline_never_starts (flake)

Load flake seen in the helium sync gate for mac/main a63c9467 (run 01m6wbs0t8kfvbp1mbn8g3gkqd, 2026-11-08 ~01:50Z, load ~75):

```
FAIL fe-workbench plugin_bridge::plugin_bridge_tests::a_hand_off_past_its_deadline_never_starts
panicked at crates/fe-workbench/src/plugin_bridge_tests.rs:543:5:
assertion failed: waited.elapsed() >= Duration::from_millis(20)
```

The test builds `Handoff::new(Instant::now() + 20ms)` and only then samples
`let waited = Instant::now()`. `Handoff::wait` (plugin_bridge.rs) returns an
error only once `deadline.saturating_duration_since(now)` is zero, so it never
returns early; but if the thread is preempted between the two `Instant::now()`
calls, `waited` is sampled late and `waited.elapsed()` at return is under 20 ms.
The production code is fine; the test measures from the wrong instant.

## Fix

Test-only: take the start instant first and build the deadline from it
(`let waited = Instant::now(); let handoff = Handoff::new(waited + 20ms);`),
so the lower-bound assertion holds by construction. Check the file's other
timing assertions (e.g. `a_hand_off_running_past_its_deadline_stops_the_wait`)
for the same ordering and fix them the same way. Don't loosen the assertion
itself, and don't touch plugin_bridge.rs.

This is a fix to the test, so it can't fail at base on demand: prove it with
repeated runs under load instead.
