---
task_uid: "01m4kxpvtjmhfpwwfk49ybpjxg"
task: "T21"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T237 (Mac T316)

Findings of the review of T237 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/lsp.rs:776: Saves received while a root retry is in flight are drained and discarded. If that attempt has already failed canonicalization but its completion has not landed, creating the root and saving again consumes the recovery-triggering save while root_retrying is true. The failed completion then leaves the manager absent indefinitely until another save.
  fix: Preserve a pending save while a root retry is running and, if that attempt fails, schedule one further resolution for the pending save. Clear pending state on success and root replacement, and add a deterministic test covering this ordering.
  instances: crates/fe-workbench/src/lsp.rs:767, crates/fe-workbench/src/lsp.rs:776



Moved from the Mac board: Mac T316. Its dependencies are finalized on Mac and arrive here with the mac/main sync.
