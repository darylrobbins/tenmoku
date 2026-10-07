---
task_uid: "01m4bsemwcdqb298ky8hppnvtp"
task: "T179"
rev: 2
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T157

Findings of the review of T157 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/open.rs:133: The per-path mutex serializes attempts but does not share their result. If the initializer fails, an overlapping waiter retries instead of receiving that failure, contrary to the spec. If removal of the partially written file fails, the waiter returns success on AlreadyExists and can open incomplete contents.
  fix: Coordinate overlapping requests around a shared initialization result, propagating failures to every waiter before any opens the file. Handle cleanup failures without treating the incomplete file as successful initialization, and add injected write/cleanup failure coverage.
  instances: crates/fe-workbench/src/open.rs:133, crates/fe-workbench/src/open.rs:148, crates/fe-workbench/src/open.rs:152

- F2 medium crates/fe-workbench/src/tests/open_settings.rs:157: The requested deterministic editor regression is missing: this test calls create_settings directly and checks disk contents, never dispatching Settings requests or checking the editor buffer. Its 200 ms timeout also does not establish that the second thread reached initialization before the first was released, so an unscheduled second thread can let the original broken implementation pass.
  fix: Add explicit synchronization establishing that the overlapping request has reached coordination while initialization is paused, exercise the Settings command through the workbench, and assert the resulting editor text equals the template after completion.
  instances: crates/fe-workbench/src/tests/open_settings.rs:183, crates/fe-workbench/src/tests/open_settings.rs:189, crates/fe-workbench/src/tests/open_settings.rs:197
