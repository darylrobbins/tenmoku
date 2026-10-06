---
task_uid: "01m47gw7zbcz1ykn94hpdyhr75"
task: "T89"
rev: 1
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T84

Findings of the review of T84 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/commands.rs:527: A version change does not necessarily mean the displayed text changed. For example, start with text "x" and a caret at offset 1, then use buffer_mut().edit to replace 0..1 with "x". Dispatching editor.undo and editor.redo leaves the text, selections, folds and wrap unchanged, but both return Changed because every applied batch advances the version. This leaves the no-op effect requirement incompletely fixed and still causes unnecessary autoscroll.
  fix: Use the version as a fast check, then compare actual text when versions differ, alongside the existing selection and display checks. Preserve returned transaction IDs and add regression tests for undo and redo of identical-text replacements.
  instances: crates/fe-editor/src/commands.rs:527
