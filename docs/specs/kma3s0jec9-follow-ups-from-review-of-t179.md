---
task_uid: "01m4c7s1hqs8nr33kma3s0jec9"
task: "T183"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "F2: if the Settings command never reaches Stage::Wait, the open_settings test fails with its clear assertion within a bounded time instead of hanging: the coordination waits are bounded and timeout cleanup releases the initializer and ends the releaser before either is joined"
    required: true
    manual: false
  - id: "C2"
    text: "On success the test still asserts the original command opened the tab without another dispatch"
    required: true
    manual: false
---

# Follow-ups from review of T179

Findings of the review of T179 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-workbench/src/tests/open_settings.rs:219: The completion wait is still not bounded on failure: if the Settings command never reaches Stage::Wait, the releaser remains blocked receiving its notification and the initializer remains blocked awaiting release. After polling expires, the test joins these threads before checking opened, so it hangs instead of reporting the requested clear failure.
  fix: Bound the coordination waits and ensure timeout cleanup releases the initializer and terminates the releaser before joining either thread; then assert that the original command opened the tab without another dispatch.
  instances: crates/fe-workbench/src/tests/open_settings.rs:188, crates/fe-workbench/src/tests/open_settings.rs:203, crates/fe-workbench/src/tests/open_settings.rs:219, crates/fe-workbench/src/tests/open_settings.rs:220
