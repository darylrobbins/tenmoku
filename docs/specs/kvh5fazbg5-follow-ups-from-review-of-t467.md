---
task_uid: "01m6qk55sjcvvh6wkvh5fazbg5"
task: "T471"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T467

Findings of the review of T467 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-06, note 1106c), for helium batch 106:

- F1 medium crates/fe-syntax/src/injection.rs:482: An unfinished directive is buffered and rescanned from its opening percent on every chunk. For `%` followed by N zero flags, `used` remains zero until the verb arrives, making scanning quadratic in directive length and retaining the entire directive. Each deadline check can therefore be followed by an arbitrarily large scan. This newly introduced behavior undermines the requested bounded, deadline-aware processing and can exhaust the highlight retry budget.
  fix: Carry incremental parser state across chunks instead of retaining and rescanning the whole directive. Bound work between deadline checks, and add a regression spanning many chunks that verifies linear processing and interruption.
  instances: crates/fe-syntax/src/injection.rs:482, crates/fe-syntax/src/verbs.rs:29, crates/fe-syntax/src/verbs.rs:46
