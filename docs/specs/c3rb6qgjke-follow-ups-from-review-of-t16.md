---
task_uid: "01m4m1z430dnc1ayc3rb6qgjke"
task: "T27"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T16

Findings of the review of T16 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-buffer/tests/proposed_change.rs:381: The regression test cannot exercise the reported race: it permanently marks the target hunk Stale before spawning threads, so no unchanged-snapshot check can read Pending. The original buggy implementation would pass this test. Additionally, the final hunks(&edited) call would restore Stale before asserting, masking any resurrection. The implementation fixes the transition, but the spec's concurrency regression coverage remains missing.
  fix: Add a deterministic regression that starts with a Pending hunk, pauses an unchanged-snapshot check after it reads Pending, records Stale from the edited snapshot, then resumes the unchanged check. Assert the recorded status remains Stale without first checking the edited snapshot again, and verify the test fails with the original unconditional store.
  instances: crates/fe-buffer/tests/proposed_change.rs:381, crates/fe-buffer/tests/proposed_change.rs:398
