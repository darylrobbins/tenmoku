---
task_uid: "01m4n0k7p79xcdxh9804xwm138"
task: "T346"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T337

Findings of the review of T337 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/ex.rs:217: The repeat-substitute branch validates endpoints before checking command-level restrictions. From line 1, `:-2&!` now reports E16, whereas Neovim reports E477: No ! allowed. This leaves the requested E477 precedence incomplete for a recognized command.
  fix: Reject an immediate bang after `&` with ExError::NoBang before invoking the endpoint check, and add message assertions covering negative ranges with `&!` alongside the delete-bang case.
  instances: crates/fe-vim/src/ex.rs:217
