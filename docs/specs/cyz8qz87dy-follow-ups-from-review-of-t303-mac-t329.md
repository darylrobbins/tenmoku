---
task_uid: "01m4kxpvvh64qmmycyz8qz87dy"
task: "T22"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T303 (Mac T329)

Findings of the review of T303 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-fileview/tests/source.rs:368: The preceding-context regression test caches both chunks: char_at(chunk) reads the preceding three bytes through window(), loading the first chunk before truncation. Cached chunks remain readable afterward, so the test never encounters unavailable preceding context and passes with the original defective implementation. This leaves an explicitly requested regression case untested.
  fix: Warm only the second chunk by reading well inside it, such as char_at(chunk + 10), then truncate. Assert the boundary and clipping behavior near the euro character and assert that the failed preceding-context read records ReadErrorKind::Changed. Verify this test fails with the original preceding-context implementation.
  instances: crates/fe-fileview/tests/source.rs:368



Moved from the Mac board: Mac T329. Its dependencies are finalized on Mac and arrive here with the mac/main sync.
