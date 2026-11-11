---
task_uid: "01m710ksn3dvrgjp99z7pfbr2g"
task: "T315"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T246

Findings of the review of T246 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/row_insert.rs:93: Redo puts the caret on the original row for both Insert Above and Copy Up. apply_row_edits does not save the supplied selections for history, so redo places carets at insertion ends. For example, Insert Above on '  fo|o', undo, redo produces '  \n|  foo' instead of '  |\n  foo'; Copy Up on 'a|b' similarly redoes to 'ab\n|ab'. Subsequent typing edits the wrong row. The new tests check only text after undo and miss this transition.
  fix: Record and restore the intended redo caret/selection positions for both commands, including multiple carets, and add command → undo → redo assertions covering those positions.
  instances: crates/fe-editor/src/row_insert.rs:93, crates/fe-editor/src/row_insert.rs:130
