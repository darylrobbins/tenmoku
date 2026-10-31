---
task_uid: "01m688ywnr5rz75bxjqm3hec94"
task: "T381"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "A DiffView side's lone caret that was moved to offset 0 stays visible across frames and refolds; an untouched initial caret still does not hold a block open (diff_fold_tests)"
    required: true
    manual: false
---

# Follow-ups from review of T380: a moved lone caret at offset 0 is never folded away

Finding of the review of T380 (Mac T1224) that doesn't block the merge (below merge.block_severity):

- F3 medium crates/fe-gpui/src/diff_fold.rs:426: The secondary-caret case is fixed, but the exception that keeps a DiffView's untouched initial caret from being folded away still identifies it by its position (a lone caret at offset 0), so a caret the user has moved to offset 0 can be treated as untouched and become hidden when the diff refolds.
  fix: Track whether each side's initial caret has been touched, preserving that state across frames and refolds, so a moved lone caret at offset 0 is never folded away while a caret nobody has touched still doesn't hold a block open.
  instances: crates/fe-gpui/src/diff_fold.rs:386, crates/fe-gpui/src/diff_fold.rs:426

Add a diff_fold_tests case: move a side's lone caret to offset 0 (or onto row 0 from elsewhere), refold (edit or toggle), and the row stays visible; an untouched initial caret still doesn't.
