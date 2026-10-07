---
task_uid: "01m49xzy0hzgbj98c2rd7cngty"
task: "T157"
rev: 2
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T124

Findings of the review of T124 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/open.rs:125: Overlapping Settings… requests still start independent initialization tasks. After one creates settings.toml but before it writes the template, another treats AlreadyExists as success and can read the empty file into a tab. The creator subsequently activates that tab without rereading it; initialization failures likewise do not propagate to the other request.
  fix: Coordinate initialization by settings path so every overlapping request waits for the completed write and receives any failure before opening the file. Add the requested deterministic regression test that pauses initialization between creation and writing and verifies the resulting editor contents.
  instances: crates/fe-workbench/src/open.rs:124, crates/fe-workbench/src/open.rs:125, crates/fe-workbench/src/workbench.rs:344
