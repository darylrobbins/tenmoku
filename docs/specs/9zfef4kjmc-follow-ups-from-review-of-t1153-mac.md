---
task_uid: "01m6z297f3nc0rcr9zfef4kjmc"
task: "T536"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "column_select_tests: undo followed directly by a column command (no set_cursor) starts a fresh column box, and so do deletion, redo, a row edit and an edit through a shared buffer; a buffer edit that leaves the resolved selections unchanged still invalidates the box, so removing the version check fails a test (review F2)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-editor and cargo clippy -p fe-editor --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Follow-ups from review of T1153 (Mac T1624)

Findings of the review of T1153 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-editor/src/column_select_tests.rs:74: The undo regression test calls set_cursor before the next column command, unconditionally clearing column_box and masking whether undo invalidation works. The typing case changes both selections and version, so it also cannot independently protect the new version check. Deletion, redo, row edits, and shared-buffer edits requested in F1 remain untested.
  fix: Exercise undo directly followed by column selection without set_cursor, and add deletion, redo, row-edit, and shared-buffer regression cases. Include a buffer edit that leaves resolved selections unchanged so removing the version check makes a test fail.
  instances: crates/fe-editor/src/column_select_tests.rs:61, crates/fe-editor/src/column_select_tests.rs:74
