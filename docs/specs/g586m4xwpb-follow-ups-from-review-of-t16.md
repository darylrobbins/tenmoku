---
task_uid: "01m5tbbbnztq1856g586m4xwpb"
task: "T347"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T16

Findings of the review of T16 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/typing.rs:138: Typing transactions record only their ending carets, leaving undo to use each restored edit's new.end. Three forward deletes from '|hello' therefore undo to 'h|ello', instead of '|hello' as the spec requires. Runs beginning with forward deletion, including mixed-delete and multi-caret runs, have the same problem. The mixed-delete test checks restored text only and misses the incorrect caret.
  fix: Record and restore the carets where each typing transaction began, including multiple carets. Add caret assertions for forward-delete and mixed-delete groups.
  instances: crates/fe-editor/src/typing.rs:138
