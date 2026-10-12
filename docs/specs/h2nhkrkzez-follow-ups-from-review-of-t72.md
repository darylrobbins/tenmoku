---
task_uid: "01m4r23brevf6m4dh2nhkrkzez"
task: "T76"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T72

Findings of the review of T72 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/rows.rs:46: Moving identical rows reports Changed despite leaving the text unchanged. For example, move_rows_down on "|a\na" produces "a\n|a"; run_as preserves Changed because the selection moved. The unconditional buffer edit also creates an unnecessary undo transaction and marks the buffer dirty. This violates the spec's requirement that Changed be returned only when text changes.
  fix: Detect text-identical row permutations before committing a buffer edit, and determine row-command effects from text changes rather than selection changes. Add regression tests for both directions with identical non-empty and empty rows.
  instances: crates/fe-editor/src/rows.rs:46, crates/fe-editor/src/rows.rs:320
