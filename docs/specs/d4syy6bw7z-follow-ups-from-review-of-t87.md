---
task_uid: "01m47jd49hxkh647d4syy6bw7z"
task: "T90"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T87

Findings of the review of T87 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/lib.rs:1198: Batched catch-up still drops folds preserved by synchronization after every edit, and now regresses benign header edits. With the spec's text folded over rows 1..=3, insert "X" at offset 6, delete 6..7, then append "!" at EOF. Per-edit synchronization keeps the fold; batched synchronization drops it because Known retains only inserted bytes surviving into the final text, so same_rows returns None for the first insertion. Inserting "\n" at offset 4 and deleting 4..5 likewise incorrectly drops a fold that merely moved down and back. Repeated complete header replacements also fail. Unknown intermediate bytes can additionally cause row_start_before to reject an otherwise preserved header.
  fix: Retain sufficient per-batch line-break and row-boundary information, or intermediate snapshots, to distinguish harmless transient edits from actual header splitting/joining. Add transient header text, inserted/deleted whole rows immediately above the header, and repeated header replacements to the per-edit comparison tests through both synchronization paths.
  instances: crates/fe-editor/src/lib.rs:1198, crates/fe-editor/src/lib.rs:1229

- F2 medium crates/fe-editor/src/lib.rs:1144: Fold catch-up now performs quadratic work in the number of pending edit batches. Every splice allocates a new vector and copies all existing pieces; repeated appends create one additional piece per batch. Both Known::new and surviving_folds replay this process, so 10,000 pending appends below an untouched fold require approximately 100 million Piece copies. A dormant split view can therefore stall when synchronized, even though none of those edits affect its fold. The second replay also continues after all folds have been invalidated.
  fix: Avoid rebuilding the complete piece vector for every batch, using an incremental indexed representation or retained per-batch structural information. Stop fold replay when no live folds remain, and verify scaling with many pending edits below an untouched fold.
  instances: crates/fe-editor/src/lib.rs:1144, crates/fe-editor/src/lib.rs:1325
