---
task_uid: "01m6m2hsmjsm63cccpbdk4f6s4"
task: "T448"
rev: 1
type: "chore"
complexity: "xs"
criteria: []
---

# Follow-ups from review of T269

Findings of the review of T269 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/tests/pending_wrap.rs:57: The dense sample uses buffer row LINES / 2 instead of the viewport top. Narrowing anchors at LINES * 3 / 4, and widening anchors in the inserted long row at LINES. Neither test therefore compares the required 200 rows around its top against the reference; local wrapping errors there can escape the every-11th-row sample and the weaker anchor-containment checks.
  fix: Pass the final viewport top or its anchor into assert_wrapped_as_in_one_go and sample 200 display rows around that position, while retaining the cursor, stride, tail, and row-count checks.
  instances: crates/fe-editor/src/tests/pending_wrap.rs:57, crates/fe-editor/src/tests/pending_wrap.rs:92, crates/fe-editor/src/tests/pending_wrap.rs:118
