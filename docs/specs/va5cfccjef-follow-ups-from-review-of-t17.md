---
task_uid: "01m4kzg0xwmxr6ykva5cfccjef"
task: "T24"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T17

Findings of the review of T17 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-lsp/src/tests/registry.rs:728: The new regression test still has a scheduling race. Both launches share fake_clock, and killing the predecessor signals process exit without waiting for its fake thread to drop its pending initialize timer. The successor records initialize before arming its timer, so wait_for_sleep can return for the predecessor's still-live timer. Advancing the clock then causes the successor to arm its timer one second later, with no further advance, and wait_initialized_at(2) times out. This undermines the requested deterministic coverage.
  fix: Synchronize specifically with the successor's timer before advancing time, for example by giving each launch a separate fake clock or adding a launch-specific timer barrier. Ensure the predecessor's pending timer cannot satisfy that barrier.
  instances: crates/fe-lsp/src/tests/registry.rs:728
