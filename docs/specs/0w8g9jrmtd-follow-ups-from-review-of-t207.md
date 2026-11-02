---
task_uid: "01m6cwrt7y2y7m750w8g9jrmtd"
task: "T404"
rev: 1
type: "chore"
complexity: "xs"
criteria: []
---

# Follow-ups from review of T207

Findings of the review of T207 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/auto_indent_host_tests.rs:189: The replacement assertions establish only that the returned copy is parsed at the requested version; they do not establish that seeding ran, as the spec requires. Skipping the seed branch and parsing the snapshot from scratch satisfies every assertion, so the test loses its seed-specific regression coverage. The change also leaves the reported skipped-work cause unexplained.
  fix: Verify the source syntax's seed preconditions and add deterministic evidence that the seed path executed, using a test counter or observable syntax state. Investigate the fast-return path and retain the elapsed-time-independent completion assertions.
  instances: crates/fe-gpui/src/auto_indent_host_tests.rs:189
