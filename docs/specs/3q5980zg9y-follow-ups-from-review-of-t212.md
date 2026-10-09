---
task_uid: "01m4ezhq698sjy0x3q5980zg9y"
task: "T234"
rev: 2
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T212

Findings of the review of T212 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-config/src/tests/watch.rs:287: The helper still explains the missing event through an unverified stream-attachment race. The revised notes explicitly say the mechanism was not reproduced, yet the comment presents attachment delay as the explanation. Repeated successful runs establish the mitigation's reliability, not this diagnosis, leaving T212's central follow-up unresolved.
  fix: Record evidence supporting the missing-event diagnosis, or explicitly describe the cause as unknown and the repeated writes as an empirical mitigation. Remove the unsupported attachment explanation from the helper comment and distinguish hypotheses from observations in the notes.
  instances: crates/fe-config/src/tests/watch.rs:287, docs/specs/jdtt72nj42-flaky-under-load-fe-config-tests-watch.md:49

- F2 medium crates/fe-config/src/tests/watch.rs:334: The new deadline is checked only after receive timeouts. Once a user batch arrives, the next receive gets a fresh 60 seconds even if the deadline is nearly exhausted; successful incomplete batches also bypass the deadline entirely. Consequently the helper does not enforce the documented TIMEOUT overall and can keep processing batches beyond it.
  fix: Check the remaining deadline on every iteration and cap each receive timeout by that remaining duration, using RETRY only while awaiting the user batch. Fail when the overall deadline expires, including when incomplete batches continue arriving.
  instances: crates/fe-config/src/tests/watch.rs:334, crates/fe-config/src/tests/watch.rs:341, docs/specs/jdtt72nj42-flaky-under-load-fe-config-tests-watch.md:50
