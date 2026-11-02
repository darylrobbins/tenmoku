---
task_uid: "01m6afgwsh4fyezga279k4ek88"
task: "T391"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T182

Findings of the review of T182 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/ex/marks.rs:31: The parser treats an Ex comment as mark names. For example, `:marks "abc` should list all marks, but instead filters to a, b and c, potentially returning E283. Likewise, comment text after an explicit filter can add unintended rows. The existing command splitter already treats the quote as a comment delimiter.
  fix: Strip the Ex comment with the existing `uncommented` helper before trimming and storing the arguments. Add regression tests for comments with and without an explicit mark filter.
  instances: crates/fe-vim/src/ex/marks.rs:31
